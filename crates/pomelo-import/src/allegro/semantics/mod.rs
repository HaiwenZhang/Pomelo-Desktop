//! Allegro source relationships and geometry, independent of the application and GPU.

pub mod bond;
pub mod connectivity;
pub mod copper;
pub mod drawing;
pub mod geometry;
pub mod layers;
pub mod pad;
pub mod padstack;
pub mod placement;
pub mod scene;
pub mod text;
pub mod units;

#[derive(Debug, Clone)]
pub struct CacheLimits {
    pub max_bytes: usize,
    pub max_entries: usize,
}

/// Optional annotation chains keep missing links as warnings, but cycles and budgets are fatal.
struct AnnotationChain<'a> {
    seen: std::collections::HashSet<super::index::RecordKey>,
    limits: &'a super::database::ChainLimits,
}
impl<'a> AnnotationChain<'a> {
    fn new(limits: &'a super::database::ChainLimits) -> Self {
        Self {
            seen: std::collections::HashSet::new(),
            limits,
        }
    }
    fn contains(&self, key: super::index::RecordKey) -> bool {
        self.seen.contains(&key)
    }
    fn visit(
        &mut self,
        key: super::index::RecordKey,
        origin: super::database::ReferenceLocation,
        context: &crate::ImportContext<'_>,
    ) -> Result<(), crate::ImportError> {
        context.check_cancelled()?;
        if self.seen.contains(&key) {
            return Err(crate::ImportError::ReferenceCycle {
                key: key.0,
                offset: origin.offset.0 as usize,
                field: origin.field,
            });
        }
        let count = self.seen.len().saturating_add(1);
        if count > self.limits.max_records {
            return Err(crate::ImportError::InvalidRecord {
                offset: origin.offset.0 as usize,
                field: "CHAIN_RECORD_COUNT",
                value: count as u64,
            });
        }
        let bytes = count.saturating_mul(64);
        if bytes > self.limits.max_visit_bytes {
            return Err(crate::ImportError::DecodeLimit {
                offset: origin.offset.0 as usize,
                actual: bytes as u64,
                limit: self.limits.max_visit_bytes as u64,
            });
        }
        self.seen.insert(key);
        Ok(())
    }
}

fn annotation_warning(
    budget: &mut CacheBudget,
    diagnostics: &mut Vec<pomelo_core::model::Diagnostic>,
    code: &'static str,
    message: pomelo_core::i18n::Message,
    key: u32,
    offset: usize,
) -> Result<(), crate::ImportError> {
    // Covers owned message arguments, code, and Vec capacity growth for the diagnostic.
    let bytes = 1024;
    budget.check(bytes, offset)?;
    budget.commit(bytes);
    let mut diagnostic = pomelo_core::model::Diagnostic::error(code, message.key);
    diagnostic.severity = pomelo_core::model::Severity::Warning;
    diagnostic.message = message;
    diagnostic.object = Some(pomelo_core::model::ObjectId(key));
    diagnostic.offset = Some(offset as u64);
    diagnostics.push(diagnostic);
    Ok(())
}
impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024 * 1024,
            max_entries: 100_000,
        }
    }
}

/// Accounting for a single build-owned cache, including collection growth.
struct CacheBudget {
    limits: CacheLimits,
    bytes: usize,
    entries: usize,
}

impl CacheBudget {
    fn new(limits: CacheLimits) -> Self {
        Self {
            limits,
            bytes: 0,
            entries: 0,
        }
    }

    fn available(&self) -> usize {
        self.limits.max_bytes.saturating_sub(self.bytes)
    }

    fn check(&self, bytes: usize, offset: usize) -> Result<(), crate::ImportError> {
        let entries = self.entries.saturating_add(1);
        if entries > self.limits.max_entries {
            return Err(crate::ImportError::InvalidRecord {
                offset,
                field: "SEMANTIC_CACHE_ENTRY_COUNT",
                value: entries as u64,
            });
        }
        let actual = self.bytes.saturating_add(bytes);
        if actual > self.limits.max_bytes {
            return Err(crate::ImportError::SemanticCacheLimit {
                offset,
                actual: actual as u64,
                limit: self.limits.max_bytes as u64,
            });
        }
        Ok(())
    }

    /// Commit only successful inserts; a failed decode must not consume the budget.
    fn commit(&mut self, bytes: usize) {
        self.bytes += bytes;
        self.entries += 1;
    }
}
