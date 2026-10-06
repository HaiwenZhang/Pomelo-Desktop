//! Board-wide network ownership from source-order Allegro connection lists.

use super::{CacheBudget, CacheLimits};
use crate::{
    ImportContext, ImportError,
    formats::allegro::{
        database::{BrdDatabase, ChainLimits, ChainRequest, ReferenceLocation},
        decoder::{DecodedRecord, fixed::FixedRecord},
        index::RecordKey,
    },
};
use pomelo_core::{
    model::NetId,
    task::{ImportProgress, ImportStage},
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct NetworkLimits {
    /// Persistent names and ownership maps; chain scratch storage is bounded separately.
    pub maps: CacheLimits,
    pub chain: ChainLimits,
    /// Total visits, including overlapping chains, bound adversarial repeated work.
    pub max_link_visits: usize,
}

impl Default for NetworkLimits {
    fn default() -> Self {
        Self {
            maps: CacheLimits {
                max_bytes: 512 * 1024 * 1024,
                max_entries: 8_000_000,
            },
            chain: ChainLimits::default(),
            max_link_visits: 64_000_000,
        }
    }
}

/// An immutable build result. Absence and an explicitly assigned net zero remain distinct.
#[derive(Debug, Serialize)]
pub struct NetworkMap {
    pub nets: BTreeMap<NetId, String>,
    owners: HashMap<RecordKey, NetId>,
    pub assignment_count: usize,
    pub link_visits: usize,
    pub overwritten_owners: usize,
    pub owner_types: BTreeMap<u8, usize>,
}

impl NetworkMap {
    pub fn owner(&self, key: RecordKey) -> Option<NetId> {
        self.owners.get(&key).copied()
    }

    pub fn owners(&self) -> &HashMap<RecordKey, NetId> {
        &self.owners
    }

    /// All chains must succeed before this result can be published as scene data.
    pub fn build(
        database: &BrdDatabase,
        limits: &NetworkLimits,
        context: &ImportContext<'_>,
    ) -> Result<Self, ImportError> {
        context.check_cancelled()?;
        let mut output = Self {
            nets: BTreeMap::new(),
            owners: HashMap::new(),
            assignment_count: 0,
            link_visits: 0,
            overwritten_owners: 0,
            owner_types: BTreeMap::new(),
        };
        let mut budget = CacheBudget::new(limits.maps.clone());
        let progress = |completed| {
            (context.progress)(ImportProgress {
                stage: ImportStage::BuildingGeometry,
                completed,
                total: None,
            })
        };
        progress(0);
        for record in database.records_of_type(0x1b, context) {
            let record = record?;
            let DecodedRecord::Fixed(FixedRecord::Net(net)) = record.fields else {
                return Err(ImportError::InvalidRecord {
                    offset: record.span.offset.0 as usize,
                    field: "NETWORK_RECORD_TYPE",
                    value: 0x1b,
                });
            };
            let name = database.string(net.net_name).unwrap_or("");
            let charge = 128_usize.saturating_add(name.len());
            budget.check(charge, record.span.offset.0 as usize)?;
            output.nets.insert(NetId(net.key.0), name.to_owned());
            budget.commit(charge);
            if output.nets.len().is_multiple_of(256) {
                progress(output.nets.len() as u64);
                context.check_cancelled()?;
            }
        }
        for record in database.records_of_type(4, context) {
            let record = record?;
            let DecodedRecord::Fixed(FixedRecord::NetAssignment(assignment)) = record.fields else {
                return Err(ImportError::InvalidRecord {
                    offset: record.span.offset.0 as usize,
                    field: "NETWORK_ASSIGNMENT_TYPE",
                    value: 4,
                });
            };
            output.assignment_count += 1;
            database.walk_chain(
                &ChainRequest {
                    start: RecordKey(assignment.conn_item),
                    terminator: assignment.key,
                    expected_types: &[],
                    origin: ReferenceLocation {
                        offset: record.span.offset,
                        field: "ConnItem",
                    },
                    link_field: "Next",
                    limits: limits.chain.clone(),
                },
                context,
                |record| Ok(record.fields.next_key().unwrap_or(RecordKey(0))),
                |record| {
                    let visits = output.link_visits.saturating_add(1);
                    if visits > limits.max_link_visits {
                        return Err(ImportError::InvalidRecord {
                            offset: record.span.offset.0 as usize,
                            field: "NETWORK_LINK_VISITS",
                            value: visits as u64,
                        });
                    }
                    let key = record.span.key;
                    if let Some(owner) = output.owners.get_mut(&key) {
                        // Web semantics: later assignments in source order take precedence.
                        *owner = NetId(assignment.net);
                        output.overwritten_owners += 1;
                    } else {
                        let charge = 64
                            + if output.owner_types.contains_key(&record.span.record_type) {
                                0
                            } else {
                                512
                            };
                        budget.check(charge, record.span.offset.0 as usize)?;
                        output.owners.insert(key, NetId(assignment.net));
                        *output
                            .owner_types
                            .entry(record.span.record_type)
                            .or_default() += 1;
                        budget.commit(charge);
                    }
                    output.link_visits = visits;
                    if visits.is_multiple_of(256) {
                        progress((output.nets.len() + visits) as u64);
                    }
                    Ok(())
                },
            )?;
            if output.assignment_count.is_multiple_of(256) {
                progress((output.nets.len() + output.link_visits) as u64);
                context.check_cancelled()?;
            }
        }
        context.check_cancelled()?;
        Ok(output)
    }
}
