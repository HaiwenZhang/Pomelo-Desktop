//! Verified padstack references. Unknown 0x2f variants never invent physical layers.

use super::{CacheBudget, CacheLimits};
use crate::{
    ImportContext, ImportError,
    allegro::{
        database::BrdDatabase,
        decoder::{
            DecodedRecord,
            fixed::{FixedRecord, UnknownRecord0x2f, Via},
            variable::{Padstack, PadstackComponent, VariableRecord},
        },
        index::RecordKey,
    },
};
use pomelo_core::model::{BackdrillDefinition, BackdrillSpan, LayerId};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Serialize)]
pub struct ResolvedStack {
    pub stack: Arc<Padstack>,
    pub embedded_layer: Option<LayerId>,
    pub region_code: Option<u32>,
    pub die: bool,
    pub backdrill: Option<SourceBackdrill>,
}

impl ResolvedStack {
    fn plain(stack: Arc<Padstack>) -> Self {
        Self {
            stack,
            embedded_layer: None,
            region_code: None,
            die: false,
            backdrill: None,
        }
    }
}

/// Dimensions stay in source units until the board-scene boundary converts them once.
#[derive(Debug, Serialize)]
pub struct SourceBackdrill {
    pub spans: Vec<BackdrillSpan>,
    pub display_diameter: f64,
    pub start_pad_diameter: f64,
    pub label_diameter: f64,
}

impl SourceBackdrill {
    pub fn to_millimetres(&self, scale: f64) -> BackdrillDefinition {
        BackdrillDefinition {
            spans: self.spans.clone(),
            display_diameter: self.display_diameter * scale,
            start_pad_diameter: self.start_pad_diameter * scale,
            label_diameter: self.label_diameter * scale,
        }
    }
}

/// Definitions are immutable and shared by all placed pads in one scene build.
pub struct PadstackResolver<'a> {
    database: &'a BrdDatabase,
    layer_count: u32,
    stacks: HashMap<RecordKey, Arc<Padstack>>,
    budget: CacheBudget,
}

impl<'a> PadstackResolver<'a> {
    pub fn layer_count(&self) -> u32 {
        self.layer_count
    }

    pub fn new(database: &'a BrdDatabase, layer_count: u32, limits: CacheLimits) -> Self {
        Self {
            database,
            layer_count,
            stacks: HashMap::new(),
            budget: CacheBudget::new(limits),
        }
    }

