//! Immutable source and index ownership, on-demand decoding, and bounded reference traversal.

use super::{
    decoder::{DecodeLimits, DecodedRecord, RecordDecoder},
    header::BrdHeader,
    index::{BrdIndex, FileOffset, IndexLimits, RecordKey, RecordSpan},
};
use crate::{ImportContext, ImportError, ImportOptions, TextEncoding};
use std::{collections::HashSet, path::Path};

/// One source buffer and its validated index; queries never retain a complete record AST.
pub struct BrdDatabase {
    bytes: Vec<u8>,
    index: BrdIndex,
    encoding: TextEncoding,
    decode_limits: DecodeLimits,
}

#[derive(Debug)]
pub struct LocatedRecord {
    pub span: RecordSpan,
    pub fields: DecodedRecord,
}

/// The record containing a reference, rather than the missing target's presumed position.
#[derive(Debug, Clone, Copy)]
pub struct ReferenceLocation {
    pub offset: FileOffset,
    pub field: &'static str,
}

#[derive(Debug, Clone)]
pub struct ChainLimits {
    pub max_records: usize,
    /// Conservative HashSet accounting, including growth and bucket slack.
    pub max_visit_bytes: usize,
}
impl Default for ChainLimits {
    fn default() -> Self {
        Self {
            max_records: 1_000_000,
            max_visit_bytes: 64 * 1024 * 1024,
        }
    }
}

/// The caller supplies each list's sentinel and known record types; zero is always an end link.
pub struct ChainRequest<'a> {
    pub start: RecordKey,
    pub terminator: RecordKey,
    /// Empty means any supported record type, otherwise only these tags are allowed.
    pub expected_types: &'a [u8],
    pub origin: ReferenceLocation,
    pub link_field: &'static str,
    pub limits: ChainLimits,
}

impl BrdDatabase {
    pub fn read(
        bytes: Vec<u8>,
        options: &ImportOptions,
        index_limits: &IndexLimits,
        decode_limits: DecodeLimits,
        context: &ImportContext<'_>,
    ) -> Result<Self, ImportError> {
        let index = BrdIndex::read(&bytes, options, index_limits, context)?;
        let encoding = index.encoding;
        Ok(Self {
            bytes,
            index,
            encoding,
            decode_limits,
        })
    }

    pub fn read_path(
        path: &Path,
        options: &ImportOptions,
        index_limits: &IndexLimits,
        decode_limits: DecodeLimits,
        context: &ImportContext<'_>,
    ) -> Result<Self, ImportError> {
        let bytes = crate::source::read_path(path, options, context)?;
        Self::read(bytes, options, index_limits, decode_limits, context)
    }

    pub fn header(&self) -> &BrdHeader {
        &self.index.header
    }
    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }
    pub fn index(&self) -> &BrdIndex {
        &self.index
    }
    pub fn source_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn string(&self, key: u32) -> Option<&str> {
        self.index.strings.get(&key).map(String::as_str)
    }

    /// Optional lookup, including zero and absent keys. Required links use require_record.
    pub fn get(
        &self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<LocatedRecord>, ImportError> {
        context.check_cancelled()?;
        self.index
            .record(key)
            .map(|span| self.decode_span(*span, context))
            .transpose()
    }

    /// Only exact indexed boundaries are accepted; this also addresses records with key zero.
    pub fn get_at_offset(
        &self,
        offset: super::index::FileOffset,
        context: &ImportContext<'_>,
    ) -> Result<Option<LocatedRecord>, ImportError> {
        context.check_cancelled()?;
        let records = self.index.records();
        records
            .binary_search_by_key(&offset.0, |span| span.offset.0)
            .ok()
            .map(|position| self.decode_span(records[position], context))
            .transpose()
    }

    /// Lazy traversal in source order, returning independently owned records.
    pub fn records_of_type<'a>(
        &'a self,
        kind: u8,
        context: &'a ImportContext<'_>,
    ) -> impl Iterator<Item = Result<LocatedRecord, ImportError>> + 'a {
        self.index
            .records_of_type(kind)
            .map(move |span| self.decode_span(*span, context))
    }

    pub fn require_record(
        &self,
        key: RecordKey,
        expected_types: &[u8],
        origin: ReferenceLocation,
        context: &ImportContext<'_>,
    ) -> Result<LocatedRecord, ImportError> {
        context.check_cancelled()?;
        let span = self
            .index
            .record(key)
            .ok_or(ImportError::MissingReference {
                key: key.0,
                offset: origin.offset.0 as usize,
                field: origin.field,
            })?;
        if !expected_types.is_empty() && !expected_types.contains(&span.record_type) {
            return Err(ImportError::ReferenceType {
                key: key.0,
                actual: span.record_type,
                expected: expected_types.to_vec(),
                offset: origin.offset.0 as usize,
                field: origin.field,
            });
        }
        self.decode_span(*span, context)
    }

    /// Visit one decoded record at a time. Cycles, missing/type-invalid links and limits fail
    /// explicitly; partial visitor output is the caller's responsibility and must not be published.
    pub fn walk_chain(
        &self,
        request: &ChainRequest<'_>,
        context: &ImportContext<'_>,
        mut next: impl FnMut(&LocatedRecord) -> Result<RecordKey, ImportError>,
        mut visit: impl FnMut(LocatedRecord) -> Result<(), ImportError>,
    ) -> Result<usize, ImportError> {
        context.check_cancelled()?;
        let mut current = request.start;
        let mut origin = request.origin;
        let mut visited = HashSet::new();
        while current.0 != 0 && current != request.terminator {
            context.check_cancelled()?;
            if visited.contains(&current) {
                return Err(ImportError::ReferenceCycle {
                    key: current.0,
                    offset: origin.offset.0 as usize,
                    field: origin.field,
                });
            }
            let count = visited.len().saturating_add(1);
            if count > request.limits.max_records {
                return Err(ImportError::InvalidRecord {
                    offset: origin.offset.0 as usize,
                    field: "CHAIN_RECORD_COUNT",
                    value: count as u64,
                });
            }
            let bytes = count.saturating_mul(64);
            if bytes > request.limits.max_visit_bytes {
                return Err(ImportError::DecodeLimit {
                    offset: origin.offset.0 as usize,
                    actual: bytes as u64,
                    limit: request.limits.max_visit_bytes as u64,
                });
            }
            let record = self.require_record(current, request.expected_types, origin, context)?;
            visited.insert(current);
            current = next(&record)?;
            origin = ReferenceLocation {
                offset: record.span.offset,
                field: request.link_field,
            };
            visit(record)?;
            context.check_cancelled()?;
        }
        Ok(visited.len())
    }

    fn decode_span(
        &self,
        span: RecordSpan,
        context: &ImportContext<'_>,
    ) -> Result<LocatedRecord, ImportError> {
        let fields = RecordDecoder::new(&self.bytes, self.header(), self.encoding).decode(
            &span,
            &self.decode_limits,
            context,
        )?;
        Ok(LocatedRecord { span, fields })
    }
}
