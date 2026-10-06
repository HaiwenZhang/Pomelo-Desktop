//! Shape identity follows source ownership, independently of mesh geometry and GPU policy.

use super::{AnnotationChain, CacheBudget};
use crate::{
    ImportContext, ImportError,
    formats::allegro::{
        database::{BrdDatabase, ChainLimits, LocatedRecord, ReferenceLocation},
        decoder::{DecodedRecord, fixed::FixedRecord},
        index::RecordKey,
    },
};
use pomelo_core::model::ZoneKind;
use std::collections::BTreeSet;

pub(super) struct ShapeKinds {
    dynamic: BTreeSet<u32>,
    complete: bool,
}

impl ShapeKinds {
    pub(super) fn build(
        database: &BrdDatabase,
        layers: u32,
        limits: &ChainLimits,
        budget: &mut CacheBudget,
        context: &ImportContext<'_>,
    ) -> Result<Self, ImportError> {
        let mut result = Self {
            dynamic: BTreeSet::new(),
            complete: database.header().version >= 172,
        };
        for record in database.records_of_type(0x28, context) {
            let record = record?;
            let DecodedRecord::Fixed(FixedRecord::Shape(boundary)) = record.fields else {
                continue;
            };
            if boundary.layer & 255 != 21 || u32::from(boundary.layer >> 8) >= layers {
                continue;
            }
            let mut origin = ReferenceLocation {
                offset: record.span.offset,
                field: "Shape.TablePtr",
            };
            let mut key = RecordKey(boundary.table_ptr.or(boundary.table_ptr_16x).unwrap_or(0));
            let mut seen = AnnotationChain::new(limits);
            let table = loop {
                if key.0 == 0 {
                    result.complete = false;
                    break None;
                }
                seen.visit(key, origin, context)?;
                let Some(linked) = metadata_record(database, key, origin, context)? else {
                    result.complete = false;
                    break None;
                };
                match linked.fields {
                    DecodedRecord::Fixed(FixedRecord::MatchGroup(wrapper))
                        if wrapper.unknown1 == Some(boundary.key.0) =>
                    {
                        key = RecordKey(wrapper.member_ptr);
                        origin = ReferenceLocation {
                            offset: linked.span.offset,
                            field: "MemberPtr",
                        };
                    }
                    DecodedRecord::Fixed(FixedRecord::Table(table))
                        if table.sub_type == 12
                            && table.next == boundary.key.0
                            && table.unknown1 == Some(boundary.key.0) =>
                    {
                        break Some((table, linked.span.offset));
                    }
                    _ => {
                        result.complete = false;
                        break None;
                    }
                }
            };
            let Some((table, table_offset)) = table else {
                continue;
            };
            // BOUNDARY ownership is independently used by KiCad's Allegro importer.
            // Group wrappers may precede the owning table; TablePtr != 0 alone proves nothing.
            let mut array_seen = AnnotationChain::new(limits);
            key = RecordKey(table.ptr1);
            let mut previous = table.key;
            origin = ReferenceLocation {
                offset: table_offset,
                field: "Ptr1",
            };
            while key.0 != 0 && key != table.key {
                array_seen.visit(key, origin, context)?;
                let Some(linked) = metadata_record(database, key, origin, context)? else {
                    result.complete = false;
                    break;
                };
                let DecodedRecord::Fixed(FixedRecord::PointerArray(array)) = linked.fields else {
                    result.complete = false;
                    break;
                };
                // The first array points back to the table, later chunks to their predecessor.
                // RDIMM's multi-chunk fills prove that GroupPtr is not always the table key.
                if array.group_ptr != previous.0
                    // Nonzero Unknown2 arrays can contain scalar values, not only keys.
                    // Their layout is unproved (e.g. 16178 raw10 includes integer 1/1).
                    || array.unknown2 != 0
                    || array.count > array.capacity
                    || array.count as usize > array.ptrs.len()
                {
                    result.complete = false;
                    break;
                }
                for &member in array.ptrs.iter().take(array.count as usize) {
                    context.check_cancelled()?;
                    if member == 0 {
                        continue;
                    }
                    let member = RecordKey(member);
                    let member_origin = ReferenceLocation {
                        offset: linked.span.offset,
                        field: "Ptrs",
                    };
                    // Non-shape members (e.g. the owning net) need existence, not decoding.
                    let span =
                        database
                            .index()
                            .record(member)
                            .ok_or(ImportError::MissingReference {
                                key: member.0,
                                offset: member_origin.offset.0 as usize,
                                field: member_origin.field,
                            })?;
                    if span.record_type != 0x28 {
                        continue;
                    }
                    let member_record =
                        database.require_record(member, &[], member_origin, context)?;
                    if let DecodedRecord::Fixed(FixedRecord::Shape(fill)) = member_record.fields
                        && fill.layer & 255 == 6
                    {
                        if fill.layer >> 8 != boundary.layer >> 8 {
                            result.complete = false;
                            continue;
                        }
                        if !result.dynamic.contains(&member.0) {
                            budget.check(64, origin.offset.0 as usize)?;
                            result.dynamic.insert(member.0);
                            budget.commit(64);
                        }
                    }
                }
                previous = array.key;
                key = RecordKey(array.next);
                origin = ReferenceLocation {
                    offset: linked.span.offset,
                    field: "Next",
                };
            }
        }
        Ok(result)
    }

    pub(super) fn classify(&self, key: RecordKey, raw_state: Option<u32>) -> ZoneKind {
        if self.dynamic.contains(&key.0) {
            ZoneKind::Dynamic
        } else if self.complete && raw_state == Some(1) {
            // Demo's standalone raw1 ETCH shape is actually static under Allegro's
            // static_shapes_fill_solid toggle. Auto-generated fillets (e.g. 4097/12289)
            // and unproved states remain Unknown; do not infer Unknown2 bit 8.
            ZoneKind::Static
        } else {
            ZoneKind::Unknown
        }
    }
}

fn metadata_record(
    database: &BrdDatabase,
    key: RecordKey,
    origin: ReferenceLocation,
    context: &ImportContext<'_>,
) -> Result<Option<LocatedRecord>, ImportError> {
    match database.require_record(key, &[], origin, context) {
        Ok(record) => Ok(Some(record)),
        // Optional identity metadata must not make previously valid geometry unimportable.
        // Required references, cancellation and resource limits keep their existing errors.
        Err(
            ImportError::UnsupportedRecordLayout { .. }
            | ImportError::InvalidRecord { .. }
            | ImportError::InvalidEncoding(_),
        ) => Ok(None),
        Err(error) => Err(error),
    }
}