    pub fn definition(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<Arc<Padstack>>, ImportError> {
        context.check_cancelled()?;
        if let Some(stack) = self.stacks.get(&key) {
            return Ok(Some(Arc::clone(stack)));
        }
        let Some(span) = self
            .database
            .index()
            .record(key)
            .filter(|span| span.record_type == 0x1c)
        else {
            return Ok(None);
        };
        // Includes component/metadata vectors, Arc, map buckets and geometric growth.
        let charge = (span.byte_length as usize)
            .saturating_mul(2)
            .saturating_add(512);
        let offset = span.offset.0 as usize;
        self.budget.check(charge, offset)?;
        let Some(record) = self.database.get(key, context)? else {
            return Ok(None);
        };
        let DecodedRecord::Variable(VariableRecord::Padstack(stack)) = record.fields else {
            return Ok(None);
        };
        context.check_cancelled()?;
        let stack = Arc::new(stack);
        self.stacks.insert(key, Arc::clone(&stack));
        self.budget.commit(charge);
        Ok(Some(stack))
    }

    fn wrapper(
        &self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<UnknownRecord0x2f>, ImportError> {
        context.check_cancelled()?;
        if self
            .database
            .index()
            .record(key)
            .is_none_or(|span| span.record_type != 0x2f)
        {
            return Ok(None);
        }
        let Some(record) = self.database.get(key, context)? else {
            return Ok(None);
        };
        match record.fields {
            DecodedRecord::Fixed(FixedRecord::UnknownRecord0x2f(wrapper)) => Ok(Some(wrapper)),
            _ => Ok(None),
        }
    }

    pub fn resolve_pin(
        &mut self,
        key: RecordKey,
        placed_id: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<ResolvedStack>, ImportError> {
        if let Some(stack) = self.definition(key, context)? {
            return Ok(Some(ResolvedStack::plain(stack)));
        }
        let Some(wrapper) = self.wrapper(key, context)? else {
            return Ok(None);
        };
        let Ok(words) = <&[u32; 6]>::try_from(wrapper.unknown_array.as_slice()) else {
            return Ok(None);
        };
        let layer = u32::from(wrapper.t2 >> 8);
        if wrapper.r#type != 0 || words[1] != placed_id.0 {
            return Ok(None);
        }
        let variant = words[5];
        match variant {
            8 if self.database.header().version >= 172
                && wrapper.t2 == 0xfc00
                && words[2] == 1
                && words[3] == 0
                && words[4] == 0 => {}
            64 if self.database.header().version >= 172
                && wrapper.t2 & 255 == 0
                && layer < self.layer_count
                && words[2] > 0xffff
                && words[2] & 0xffff == 1
                && words[3] == 0
                && words[4] == 0 => {}
            // Word 3 is opaque metadata; native-verified single-layer pads may retain it.
            16 if wrapper.t2 & 255 == 0
                && layer < self.layer_count
                && words[2] == 1
                && words[4] == 0 => {}
            _ => return Ok(None),
        }
        let Some(stack) = self.definition(RecordKey(words[0]), context)? else {
            return Ok(None);
        };
        if stack.layer_count != 1 || stack.drill_size != 0 || stack.slot_y != 0 {
            return Ok(None);
        }
        if variant != 16
            && (stack.start_layer != 0
                || stack.plated
                || stack.slot_x != 0
                || stack.pad_type != Some(if variant == 8 { 26 } else { 10 }))
        {
            return Ok(None);
        }
        let mut resolved = ResolvedStack::plain(stack);
        if variant == 8 {
            resolved.die = true;
        } else {
            resolved.embedded_layer = Some(LayerId(layer));
        }
        if variant == 64 {
            resolved.region_code = Some(words[2] >> 16);
        }
        Ok(Some(resolved))
    }

    pub fn resolve_via(
        &mut self,
        key: RecordKey,
        owner: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<ResolvedStack>, ImportError> {
        if let Some(stack) = self.definition(key, context)? {
            return Ok(Some(ResolvedStack::plain(stack)));
        }
        let Some(wrapper) = self.wrapper(key, context)? else {
            return Ok(None);
        };
        let Ok(words) = <&[u32; 6]>::try_from(wrapper.unknown_array.as_slice()) else {
            return Ok(None);
        };
        if wrapper.r#type != 0 || wrapper.t2 != 0 || words[1] != owner.0 {
            return Ok(None);
        }
        match words[5] {
            64 if self.database.header().version >= 172
                && words[2] > 0xffff
                && words[2] & 0xffff == self.layer_count
                && words[3] == 0
                && words[4] == 0 => {}
            32 if words[2] == self.layer_count
                && words[4] > 0
                && words[4] <= 0xffff
                && (words[4] & 255) + (words[4] >> 8) < self.layer_count => {}
            _ => return Ok(None),
        }
        let Some(stack) = self.definition(RecordKey(words[0]), context)? else {
            return Ok(None);
        };
        if stack.start_layer != 0
            || u32::from(stack.layer_count) != self.layer_count
            || stack.drill_size == 0
            || stack.slot_x != 0
            || stack.slot_y != 0
        {
            return Ok(None);
        }
        if words[5] == 64 {
            if stack.pad_type != Some(4) || !stack.plated {
                return Ok(None);
            }
            let mut resolved = ResolvedStack::plain(stack);
            resolved.region_code = Some(words[2] >> 16);
            return Ok(Some(resolved));
        }
        let Some(backdrill) = decode_backdrill(&stack, self.database.header().version, words[4])
        else {
            return Ok(None);
        };
        let mut resolved = ResolvedStack::plain(stack);
        resolved.backdrill = Some(backdrill);
        Ok(Some(resolved))
    }
}

fn concentric_circle(pad: &PadstackComponent) -> bool {
    pad.r#type == 2
        && pad.width > 0
        && pad.height == pad.width
        && pad.offset_x == 0
        && pad.offset_y == 0
}

fn decode_backdrill(stack: &Padstack, version: u16, encoded: u32) -> Option<SourceBackdrill> {
    if version < 172 || stack.num_fixed_comp_entries != 21 || stack.num_comps_per_layer != 4 {
        return None;
    }
    let metadata = stack.drill_metadata_words.as_ref()?;
    if metadata.len() != if version >= 180 { 29 } else { 21 } {
        return None;
    }
    let display = i64::from(metadata[if version >= 180 { 7 } else { 0 }] as i32).abs() as f64;
    if display <= f64::from(stack.drill_size) {
        return None;
    }
    // BACKDRILL START is slot 5; solder-mask slots 14/15 must never supply this size.
    let start = stack.components.get(5)?;
    if !concentric_circle(start) || i64::from(start.width) < i64::from(stack.drill_size) {
        return None;
    }
    let mut ordinary = 0;
    for layer in 0..usize::from(stack.layer_count) {
        let pad = stack.components.get(21 + 4 * layer + 2)?;
        if pad.r#type != 0 {
            if !concentric_circle(pad) {
                return None;
            }
            ordinary = ordinary.max(pad.width);
        }
    }
    let top = encoded & 255;
    let bottom = encoded >> 8;
    let layers = u32::from(stack.layer_count);
    let mut spans = Vec::with_capacity(2);
    if top > 0 {
        spans.push(BackdrillSpan {
            start_layer: LayerId(0),
            stop_layer: LayerId(top - 1),
            protected_layer: LayerId(top),
        });
    }
    if bottom > 0 {
        spans.push(BackdrillSpan {
            start_layer: LayerId(layers - 1),
            stop_layer: LayerId(layers - bottom),
            protected_layer: LayerId(layers - bottom - 1),
        });
    }
    Some(SourceBackdrill {
        spans,
        display_diameter: display,
        start_pad_diameter: f64::from(start.width),
        label_diameter: f64::from(stack.drill_size).max(f64::from(start.width.min(ordinary))),
    })
}

/// Bond fingers share via storage but retain their own rotation and undrilled padstack.
pub fn bond_finger_angle(via: &Via, stack: &Padstack, layer_count: u32) -> Option<f64> {
    (via.layer_info == 0xc012
        && stack.pad_type == Some(30)
        && stack.layer_count == 1
        && stack.start_layer < layer_count
        && !stack.plated
        && stack.drill_size == 0
        && stack.slot_x == 0
        && stack.slot_y == 0
        && via.unknown5 < 360_000)
        .then_some(f64::from(via.unknown5) * std::f64::consts::PI / 180_000.0)
}
