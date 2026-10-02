//! Copper pad and drill semantics with build-local immutable geometry caches.

use super::{
    CacheBudget, CacheLimits,
    geometry::{GeometryDecoder, GeometryLimits},
    units::millimetres_per_unit,
};
use crate::{
    ImportContext, ImportError,
    allegro::{
        database::BrdDatabase,
        decoder::variable::{Padstack, PadstackComponent},
        index::RecordKey,
    },
};
use pomelo_core::{
    i18n::{Message, MessageKey},
    model::{
        BackdrillDefinition, CustomPadGeometry, Diagnostic, DrillShape, LayerId, ObjectId, Pad,
        PadKind, Point, Segment, Severity,
    },
    pad::PadPlacement,
};
use std::{
    collections::{HashMap, HashSet},
    mem::size_of,
    sync::Arc,
};

pub struct PadDecoder<'a> {
    database: &'a BrdDatabase,
    scale: f64,
    geometry_limits: GeometryLimits,
    budget: CacheBudget,
    custom: HashMap<RecordKey, Arc<CustomPadGeometry>>,
    drills: HashMap<RecordKey, DrillShape>,
    reported: HashSet<(&'static str, RecordKey, u16, u32)>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> PadDecoder<'a> {
    pub fn new(
        database: &'a BrdDatabase,
        limits: CacheLimits,
        geometry_limits: GeometryLimits,
    ) -> Result<Self, ImportError> {
        Ok(Self {
            database,
            scale: millimetres_per_unit(database.header().units, database.header().divisor)?,
            geometry_limits,
            budget: CacheBudget::new(limits),
            custom: HashMap::new(),
            drills: HashMap::new(),
            reported: HashSet::new(),
            diagnostics: Vec::new(),
        })
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    fn offset(&self, key: RecordKey) -> usize {
        self.database
            .index()
            .record(key)
            .map_or(0, |span| span.offset.0 as usize)
    }

    fn report(
        &mut self,
        key: MessageKey,
        stack: RecordKey,
        pad_type: u16,
        shape: u32,
    ) -> Result<(), ImportError> {
        let identity = (key.as_str(), stack, pad_type, shape);
        if self.reported.contains(&identity) {
            return Ok(());
        }
        let offset = self.offset(stack);
        // Covers the diagnostic, parameter map, dedup set and vector growth before allocation.
        self.budget.check(1024, offset)?;
        let mut message = Message::new(key).arg("stack", u64::from(stack.0));
        if key != MessageKey::PadDonut {
            message = message.arg("pad_type", u64::from(pad_type));
        }
        if key == MessageKey::PadUnsupported {
            message = message.arg("shape", u64::from(shape));
        }
        let code = match key {
            MessageKey::PadDonut => "BRD_PAD_DONUT",
            MessageKey::PadUnsupported => "BRD_PAD_UNSUPPORTED",
            _ => "BRD_PAD_DIMENSIONS",
        };
        self.diagnostics.push(Diagnostic {
            code: code.into(),
            severity: Severity::Warning,
            message,
            offset: Some(offset as u64),
            object: Some(ObjectId(stack.0)),
            path: None,
            technical_details: None,
        });
        self.reported.insert(identity);
        self.budget.commit(1024);
        Ok(())
    }

    fn custom_geometry(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Arc<CustomPadGeometry>, ImportError> {
        context.check_cancelled()?;
        if let Some(geometry) = self.custom.get(&key) {
            return Ok(Arc::clone(geometry));
        }
        let offset = self.offset(key);
        self.budget.check(256, offset)?;
        let mut limits = self.geometry_limits.clone();
        limits.max_allocation_bytes = limits
            .max_allocation_bytes
            .min(self.budget.available().saturating_sub(256));
        let contours = GeometryDecoder::new(self.database, limits)?.read_contours(key, context)?;
        let mut charge = 256_usize;
        charge = charge.saturating_add(
            contours
                .paths
                .capacity()
                .saturating_mul(2 * size_of::<Vec<Segment>>()),
        );
        charge = charge.saturating_add(
            contours
                .rings
                .capacity()
                .saturating_mul(2 * size_of::<Vec<Point>>()),
        );
        for path in &contours.paths {
            charge = charge.saturating_add(path.capacity().saturating_mul(size_of::<Segment>()));
        }
        for ring in &contours.rings {
            charge = charge.saturating_add(ring.capacity().saturating_mul(size_of::<Point>()));
        }
        self.budget.check(charge, offset)?;
        context.check_cancelled()?;
        let geometry = Arc::new(CustomPadGeometry {
            contours: contours.rings,
            paths: contours.paths,
        });
        self.custom.insert(key, Arc::clone(&geometry));
        self.budget.commit(charge);
        Ok(geometry)
    }

    /// The caller transforms the component offset into board space before passing it here.
    pub fn shape(
        &mut self,
        component: &PadstackComponent,
        layer: LayerId,
        offset: Point,
        stack: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<Pad>, ImportError> {
        context.check_cancelled()?;
        let kind = component.r#type;
        if kind == 0 {
            return Ok(None);
        }
        let height = if matches!(kind, 2 | 5 | 25) {
            component.width
        } else {
            component.height
        };
        // Zero-size rectangles mean None in Design Layers, with no invented minimum copper.
        if kind == 6 && component.width >= 0 && height >= 0 && (component.width == 0 || height == 0)
        {
            return Ok(None);
        }
        let mut pad = Pad {
            layer,
            width: f64::from(component.width) * self.scale,
            height: f64::from(height) * self.scale,
            offset,
            kind: PadKind(kind),
            corner: if kind == 25 {
                0.0
            } else {
                f64::from(component.z1) * self.scale
            },
            inner_diameter: None,
            custom: None,
            backdrill: false,
            backdrill_base: false,
        };
        if kind == 25 {
            let inner = f64::from(component.z1) * self.scale;
            if inner <= 0.0 || inner >= pad.width || !inner.is_finite() {
                self.report(MessageKey::PadDonut, stack, kind, 0)?;
                return Ok(None);
            }
            pad.inner_diameter = Some(inner);
        }
        if kind == 22 {
            let geometry = self.custom_geometry(RecordKey(component.shape_ptr), context)?;
            if !geometry.contours.is_empty() {
                pad.custom = Some(geometry);
                if (pad.width <= 0.0 || pad.height <= 0.0)
                    && let Some(bounds) = pad.bounds(PadPlacement::default())
                {
                    if pad.width <= 0.0 {
                        pad.width = bounds.max.x - bounds.min.x;
                    }
                    if pad.height <= 0.0 {
                        pad.height = bounds.max.y - bounds.min.y;
                    }
                }
            }
        }
        if !pad.supported() {
            self.report(
                MessageKey::PadUnsupported,
                stack,
                kind,
                if kind == 22 { component.shape_ptr } else { 0 },
            )?;
        }
        if !pad.width.is_finite()
            || !pad.height.is_finite()
            || pad.width <= 0.0
            || pad.height <= 0.0
        {
            self.report(MessageKey::PadDimensions, stack, kind, 0)?;
            return Ok(None);
        }
        if !offset.x.is_finite() || !offset.y.is_finite() {
            return Err(ImportError::InvalidGeometry {
                key: stack.0,
                offset: self.offset(stack),
                field: "PAD_OFFSET",
            });
        }
        Ok(Some(pad))
    }

    pub fn drill(
        &mut self,
        stack: &Padstack,
        context: &ImportContext<'_>,
    ) -> Result<DrillShape, ImportError> {
        context.check_cancelled()?;
        if let Some(drill) = self.drills.get(&stack.key) {
            return Ok(*drill);
        }
        self.budget.check(128, self.offset(stack.key))?;
        let mut width = if stack.slot_y > 0 {
            stack.slot_x
        } else {
            stack.drill_size
        };
        let mut height = if stack.slot_y > 0 {
            stack.slot_y
        } else {
            stack.drill_size
        };
        if width != height
            && stack
                .components
                .get(stack.num_fixed_comp_entries.saturating_add(2))
                .is_some_and(|pad| (pad.height > pad.width) != (height > width))
        {
            std::mem::swap(&mut width, &mut height);
        }
        let drill = DrillShape {
            width: f64::from(width) * self.scale,
            height: f64::from(height) * self.scale,
            plated: stack.plated,
        };
        self.drills.insert(stack.key, drill);
        self.budget.commit(128);
        Ok(drill)
    }

    /// Definition preview in normal physical-layer order; placement is a scene-builder concern.
    pub fn regular_pads(
        &mut self,
        stack: &Padstack,
        context: &ImportContext<'_>,
    ) -> Result<Vec<Pad>, ImportError> {
        let mut pads = Vec::new();
        for index in 0..usize::from(stack.layer_count) {
            context.check_cancelled()?;
            let slot = stack
                .num_fixed_comp_entries
                .checked_add(stack.num_comps_per_layer.saturating_mul(index))
                .and_then(|slot| slot.checked_add(2));
            let component = slot.and_then(|slot| stack.components.get(slot)).ok_or(
                ImportError::InvalidRecord {
                    offset: self.offset(stack.key),
                    field: "PADSTACK_REGULAR_COMPONENT",
                    value: index as u64,
                },
            )?;
            let layer = stack.start_layer.checked_add(index as u32).ok_or(
                ImportError::InvalidGeometry {
                    key: stack.key.0,
                    offset: self.offset(stack.key),
                    field: "PAD_LAYER",
                },
            )?;
            if let Some(pad) = self.shape(
                component,
                LayerId(layer),
                Point::new(
                    f64::from(component.offset_x) * self.scale,
                    f64::from(component.offset_y) * self.scale,
                ),
                stack.key,
                context,
            )? {
                pads.push(pad);
            }
        }
        Ok(pads)
    }
}

/// Apply verified backdrill geometry without touching the protected layer or shared definitions.
pub fn apply_backdrill(
    pads: &[Pad],
    definition: &BackdrillDefinition,
    layer_count: u16,
    context: &ImportContext<'_>,
) -> Result<Vec<Pad>, ImportError> {
    context.check_cancelled()?;
    if layer_count > 256
        || definition.spans.len() > 2
        || ![
            definition.display_diameter,
            definition.start_pad_diameter,
            definition.label_diameter,
        ]
        .into_iter()
        .all(f64::is_finite)
        || definition.display_diameter <= 0.0
        || definition.start_pad_diameter <= 0.0
        || definition.label_diameter < 0.0
        || definition.spans.iter().any(|span| {
            span.start_layer.0 >= u32::from(layer_count)
                || span.stop_layer.0 >= u32::from(layer_count)
                || span.protected_layer.0 >= u32::from(layer_count)
                || definition.contains_layer(span.protected_layer)
        })
    {
        return Err(ImportError::InvalidGeometry {
            key: 0,
            offset: 0,
            field: "BACKDRILL_SPAN",
        });
    }
    let mut effective: Vec<_> = pads
        .iter()
        .filter(|pad| !definition.contains_layer(pad.layer))
        .cloned()
        .collect();
    for span in &definition.spans {
        for layer in
            span.start_layer.0.min(span.stop_layer.0)..=span.start_layer.0.max(span.stop_layer.0)
        {
            context.check_cancelled()?;
            let ordinary = pads.iter().find(|pad| pad.layer.0 == layer);
            let base = if layer == span.start_layer.0 {
                definition.start_pad_diameter
            } else {
                definition
                    .start_pad_diameter
                    .min(ordinary.map_or(0.0, |pad| pad.width))
            };
            if base > 0.0 {
                let mut pad = Pad::circle(LayerId(layer), base);
                pad.backdrill_base = true;
                effective.push(pad);
            }
            let mut pad = Pad::circle(LayerId(layer), definition.display_diameter);
            pad.backdrill = true;
            effective.push(pad);
        }
    }
    effective.sort_by_key(|pad| pad.layer);
    Ok(effective)
}
