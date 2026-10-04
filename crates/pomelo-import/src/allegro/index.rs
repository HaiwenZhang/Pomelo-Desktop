//! Compact record boundaries and string lookup, without materializing record ASTs.

use super::{header::BrdHeader, reader::Reader, record_scan::scan_record};
use crate::{ImportContext, ImportError, ImportOptions};
use pomelo_core::task::{ImportProgress, ImportStage};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, hash_map::Entry};
use std::time::{Duration, Instant};

/// Record identity is independent of its position in the source file.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RecordKey(pub u32);

/// Checked source offset; source/index budgets restrict input to at most u32::MAX bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileOffset(pub u32);

#[derive(Debug, Clone, Copy)]
pub struct RecordSpan {
    pub offset: FileOffset,
    pub byte_length: u32,
    pub key: RecordKey,
    pub record_type: u8,
}

#[derive(Debug, Clone)]
pub struct IndexLimits {
    /// Conservative accounting limit for index entries plus decoded strings.
    pub max_index_bytes: u64,
    /// Bound a single decoded payload as well as cancellable string-table scans.
    pub max_text_bytes: usize,
}

impl Default for IndexLimits {
    fn default() -> Self {
        Self {
            max_index_bytes: 2 * 1024 * 1024 * 1024,
            max_text_bytes: 16 * 1024 * 1024,
        }
    }
}

pub struct BrdIndex {
    pub header: BrdHeader,
    pub strings: HashMap<u32, String>,
    pub object_offset: FileOffset,
    pub end_offset: FileOffset,
    records: Vec<RecordSpan>,
    by_key: HashMap<RecordKey, u32>,
    by_type: Vec<Vec<u32>>,
    accounted_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct IndexSummary {
    pub strings: usize,
    pub records: usize,
    pub keyed_records: usize,
    pub object_offset: u32,
    pub end_offset: u32,
    pub accounted_bytes: u64,
    pub by_type: BTreeMap<String, usize>,
}

impl BrdIndex {
    /// Validate every supported record boundary; errors retain source offsets and typed details.
    pub fn read(
        bytes: &[u8],
        options: &ImportOptions,
        limits: &IndexLimits,
        context: &ImportContext<'_>,
    ) -> Result<Self, ImportError> {
        context.check_cancelled()?;
        let file_limit = options.max_file_bytes.min(u32::MAX as u64);
        if bytes.len() as u64 > file_limit {
            return Err(ImportError::ResourceLimit {
                actual: bytes.len() as u64,
                limit: file_limit,
            });
        }
        let header = BrdHeader::read(bytes, options.text_encoding)?;
        let mut reader = Reader::new(bytes, options.text_encoding);
        reader.seek(0x1200)?;
        let mut strings = HashMap::new();
        // Includes per-type Vec headers and growth slack, not just populated elements.
        let mut accounted_bytes = 0_u64;
        reserve_budget(&mut accounted_bytes, 2048, limits)?;
        let mut progress = Progress::new(context, bytes.len());
        progress.emit(reader.offset(), true)?;
        for _ in 0..header.string_count {
            context.check_cancelled()?;
            let offset = reader.offset();
            let key = reader.u32()?;
            let entry = match strings.entry(key) {
                Entry::Occupied(_) => return Err(ImportError::DuplicateString { key, offset }),
                Entry::Vacant(entry) => entry,
            };
            reserve_budget(&mut accounted_bytes, 128, limits)?;
            let remaining = limits.max_index_bytes - accounted_bytes;
            let max_text = limits
                .max_text_bytes
                .min((remaining / 3).min(usize::MAX as u64) as usize);
            let value = reader.bounded_cstring(max_text, context)?;
            reserve_budget(&mut accounted_bytes, 3 * value.len() as u64, limits)?;
            entry.insert(value);
            progress.emit(reader.offset(), false)?;
        }
        let object_offset = FileOffset(reader.offset() as u32);
        let mut index = Self {
            header,
            strings,
            object_offset,
            end_offset: object_offset,
            records: Vec::new(),
            by_key: HashMap::new(),
            by_type: (0..=0x3e).map(|_| Vec::new()).collect(),
            accounted_bytes,
        };
        while reader.offset() < bytes.len() {
            context.check_cancelled()?;
            let offset = reader.offset();
            if !offset.is_multiple_of(4) {
                return Err(ImportError::UnalignedRecord(offset));
            }
            let record_type = reader.record_type(index.header.version)?;
            if record_type == 0 {
                if index.header.version >= 180 {
                    let mut next = reader.offset();
                    while next < bytes.len() && bytes[next] == 0 {
                        if next.is_multiple_of(64 * 1024) {
                            progress.emit(next, false)?;
                        }
                        next += 1;
                    }
                    if next < bytes.len() && next.is_multiple_of(4) && bytes[next] <= 0x3e {
                        reader.seek(next)?;
                        continue;
                    }
                }
                index.end_offset = FileOffset(offset as u32);
                progress.emit(bytes.len(), true)?;
                return Ok(index);
            }
            let key = scan_record(&mut reader, record_type, &index.header, limits, context)?;
            let key = RecordKey(key.unwrap_or(0));
            let entry = if key.0 == 0 {
                None
            } else {
                match index.by_key.entry(key) {
                    Entry::Occupied(_) => {
                        return Err(ImportError::DuplicateRecord { key: key.0, offset });
                    }
                    Entry::Vacant(entry) => Some(entry),
                }
            };
            // Allow Vec doubling and HashMap load-factor slack for all three indexes.
            reserve_budget(&mut index.accounted_bytes, 96, limits)?;
            let record_index = index.records.len() as u32;
            if let Some(entry) = entry {
                entry.insert(record_index);
            }
            index.by_type[usize::from(record_type)].push(record_index);
            index.records.push(RecordSpan {
                offset: FileOffset(offset as u32),
                byte_length: (reader.offset() - offset) as u32,
                key,
                record_type,
            });
            progress.emit(reader.offset(), false)?;
        }
        index.end_offset = FileOffset(reader.offset() as u32);
        progress.emit(bytes.len(), true)?;
        Ok(index)
    }

