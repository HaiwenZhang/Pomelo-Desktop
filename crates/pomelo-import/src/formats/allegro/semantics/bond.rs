//! Verified TOP bond-wire relationships; unknown source variants remain diagnostic.

use super::{
    CacheBudget, CacheLimits,
    padstack::{PadstackResolver, bond_finger_angle},
};
use crate::{
    ImportContext, ImportError,
    formats::allegro::{
        database::{BrdDatabase, ChainLimits, LocatedRecord},
        decoder::{
            DecodedRecord,
            fixed::{FixedRecord, Track},
            variable::{FieldValue, VariableRecord},
        },
        index::RecordKey,
    },
};
use pomelo_core::{
    model::{BondWireInfo, NetId, ObjectId, Point},
    task::{ImportProgress, ImportStage},
};
use std::collections::{HashMap, HashSet};

/// Source track endpoints, rather than the finger's unrelated network Next link.
pub struct BondLinks {
    pins: HashMap<RecordKey, Option<RecordKey>>,
}

impl BondLinks {
    pub fn build(
        database: &BrdDatabase,
        limits: CacheLimits,
        context: &ImportContext<'_>,
    ) -> Result<Self, ImportError> {
        let mut pins = HashMap::new();
        let mut budget = CacheBudget::new(limits);
        for (index, record) in database.records_of_type(5, context).enumerate() {
            let record = record?;
            let DecodedRecord::Fixed(FixedRecord::Track(track)) = record.fields else {
                continue;
            };
            if track.layer == 0xfd06 {
                let finger = typed(database, track.unknown5a.unwrap_or(0), 0x33, context)?;
                let pin = typed(database, track.unknown4.unwrap_or(0), 0x32, context)?;
                if let (
                    Some(DecodedRecord::Fixed(FixedRecord::Via(finger))),
                    Some(DecodedRecord::Fixed(FixedRecord::PlacedPad(pin))),
                ) = (finger, pin)
                    && pin.parent_fp == track.unknown_ptr2a
                    && finger.unknown_ptr2 == Some(pin.parent_fp)
                {
                    match pins.get_mut(&finger.key) {
                        Some(previous) => {
                            if *previous != Some(pin.key) {
                                *previous = None;
                            }
                        }
                        None => {
                            budget.check(64, record.span.offset.0 as usize)?;
                            pins.insert(finger.key, Some(pin.key));
                            budget.commit(64);
                        }
                    }
                }
            }
            if index.is_multiple_of(256) {
                context.check_cancelled()?;
                (context.progress)(ImportProgress {
                    stage: ImportStage::BuildingGeometry,
                    completed: index as u64,
                    total: None,
                });
            }
        }
        context.check_cancelled()?;
        Ok(Self { pins })
    }

    pub fn pin(&self, finger: RecordKey) -> Option<RecordKey> {
        self.pins.get(&finger).copied().flatten()
    }
}

/// Avoid materializing a different, potentially large variable record for an optional link.
pub(super) fn typed(
    database: &BrdDatabase,
    key: u32,
    kind: u8,
    context: &ImportContext<'_>,
) -> Result<Option<DecodedRecord>, ImportError> {
    context.check_cancelled()?;
    if !database
        .index()
        .record(RecordKey(key))
        .is_some_and(|span| span.record_type == kind)
    {
        return Ok(None);
    }
    Ok(database
        .get(RecordKey(key), context)?
        .map(|record| record.fields))
}

pub(super) struct ResolvedWire {
    pub segment: LocatedRecord,
    pub net: NetId,
    pub info: BondWireInfo,
}

