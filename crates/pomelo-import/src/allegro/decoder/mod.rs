//! Source records decoded on demand. Scene construction owns semantic normalization.

pub mod fixed;
pub mod variable;

use super::{header::BrdHeader, index::RecordSpan, reader::Reader};
use crate::{ImportContext, ImportError, TextEncoding};
use serde::Serialize;

/// Limits for one on-demand record; no decoded whole-board cache is retained.
#[derive(Debug, Clone)]
pub struct DecodeLimits {
    /// Conservative accounting for owned payloads, vectors and text expansion.
    pub max_allocation_bytes: usize,
    pub max_text_bytes: usize,
    pub max_entries: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_allocation_bytes: 64 * 1024 * 1024,
            max_text_bytes: 16 * 1024 * 1024,
            max_entries: 1_000_000,
        }
    }
}

/// Type-specific source fields before scene-level unit and connectivity normalization.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DecodedRecord {
    Fixed(fixed::FixedRecord),
    Variable(variable::VariableRecord),
}

impl DecodedRecord {
    pub fn next_key(&self) -> Option<super::index::RecordKey> {
        match self {
            Self::Fixed(record) => record.next_key(),
            Self::Variable(record) => record.next_key(),
        }
    }

    pub fn key(&self) -> super::index::RecordKey {
        match self {
            Self::Fixed(record) => record.key(),
            Self::Variable(record) => record.key(),
        }
    }
}

/// Borrows the immutable source; decoded records are not cached across queries.
pub struct RecordDecoder<'a> {
    bytes: &'a [u8],
    header: &'a BrdHeader,
    encoding: TextEncoding,
}

impl<'a> RecordDecoder<'a> {
    pub fn new(bytes: &'a [u8], header: &'a BrdHeader, encoding: TextEncoding) -> Self {
        Self {
            bytes,
            header,
            encoding,
        }
    }

    /// Decode one known fixed layout, preserving absolute file offsets and alignment.
    pub fn decode_fixed(
        &self,
        span: &RecordSpan,
        context: &ImportContext<'_>,
    ) -> Result<fixed::FixedRecord, ImportError> {
        self.decode_with(
            span,
            context,
            |reader, kind| fixed::read(reader, kind, self.header.version),
            fixed::FixedRecord::key,
        )
    }

    /// Decode fixed or variable records using the validated index span as a hard boundary.
    pub fn decode(
        &self,
        span: &RecordSpan,
        limits: &DecodeLimits,
        context: &ImportContext<'_>,
    ) -> Result<DecodedRecord, ImportError> {
        context.check_cancelled()?;
        // Large opaque source regions are skipped without allocation. Bound materialized
        // payloads in the decoder, while decode_with enforces the full indexed source span.
        self.decode_with(
            span,
            context,
            |reader, kind| {
                if variable::supports(kind) {
                    variable::read(reader, kind, self.header, limits, context)
                        .map(DecodedRecord::Variable)
                } else {
                    fixed::read(reader, kind, self.header.version).map(DecodedRecord::Fixed)
                }
            },
            DecodedRecord::key,
        )
    }

    fn decode_with<T>(
        &self,
        span: &RecordSpan,
        context: &ImportContext<'_>,
        read: impl FnOnce(&mut Reader<'_>, u8) -> Result<T, ImportError>,
        key: impl Fn(&T) -> super::index::RecordKey,
    ) -> Result<T, ImportError> {
        context.check_cancelled()?;
        let start = span.offset.0 as usize;
        if !start.is_multiple_of(4) {
            return Err(ImportError::UnalignedRecord(start));
        }
        let end = start
            .checked_add(span.byte_length as usize)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(ImportError::OutOfBounds {
                offset: start,
                requested: span.byte_length as usize,
                length: self.bytes.len(),
            })?;
        let mut reader = Reader::new(&self.bytes[..end], self.encoding);
        reader.seek(start)?;
        let kind = reader.record_type(self.header.version)?;
        if kind != span.record_type {
            return Err(ImportError::InvalidRecord {
                offset: start,
                field: "RECORD_TYPE",
                value: u64::from(kind),
            });
        }
        let record = read(&mut reader, kind).map_err(|error| match error {
            ImportError::OutOfBounds {
                offset, requested, ..
            } => ImportError::InvalidRecord {
                offset: start,
                field: "RECORD_BOUNDARY",
                value: offset.saturating_add(requested) as u64,
            },
            other => other,
        })?;
        if reader.offset() != end {
            return Err(ImportError::InvalidRecord {
                offset: start,
                field: "RECORD_BOUNDARY",
                value: reader.offset() as u64,
            });
        }
        if key(&record) != span.key {
            return Err(ImportError::InvalidRecord {
                offset: start,
                field: "RECORD_KEY",
                value: u64::from(key(&record).0),
            });
        }
        context.check_cancelled()?;
        Ok(record)
    }
}