    pub fn records(&self) -> &[RecordSpan] {
        &self.records
    }

    pub fn record(&self, key: RecordKey) -> Option<&RecordSpan> {
        self.by_key
            .get(&key)
            .map(|&index| &self.records[index as usize])
    }

    pub fn records_of_type(&self, record_type: u8) -> impl Iterator<Item = &RecordSpan> {
        self.by_type
            .get(usize::from(record_type))
            .into_iter()
            .flatten()
            .map(|&index| &self.records[index as usize])
    }

    pub fn summary(&self) -> IndexSummary {
        IndexSummary {
            strings: self.strings.len(),
            records: self.records.len(),
            keyed_records: self.by_key.len(),
            object_offset: self.object_offset.0,
            end_offset: self.end_offset.0,
            accounted_bytes: self.accounted_bytes,
            by_type: self
                .by_type
                .iter()
                .enumerate()
                .filter(|(_, records)| !records.is_empty())
                .map(|(kind, records)| (format!("0x{kind:02x}"), records.len()))
                .collect(),
        }
    }
}

fn reserve_budget(
    accounted: &mut u64,
    amount: u64,
    limits: &IndexLimits,
) -> Result<(), ImportError> {
    let actual = accounted.saturating_add(amount);
    if actual > limits.max_index_bytes {
        return Err(ImportError::IndexLimit {
            actual,
            limit: limits.max_index_bytes,
        });
    }
    *accounted = actual;
    Ok(())
}

struct Progress<'a, 'b> {
    context: &'a ImportContext<'b>,
    total: usize,
    last: Instant,
    last_checked_offset: usize,
}

impl<'a, 'b> Progress<'a, 'b> {
    fn new(context: &'a ImportContext<'b>, total: usize) -> Self {
        Self {
            context,
            total,
            last: Instant::now(),
            last_checked_offset: 0,
        }
    }
    fn emit(&mut self, offset: usize, force: bool) -> Result<(), ImportError> {
        self.context.check_cancelled()?;
        // Cancellation stays per-record. Reading the OS clock for each tiny
        // record is unnecessary for a progress callback capped at 10 Hz.
        if !force && offset.saturating_sub(self.last_checked_offset) < 64 * 1024 {
            return Ok(());
        }
        self.last_checked_offset = offset;
        if force || self.last.elapsed() >= Duration::from_millis(100) {
            (self.context.progress)(ImportProgress {
                stage: ImportStage::Indexing,
                completed: offset as u64,
                total: Some(self.total as u64),
            });
            self.last = Instant::now();
            self.context.check_cancelled()?;
        }
        Ok(())
    }
}