pub(super) fn resolve_wire(
    database: &BrdDatabase,
    track: &Track,
    stacks: &mut PadstackResolver<'_>,
    chain: &ChainLimits,
    max_labels: usize,
    context: &ImportContext<'_>,
) -> Result<Option<ResolvedWire>, ImportError> {
    if database.header().version < 172 || track.layer != 0xfd06 || track.unknown_ptr1 != 160 {
        return Ok(None);
    }
    let Some(DecodedRecord::Fixed(FixedRecord::PlacedPad(pin))) =
        typed(database, track.unknown4.unwrap_or(0), 0x32, context)?
    else {
        return Ok(None);
    };
    let Some(DecodedRecord::Fixed(FixedRecord::Via(finger))) =
        typed(database, track.unknown5a.unwrap_or(0), 0x33, context)?
    else {
        return Ok(None);
    };
    let Some(DecodedRecord::Fixed(FixedRecord::FootprintInstance(fp))) =
        typed(database, track.unknown_ptr2a, 0x2d, context)?
    else {
        return Ok(None);
    };
    if fp.layer != 0 || pin.parent_fp != fp.key.0 || finger.unknown_ptr2 != Some(fp.key.0) {
        return Ok(None);
    }
    if !database
        .index()
        .record(RecordKey(track.first_seg_ptr))
        .is_some_and(|span| span.record_type == 0x16)
    {
        return Ok(None);
    }
    let Some(segment) = database.get(RecordKey(track.first_seg_ptr), context)? else {
        return Ok(None);
    };
    let DecodedRecord::Fixed(FixedRecord::Segment(edge)) = &segment.fields else {
        return Ok(None);
    };
    if edge.parent != track.key.0
        || edge.next != track.key.0
        || edge.flags != 160
        || edge.width == 0
        || (edge.start_x == edge.end_x && edge.start_y == edge.end_y)
    {
        return Ok(None);
    }
    let Some(DecodedRecord::Fixed(FixedRecord::Pad(pad))) =
        typed(database, pin.pad_ptr, 0x0d, context)?
    else {
        return Ok(None);
    };
    if !stacks
        .resolve_pin(RecordKey(pad.pad_stack), pin.key, context)?
        .is_some_and(|resolved| resolved.die)
    {
        return Ok(None);
    }
    // Wire variants require a direct TOP finger stack, unlike general via placement.
    let Some(finger_stack) = stacks.definition(RecordKey(finger.padstack), context)? else {
        return Ok(None);
    };
    if finger_stack.start_layer != 0
        || bond_finger_angle(&finger, &finger_stack, stacks.layer_count()).is_none()
    {
        return Ok(None);
    }
    let Some(DecodedRecord::Fixed(FixedRecord::NetAssignment(net))) =
        typed(database, track.net_assignment, 4, context)?
    else {
        return Ok(None);
    };
    if net.net == 0 {
        return Ok(None);
    }
    for key in [pin.net_ptr, finger.net_ptr] {
        if !matches!(typed(database, key, 4, context)?, Some(DecodedRecord::Fixed(FixedRecord::NetAssignment(assignment))) if assignment.net == net.net)
        {
            return Ok(None);
        }
    }
    // Wire endpoint validation uses the unrounded source position; pin placement rounds.
    let at = Point::new(f64::from(pad.coords_x), f64::from(pad.coords_y))
        .rotate(super::placement::rotation(fp.rotation));
    let at = Point::new(at.x + f64::from(fp.coord_x), at.y + f64::from(fp.coord_y));
    if at.distance(Point::new(f64::from(edge.start_x), f64::from(edge.start_y))) > 1.0
        || edge.end_x != finger.coords_x
        || edge.end_y != finger.coords_y
    {
        return Ok(None);
    }
    let mut key = track.unknown_ptr5;
    let mut visited = HashSet::new();
    let mut profile = None;
    let mut material = None;
    while key != 0 && key != track.key.0 {
        context.check_cancelled()?;
        if visited.contains(&key) {
            return Ok(None);
        }
        let count = visited.len().saturating_add(1);
        if count > chain.max_records {
            return Err(ImportError::InvalidRecord {
                offset: segment.span.offset.0 as usize,
                field: "BOND_ATTRIBUTE_COUNT",
                value: count as u64,
            });
        }
        let actual = count.saturating_mul(64);
        if actual > chain.max_visit_bytes {
            return Err(ImportError::DecodeLimit {
                offset: segment.span.offset.0 as usize,
                actual: actual as u64,
                limit: chain.max_visit_bytes as u64,
            });
        }
        visited.insert(key);
        let Some(DecodedRecord::Variable(VariableRecord::Field(field))) =
            typed(database, key, 3, context)?
        else {
            return Ok(None);
        };
        if field.sub_type == 104 {
            let target = match field.hdr1 {
                400 => Some(&mut profile),
                540 => Some(&mut material),
                _ => None,
            };
            if let Some(target) = target {
                if target.is_some() {
                    return Ok(None);
                }
                *target = field.value;
            }
        }
        key = field.next;
    }
    let Some(FieldValue::Text(profile)) = profile else {
        return Ok(None);
    };
    if key != track.key.0 || profile != "TOP" {
        return Ok(None);
    }
    let material = match material {
        None => None,
        Some(FieldValue::Text(value)) => Some(value),
        _ => return Ok(None),
    };
    let component = typed(database, fp.inst_ref.unwrap_or(0), 7, context)?;
    let reference = match &component {
        Some(DecodedRecord::Fixed(FixedRecord::ComponentInstance(value))) => value
            .ref_des
            .as_deref()
            .or_else(|| value.ref_des_str_ptr.and_then(|key| database.string(key)))
            .unwrap_or(""),
        _ => "",
    };
    let pin_name = pad
        .name
        .as_deref()
        .or_else(|| pad.name_str_id.and_then(|key| database.string(key)))
        .unwrap_or("");
    let actual = profile
        .len()
        .saturating_add(material.as_ref().map_or(0, String::len))
        .saturating_add(reference.len())
        .saturating_add(pin_name.len());
    if actual > max_labels {
        return Err(ImportError::SemanticCacheLimit {
            offset: segment.span.offset.0 as usize,
            actual: actual as u64,
            limit: max_labels as u64,
        });
    }
    Ok(Some(ResolvedWire {
        segment,
        net: NetId(net.net),
        info: BondWireInfo {
            profile,
            material,
            source_pin: ObjectId(pin.key.0),
            finger: ObjectId(finger.key.0),
            reference: reference.to_owned(),
            pin_name: pin_name.to_owned(),
        },
    }))
}
