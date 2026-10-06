//! Footprint and pin placement, including source-grid rounding before millimetre conversion.

mod routing;
mod via;

use super::{
    CacheBudget, CacheLimits, bond::BondLinks, connectivity::NetworkMap, geometry::GeometryLimits,
    pad::PadDecoder, padstack::PadstackResolver,
};
use crate::{
    ImportContext, ImportError,
    formats::allegro::{
        database::{BrdDatabase, ChainLimits, ChainRequest, ReferenceLocation},
        decoder::{
            DecodedRecord,
            fixed::{FixedRecord, FootprintInstance, Pad as SourcePad, PlacedPad},
        },
        index::RecordKey,
    },
};
use pomelo_core::{
    i18n::{Message, MessageKey},
    model::{
        ComponentPlacement, Diagnostic, DiePad, LayerId, NetId, ObjectId, Pin, Point, Severity,
        StackupRegion,
    },
    task::{ImportProgress, ImportStage},
};
use serde::Serialize;
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone)]
pub struct PlacementLimits {
    /// Returned objects, strings, diagnostics and vector growth; definition caches are separate.
    pub objects: CacheLimits,
    pub chain: ChainLimits,
    pub stacks: CacheLimits,
    pub pads: CacheLimits,
    pub geometry: GeometryLimits,
    pub links: CacheLimits,
}
impl Default for PlacementLimits {
    fn default() -> Self {
        Self {
            objects: CacheLimits {
                max_bytes: 2 * 1024 * 1024 * 1024,
                max_entries: 8_000_000,
            },
            chain: ChainLimits::default(),
            stacks: CacheLimits::default(),
            pads: CacheLimits::default(),
            geometry: GeometryLimits::default(),
            links: CacheLimits::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlacedFootprint {
    pub component: ComponentPlacement,
    pub pins: Vec<Pin>,
}

/// A build owns all caches and budgets; cancellation cannot poison a future import.
pub struct PlacementDecoder<'a> {
    database: &'a BrdDatabase,
    networks: &'a NetworkMap,
    layers: u32,
    stacks: PadstackResolver<'a>,
    pads: PadDecoder<'a>,
    budget: CacheBudget,
    object_limit_bytes: usize,
    chain: ChainLimits,
    diagnostics: Vec<Diagnostic>,
    placed_count: usize,
    geometry_limits: GeometryLimits,
    bond_links: Option<BondLinks>,
    link_limits: CacheLimits,
    via_pads: HashMap<(RecordKey, via::LayerMode), Arc<[pomelo_core::model::Pad]>>,
    backdrill_pads: HashMap<via::BackdrillKey, Arc<[pomelo_core::model::Pad]>>,
}

impl<'a> PlacementDecoder<'a> {
    pub fn new(
        database: &'a BrdDatabase,
        networks: &'a NetworkMap,
        layers: u32,
        limits: PlacementLimits,
    ) -> Result<Self, ImportError> {
        if layers == 0 || layers > 256 {
            return Err(ImportError::InvalidRecord {
                offset: 0,
                field: "PLACEMENT_LAYER_COUNT",
                value: layers as u64,
            });
        }
        Ok(Self {
            database,
            networks,
            layers,
            stacks: PadstackResolver::new(database, layers, limits.stacks),
            pads: PadDecoder::new(database, limits.pads, limits.geometry.clone())?,
            object_limit_bytes: limits.objects.max_bytes,
            budget: CacheBudget::new(limits.objects),
            chain: limits.chain,
            diagnostics: Vec::new(),
            placed_count: 0,
            geometry_limits: limits.geometry,
            bond_links: None,
            link_limits: limits.links,
            via_pads: HashMap::new(),
            backdrill_pads: HashMap::new(),
        })
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        let mut diagnostics = std::mem::take(&mut self.diagnostics);
        diagnostics.extend(self.pads.take_diagnostics());
        diagnostics
    }

    pub(super) fn output_bytes(&self) -> usize {
        self.budget.bytes
    }

    /// Scene assembly supplies the remaining aggregate output budget before each query.
    pub(super) fn bound_next_output(&mut self, bytes: usize) {
        self.budget.limits.max_bytes = self
            .object_limit_bytes
            .min(self.budget.bytes.saturating_add(bytes));
    }

    fn warning(
        &mut self,
        pin: RecordKey,
        stack: u32,
        key: MessageKey,
        offset: usize,
    ) -> Result<(), ImportError> {
        let mut message = Message::new(key).arg("pin", pin.0);
        if key == MessageKey::PinDefinitionUnsupported {
            message = message.arg("stack", stack);
        }
        let code = match key {
            MessageKey::DieBackUnsupported => "BRD_DIE_BACK_UNSUPPORTED",
            _ => "BRD_PIN_DEFINITION_UNSUPPORTED",
        };
        self.report(code, message, pin, offset)
    }

    fn report(
        &mut self,
        code: &'static str,
        message: Message,
        object: RecordKey,
        offset: usize,
    ) -> Result<(), ImportError> {
        self.budget.check(1024, offset)?;
        self.diagnostics.push(Diagnostic {
            code: code.into(),
            severity: Severity::Warning,
            message,
            offset: Some(offset as u64),
            object: Some(ObjectId(object.0)),
            path: None,
            technical_details: None,
        });
        self.budget.commit(1024);
        Ok(())
    }

    /// Visit NextInFp, never the unrelated network Next or NextInCompInst links.
    pub fn footprint(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<PlacedFootprint, ImportError> {
        let database = self.database;
        let origin = database
            .index()
            .record(key)
            .map_or(0, |span| span.offset.0 as usize);
        let located = database.require_record(
            key,
            &[0x2d],
            ReferenceLocation {
                offset: super::super::index::FileOffset(origin as u32),
                field: "Footprint",
            },
            context,
        )?;
        let DecodedRecord::Fixed(FixedRecord::FootprintInstance(fp)) = located.fields else {
            return Err(ImportError::InvalidRecord {
                offset: origin,
                field: "FOOTPRINT_RECORD_TYPE",
                value: 0x2d,
            });
        };
        let source_reference = fp.inst_ref.filter(|key| *key != 0).map(ObjectId);
        let component = database.get(RecordKey(fp.inst_ref.unwrap_or(0)), context)?;
        let reference = match component.as_ref().map(|record| &record.fields) {
            Some(DecodedRecord::Fixed(FixedRecord::ComponentInstance(component))) => component
                .ref_des
                .as_deref()
                .or_else(|| {
                    component
                        .ref_des_str_ptr
                        .and_then(|key| database.string(key))
                })
                .unwrap_or(""),
            _ => "",
        };
        let charge = 512_usize.saturating_add(reference.len());
        self.budget.check(charge, origin)?;
        let mut output = PlacedFootprint {
            component: ComponentPlacement {
                id: ObjectId(key.0),
                source_reference,
                reference: reference.to_owned(),
                at: Point::new(
                    f64::from(fp.coord_x) * self.pads.scale(),
                    f64::from(fp.coord_y) * self.pads.scale(),
                ),
                angle: rotation(fp.rotation),
                mirrored: fp.layer != 0,
                pins: Vec::new(),
            },
            pins: Vec::new(),
        };
        self.budget.commit(charge);
        database.walk_chain(
            &ChainRequest {
                start: RecordKey(fp.first_pad_ptr),
                terminator: fp.key,
                expected_types: &[0x32],
                origin: ReferenceLocation {
                    offset: located.span.offset,
                    field: "FirstPadPtr",
                },
                link_field: "NextInFp",
                limits: self.chain.clone(),
            },
            context,
            |record| match &record.fields {
                DecodedRecord::Fixed(FixedRecord::PlacedPad(p)) => Ok(RecordKey(p.next_in_fp)),
                _ => Err(ImportError::InvalidRecord {
                    offset: record.span.offset.0 as usize,
                    field: "PLACED_PAD_RECORD_TYPE",
                    value: record.span.record_type as u64,
                }),
            },
            |record| {
                let DecodedRecord::Fixed(FixedRecord::PlacedPad(placed)) = record.fields else {
                    return Err(ImportError::InvalidRecord {
                        offset: record.span.offset.0 as usize,
                        field: "PLACED_PAD_RECORD_TYPE",
                        value: record.span.record_type as u64,
                    });
                };
                if placed.parent_fp != fp.key.0 {
                    return Err(ImportError::InvalidRecord {
                        offset: record.span.offset.0 as usize,
                        field: "ParentFp",
                        value: placed.parent_fp as u64,
                    });
                }
                if let Some(pin) = self.pin(
                    &placed,
                    &fp,
                    &output.component.reference,
                    record.span.offset.0 as usize,
                    context,
                )? {
                    output.component.pins.push(pin.id);
                    output.pins.push(pin);
                }
                self.placed_count += 1;
                if self.placed_count.is_multiple_of(256) {
                    (context.progress)(ImportProgress {
                        stage: ImportStage::BuildingGeometry,
                        completed: self.placed_count as u64,
                        total: None,
                    });
                }
                Ok(())
            },
        )?;
        context.check_cancelled()?;
        Ok(output)
    }

    fn pin(
        &mut self,
        placed: &PlacedPad,
        fp: &FootprintInstance,
        reference: &str,
        origin: usize,
        context: &ImportContext<'_>,
    ) -> Result<Option<Pin>, ImportError> {
        context.check_cancelled()?;
        let pad = match self
            .database
            .get(RecordKey(placed.pad_ptr), context)?
            .map(|record| record.fields)
        {
            Some(DecodedRecord::Fixed(FixedRecord::Pad(pad))) => pad,
            _ => {
                self.warning(placed.key, 0, MessageKey::PinDefinitionUnsupported, origin)?;
                return Ok(None);
            }
        };
        let Some(resolved) =
            self.stacks
                .resolve_pin(RecordKey(pad.pad_stack), placed.key, context)?
        else {
            self.warning(
                placed.key,
                pad.pad_stack,
                MessageKey::PinDefinitionUnsupported,
                origin,
            )?;
            return Ok(None);
        };
        if resolved.die && fp.layer != 0 {
            self.warning(
                placed.key,
                pad.pad_stack,
                MessageKey::DieBackUnsupported,
                origin,
            )?;
            return Ok(None);
        }
        let stack = &resolved.stack;
        let name = pad
            .name
            .as_deref()
            .or_else(|| pad.name_str_id.and_then(|key| self.database.string(key)))
            .unwrap_or("");
        // Covers returned pad vectors, component ID list growth and copied source names.
        // Custom contours remain immutable Arc references accounted by PadDecoder's cache.
        let charge = 1024_usize
            .saturating_add(reference.len())
            .saturating_add(name.len())
            .saturating_add(if resolved.die {
                self.database.string(stack.pad_str).unwrap_or("").len()
            } else {
                0
            })
            .saturating_add(
                usize::from(stack.layer_count)
                    .saturating_mul(2 * std::mem::size_of::<pomelo_core::model::Pad>()),
            );
        self.budget.check(charge, origin)?;
        let mut shapes = Vec::new();
        let angle = rotation(fp.rotation);
        let local = rotation(pad.rotation);
        for i in 0..u32::from(stack.layer_count) {
            context.check_cancelled()?;
            let index = stack
                .num_fixed_comp_entries
                .saturating_add(stack.num_comps_per_layer.saturating_mul(i as usize))
                .saturating_add(2);
            let p = stack
                .components
                .get(index)
                .ok_or(ImportError::InvalidGeometry {
                    key: stack.key.0,
                    offset: origin,
                    field: "REGULAR_PAD_COMPONENT",
                })?;
            if p.r#type == 0 {
                continue;
            }
            let layer = if resolved.die {
                LayerId::BOND_TOP
            } else if let Some(layer) = resolved.embedded_layer {
                layer
            } else {
                let layer = if fp.layer != 0 {
                    i64::from(self.layers) - 1 - i64::from(stack.start_layer) - i64::from(i)
                } else {
                    i64::from(stack.start_layer) + i64::from(i)
                };
                if layer < 0 || layer >= i64::from(self.layers) {
                    return Err(ImportError::InvalidGeometry {
                        key: placed.key.0,
                        offset: origin,
                        field: "PIN_LAYER",
                    });
                }
                LayerId(layer as u32)
            };
            let mut offset = Point::new(
                f64::from(p.offset_x) * self.pads.scale(),
                f64::from(p.offset_y) * self.pads.scale(),
            )
            .rotate(local);
            if fp.layer != 0 {
                offset.x = -offset.x;
            }
            offset = offset.rotate(angle);
            if let Some(value) = self.pads.shape(p, layer, offset, stack.key, context)? {
                shapes.push(value);
            }
        }
        let net = match self
            .database
            .get(RecordKey(placed.net_ptr), context)?
            .map(|record| record.fields)
        {
            Some(DecodedRecord::Fixed(FixedRecord::NetAssignment(assignment))) => {
                NetId(assignment.net)
            }
            _ => self.networks.owner(placed.key).unwrap_or(NetId(0)),
        };
        let pin = Pin {
            id: ObjectId(placed.key.0),
            owner_id: ObjectId(fp.key.0),
            net,
            name: name.to_owned(),
            reference: reference.to_owned(),
            at: pin_centre(fp, &pad, self.pads.scale()),
            angle: angle
                + if fp.layer != 0 {
                    std::f64::consts::PI - local
                } else {
                    local
                },
            mirrored: fp.layer != 0,
            drill: f64::from(stack.drill_size) * self.pads.scale(),
            drill_shape: self.pads.drill(stack, context)?,
            pads: shapes,
            stackup_region: resolved.region_code.map(|code| StackupRegion {
                source_reference: ObjectId(pad.pad_stack),
                code,
            }),
            die: resolved.die.then(|| DiePad {
                source_reference: ObjectId(pad.pad_stack),
                padstack_name: self.database.string(stack.pad_str).unwrap_or("").to_owned(),
            }),
        };
        context.check_cancelled()?;
        self.budget.commit(charge);
        Ok(Some(pin))
    }
}

pub(super) fn rotation(value: u32) -> f64 {
    f64::from(value) * std::f64::consts::PI / 180_000.0
}

/// Allegro placement rounds rotated offsets in source-grid units, with ties away from zero.
fn pin_centre(fp: &FootprintInstance, pad: &SourcePad, scale: f64) -> Point {
    let offset = Point::new(
        if fp.layer != 0 {
            -f64::from(pad.coords_x)
        } else {
            f64::from(pad.coords_x)
        },
        f64::from(pad.coords_y),
    )
    .rotate(rotation(fp.rotation));
    Point::new(
        (f64::from(fp.coord_x) + offset.x.round()) * scale,
        (f64::from(fp.coord_y) + offset.y.round()) * scale,
    )
}
