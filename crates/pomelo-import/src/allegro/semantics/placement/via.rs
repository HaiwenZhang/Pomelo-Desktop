use super::PlacementDecoder;
use crate::{
    ImportContext, ImportError,
    allegro::{
        database::ReferenceLocation,
        decoder::{DecodedRecord, fixed::FixedRecord, variable::Padstack},
        index::{FileOffset, RecordKey},
        semantics::{
            bond::{BondLinks, typed},
            pad::apply_backdrill,
            padstack::bond_finger_angle,
        },
    },
};
use pomelo_core::{
    i18n::{Message, MessageKey},
    model::{
        Backdrill, BackdrillDefinition, BondFinger, LayerId, NetId, ObjectId, Pad, Point,
        StackupRegion, Via,
    },
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum LayerMode {
    Normal,
    Reverse,
    Flip,
}
impl LayerMode {
    fn from_bits(bits: u16) -> Self {
        if bits & 0x3000 == 0x3000 {
            Self::Reverse
        } else if bits & 0x2000 != 0 {
            Self::Flip
        } else {
            Self::Normal
        }
    }
    fn map(
        self,
        stack: &Padstack,
        index: u32,
        layers: u32,
        offset: usize,
    ) -> Result<LayerId, ImportError> {
        let layer = match self {
            Self::Normal => i64::from(stack.start_layer) + i64::from(index),
            Self::Reverse => {
                i64::from(layers) - 1 - i64::from(stack.start_layer) - i64::from(index)
            }
            Self::Flip => {
                i64::from(layers) - i64::from(stack.start_layer) - i64::from(stack.layer_count)
                    + i64::from(index)
            }
        };
        if layer < 0 || layer >= i64::from(layers) {
            return Err(ImportError::InvalidGeometry {
                key: stack.key.0,
                offset,
                field: "VIA_LAYER",
            });
        }
        Ok(LayerId(layer as u32))
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(super) struct BackdrillKey {
    stack: RecordKey,
    mode: LayerMode,
    spans: [Option<(u32, u32, u32)>; 2],
}
impl BackdrillKey {
    fn new(
        stack: RecordKey,
        mode: LayerMode,
        definition: &BackdrillDefinition,
        offset: usize,
    ) -> Result<Self, ImportError> {
        if definition.spans.len() > 2 {
            return Err(ImportError::InvalidGeometry {
                key: stack.0,
                offset,
                field: "BACKDRILL_SPAN_COUNT",
            });
        }
        let mut spans = [None; 2];
        for (target, span) in spans.iter_mut().zip(&definition.spans) {
            *target = Some((
                span.start_layer.0,
                span.stop_layer.0,
                span.protected_layer.0,
            ));
        }
        Ok(Self { stack, mode, spans })
    }
}

impl PlacementDecoder<'_> {
    /// Place one ordinary via or bond finger, sharing immutable stack pad recipes.
    pub fn via(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<Via>, ImportError> {
        let offset = self
            .database
            .index()
            .record(key)
            .map_or(0, |span| span.offset.0 as usize);
        let record = self.database.require_record(
            key,
            &[0x33],
            ReferenceLocation {
                offset: FileOffset(offset as u32),
                field: "Via",
            },
            context,
        )?;
        let DecodedRecord::Fixed(FixedRecord::Via(source)) = record.fields else {
            return Err(ImportError::InvalidGeometry {
                key: key.0,
                offset,
                field: "VIA_RECORD_TYPE",
            });
        };
        let Some(resolved) =
            self.stacks
                .resolve_via(RecordKey(source.padstack), source.key, context)?
        else {
            self.report(
                "BRD_VIA_DEFINITION_UNSUPPORTED",
                Message::new(MessageKey::ViaDefinitionUnsupported)
                    .arg("via", key.0)
                    .arg("stack", source.padstack),
                key,
                offset,
            )?;
            return Ok(None);
        };
        let stack = &resolved.stack;
        let mode = LayerMode::from_bits(source.layer_info);
        let span = if stack.layer_count == 0 {
            None
        } else {
            let first = mode.map(stack, 0, self.layers, offset)?;
            let last = mode.map(stack, u32::from(stack.layer_count) - 1, self.layers, offset)?;
            Some((LayerId(first.0.min(last.0)), LayerId(first.0.max(last.0))))
        };
        let mut pads = self.via_pad_recipe(stack, mode, offset, context)?;
        let mut angle = 0.0;
        let mut finger = None;
        if stack.pad_type == Some(30) {
            let Some(placement) = bond_finger_angle(&source, stack, self.layers) else {
                self.report(
                    "BRD_BOND_FINGER_UNSUPPORTED",
                    Message::new(MessageKey::BondFingerUnsupported).arg("finger", key.0),
                    key,
                    offset,
                )?;
                return Ok(None);
            };
            angle = placement;
            if self.bond_links.is_none() {
                self.bond_links = Some(BondLinks::build(
                    self.database,
                    self.link_limits.clone(),
                    context,
                )?);
            }
            let fp = typed(
                self.database,
                source.unknown_ptr2.unwrap_or(0),
                0x2d,
                context,
            )?;
            let fp = match fp {
                Some(DecodedRecord::Fixed(FixedRecord::FootprintInstance(value))) => Some(value),
                _ => None,
            };
            let component = typed(
                self.database,
                fp.as_ref().and_then(|fp| fp.inst_ref).unwrap_or(0),
                7,
                context,
            )?;
            let reference = match &component {
                Some(DecodedRecord::Fixed(FixedRecord::ComponentInstance(value))) => value
                    .ref_des
                    .as_deref()
                    .or_else(|| {
                        value
                            .ref_des_str_ptr
                            .and_then(|key| self.database.string(key))
                    })
                    .unwrap_or(""),
                _ => "",
            };
            let pin_key = self.bond_links.as_ref().and_then(|links| links.pin(key));
            let pin = typed(self.database, pin_key.map_or(0, |key| key.0), 0x32, context)?;
            let pin = match pin {
                Some(DecodedRecord::Fixed(FixedRecord::PlacedPad(value)))
                    if fp.as_ref().is_some_and(|fp| fp.key.0 == value.parent_fp) =>
                {
                    Some(value)
                }
                _ => None,
            };
            let pin_pad = typed(
                self.database,
                pin.as_ref().map_or(0, |pin| pin.pad_ptr),
                0x0d,
                context,
            )?;
            let pin_pad = match pin_pad {
                Some(DecodedRecord::Fixed(FixedRecord::Pad(value))) => Some(value),
                _ => None,
            };
            let name = pin_pad
                .as_ref()
                .and_then(|pad| {
                    pad.name
                        .as_deref()
                        .or_else(|| pad.name_str_id.and_then(|key| self.database.string(key)))
                })
                .unwrap_or("");
            let charge = 128_usize
                .saturating_add(reference.len())
                .saturating_add(name.len())
                .saturating_add(pads.len().saturating_mul(2 * std::mem::size_of::<Pad>()));
            self.budget.check(charge, offset)?;
            finger = Some(BondFinger {
                reference: reference.to_owned(),
                name: name.to_owned(),
                source_pin: pin_pad
                    .as_ref()
                    .and(pin.as_ref())
                    .map(|pin| ObjectId(pin.key.0)),
            });
            pads = pads
                .iter()
                .map(|pad| {
                    let mut pad = pad.clone();
                    pad.offset = pad.offset.rotate(angle);
                    pad
                })
                .collect::<Vec<_>>()
                .into();
            self.budget.commit(charge);
        }
        let backdrill = if let Some(source_definition) = resolved.backdrill {
            let definition = source_definition.to_millimetres(self.pads.scale());
            let cache_key = BackdrillKey::new(stack.key, mode, &definition, offset)?;
            if let Some(effective) = self.backdrill_pads.get(&cache_key) {
                pads = Arc::clone(effective);
            } else {
                // Up to two spans, each with a base and display circle per physical layer.
                let count = pads.len().saturating_add(4 * self.layers as usize);
                let charge =
                    128_usize.saturating_add(count.saturating_mul(2 * std::mem::size_of::<Pad>()));
                self.budget.check(charge, offset)?;
                let effective: Arc<[Pad]> =
                    apply_backdrill(&pads, &definition, self.layers as u16, context)?.into();
                self.backdrill_pads
                    .insert(cache_key, Arc::clone(&effective));
                self.budget.commit(charge);
                pads = effective;
            }
            Some(Backdrill {
                definition,
                source_reference: ObjectId(source.padstack),
                rotation_degrees: f64::from(source.unknown5) / 1000.0,
                mirrored: source.layer_info & 0x100 != 0,
            })
        } else {
            None
        };
        let name = self.database.string(stack.pad_str).unwrap_or("");
        let charge = 512_usize.saturating_add(name.len());
        self.budget.check(charge, offset)?;
        let placed = Via {
            id: ObjectId(source.key.0),
            net: self.networks.owner(source.key).unwrap_or(NetId(0)),
            at: Point::new(
                f64::from(source.coords_x) * self.pads.scale(),
                f64::from(source.coords_y) * self.pads.scale(),
            ),
            drill: f64::from(stack.drill_size) * self.pads.scale(),
            drill_shape: self.pads.drill(stack, context)?,
            padstack: ObjectId(stack.key.0),
            padstack_name: name.to_owned(),
            start_layer: span.map(|(first, _)| first),
            end_layer: span.map(|(_, last)| last),
            pads,
            backdrill,
            stackup_region: resolved.region_code.map(|code| StackupRegion {
                source_reference: ObjectId(source.padstack),
                code,
            }),
            angle,
            mirrored: false,
            finger,
        };
        context.check_cancelled()?;
        self.budget.commit(charge);
        Ok(Some(placed))
    }

    fn via_pad_recipe(
        &mut self,
        stack: &Padstack,
        mode: LayerMode,
        offset: usize,
        context: &ImportContext<'_>,
    ) -> Result<Arc<[Pad]>, ImportError> {
        context.check_cancelled()?;
        if let Some(pads) = self.via_pads.get(&(stack.key, mode)) {
            return Ok(Arc::clone(pads));
        }
        let charge = 128_usize.saturating_add(
            usize::from(stack.layer_count).saturating_mul(2 * std::mem::size_of::<Pad>()),
        );
        self.budget.check(charge, offset)?;
        let mut pads = Vec::new();
        for index in 0..u32::from(stack.layer_count) {
            context.check_cancelled()?;
            let component = stack
                .components
                .get(stack.num_fixed_comp_entries + stack.num_comps_per_layer * index as usize + 2)
                .ok_or(ImportError::InvalidGeometry {
                    key: stack.key.0,
                    offset,
                    field: "REGULAR_PAD_COMPONENT",
                })?;
            let layer = mode.map(stack, index, self.layers, offset)?;
            let at = Point::new(
                f64::from(component.offset_x) * self.pads.scale(),
                f64::from(component.offset_y) * self.pads.scale(),
            );
            if let Some(pad) = self.pads.shape(component, layer, at, stack.key, context)? {
                pads.push(pad);
            }
        }
        let pads: Arc<[Pad]> = pads.into();
        self.via_pads.insert((stack.key, mode), Arc::clone(&pads));
        self.budget.commit(charge);
        Ok(pads)
    }
}
