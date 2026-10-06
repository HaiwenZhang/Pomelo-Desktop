use super::{
    binary::{Reader, invalid},
    connectivity::Junctions,
    definitions::{Footprint, Metadata, PadRow, Placement, Stack},
};
use crate::{ImportContext, ImportError};
use pomelo_core::model::{
    ComponentPlacement, CustomPadGeometry, DrillShape, LayerId, NetId, ObjectId, Pad, PadKind, Pin,
    Point, Via,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

fn place(point: Point, part: &Placement) -> Point {
    let point = point.rotate(part.angle);
    Point::new(
        part.at.x + if part.bottom { -point.x } else { point.x },
        part.at.y + point.y,
    )
}
fn geometry(
    stack: &Stack,
    row: Option<&PadRow>,
    layer: u32,
    part_angle: f64,
    bottom: bool,
    owner_angle: f64,
) -> Result<Option<Pad>, ImportError> {
    let (code, a, second) = row.map_or((stack.code, stack.width, stack.finger), |r| {
        (r.code, r.width, r.second)
    });
    if code > 4 || a < 0.0 || second < 0.0 {
        return Err(invalid(format!("Unsupported pad shape {code}")));
    }
    if a == 0.0 {
        return Ok(None);
    }
    let finger = code <= 1;
    let b = if finger && second > 0.0 { second } else { a };
    let rotation = if finger { stack.angle } else { 0.0 };
    let sign = if bottom { -1.0 } else { 1.0 };
    let mut offset =
        Point::new(if finger { stack.offset } else { 0.0 }, 0.0).rotate(part_angle + rotation);
    offset.x *= sign;
    let mut pad = Pad {
        layer: LayerId(layer),
        width: b,
        height: a,
        offset,
        kind: PadKind(if code == 4 {
            25
        } else if code == 2 {
            2
        } else if code == 0 {
            11
        } else {
            5
        }),
        corner: 0.0,
        inner_diameter: None,
        custom: None,
        backdrill: false,
        backdrill_base: false,
    };
    if code == 4 {
        if second <= 0.0 || second >= a {
            return Err(invalid("Invalid annulus inner diameter"));
        }
        pad.inner_diameter = Some(second);
    }
    let relative = sign * (part_angle + rotation) - owner_angle;
    let quarter = (relative / std::f64::consts::FRAC_PI_2).round();
    if (relative - quarter * std::f64::consts::FRAC_PI_2).abs() < 1e-10 {
        if (quarter as i64).abs() % 2 == 1 {
            std::mem::swap(&mut pad.width, &mut pad.height);
        }
    } else {
        let mut contours = Vec::new();
        let radius = if code == 2 || code == 4 {
            a * 0.5
        } else if code == 0 {
            a.min(b) * 0.5
        } else {
            0.0
        };
        let mut ring = Vec::new();
        if radius == 0.0 {
            ring = vec![
                Point::new(-b * 0.5, -a * 0.5),
                Point::new(b * 0.5, -a * 0.5),
                Point::new(b * 0.5, a * 0.5),
                Point::new(-b * 0.5, a * 0.5),
            ];
        } else {
            let steps = ((std::f64::consts::FRAC_PI_2
                / (2.0 * (1.0 - 0.001 / radius).clamp(-1.0, 1.0).acos()))
            .ceil() as usize)
                .clamp(2, 4096);
            for corner in 0..4 {
                let theta = corner as f64 * std::f64::consts::FRAC_PI_2;
                let center = Point::new(
                    if corner == 0 || corner == 3 {
                        b * 0.5 - radius
                    } else {
                        -b * 0.5 + radius
                    },
                    if corner < 2 {
                        a * 0.5 - radius
                    } else {
                        -a * 0.5 + radius
                    },
                );
                for j in 0..=steps {
                    let t = theta + j as f64 / steps as f64 * std::f64::consts::FRAC_PI_2;
                    ring.push(Point::new(
                        center.x + radius * t.cos(),
                        center.y + radius * t.sin(),
                    ));
                }
            }
        }
        let transform = |p: Point| {
            let mut p = p.rotate(relative);
            if bottom {
                p.y = -p.y;
            }
            p
        };
        contours.push(ring.into_iter().map(transform).collect());
        if let Some(inner) = pad.inner_diameter {
            let count = ((std::f64::consts::TAU
                / (2.0 * (1.0 - 0.001 / (inner * 0.5)).clamp(-1.0, 1.0).acos()))
            .ceil() as usize)
                .clamp(8, 16384);
            contours.push(
                (0..count)
                    .map(|j| {
                        let t = j as f64 / count as f64 * std::f64::consts::TAU;
                        transform(Point::new(inner * 0.5 * t.cos(), inner * 0.5 * t.sin()))
                    })
                    .collect(),
            );
        }
        pad.kind = PadKind::CUSTOM;
        pad.custom = Some(Arc::new(CustomPadGeometry {
            contours,
            paths: Vec::new(),
        }));
    }
    Ok(Some(pad))
}

fn shapes(
    stack: &Stack,
    physical: &HashMap<usize, u32>,
    version: u16,
    bottom: bool,
    part_angle: f64,
    owner_angle: f64,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<Pad>, ImportError> {
    if !stack.active || physical.is_empty() {
        return Err(invalid("Inactive padstack or missing copper layers"));
    }
    let count = physical.len();
    let mut by_layer = HashMap::<usize, (u8, Vec<&PadRow>)>::new();
    let mut put = |layer: usize, row, priority: u8| {
        let entry = by_layer.entry(layer).or_insert((priority, Vec::new()));
        if entry.0 < priority {
            entry.0 = priority;
            entry.1.clear();
        }
        if entry.0 == priority {
            entry.1.push(row);
        }
    };
    for row in &stack.rows {
        if (6..=9).contains(&row.code) {
            continue;
        }
        if row.selector == 255 {
            put(count - 1, row, 2);
        } else if row.selector == 0 {
            for layer in 1..count.saturating_sub(1) {
                put(layer, row, 1);
            }
        } else if let Some(&layer) =
            physical.get(&(row.selector as usize + usize::from(version <= 0x2021)))
        {
            put(layer as usize, row, 2);
        }
    }
    let mut result = Vec::new();
    for local in 0..count {
        let layer = if bottom { count - 1 - local } else { local };
        let rows = by_layer
            .get(&local)
            .map(|e| e.1.as_slice())
            .unwrap_or_default();
        if rows.iter().skip(1).any(|row| {
            row.code != rows[0].code || row.width != rows[0].width || row.second != rows[0].second
        }) {
            diagnostics.push(format!("PADS conflicting pad definitions on layer {layer}"));
            continue;
        }
        let row = rows.first().copied();
        if row.is_none() && stack.drill == 0.0 && local != 0 {
            continue;
        }
        match geometry(stack, row, layer as u32, part_angle, bottom, owner_angle) {
            Ok(Some(p)) => result.push(p),
            Ok(None) => {}
            Err(e) => diagnostics.push(e.to_string()),
        }
    }
    Ok(result)
}

type PlacedOwners = (Vec<Pin>, Vec<Via>, Vec<ComponentPlacement>);

#[expect(
    clippy::too_many_arguments,
    reason = "Placement combines the independently validated source definition and connectivity tables"
)]
pub(super) fn build(
    r: &Reader<'_>,
    metadata: &Metadata,
    stacks: &[Stack],
    fps: &[Footprint],
    instances: &[Option<usize>],
    assignments: &HashMap<(u32, u32), u32>,
    junctions: &Junctions,
    physical: &HashMap<usize, u32>,
    context: &ImportContext<'_>,
    diagnostics: &mut Vec<String>,
) -> Result<PlacedOwners, ImportError> {
    let s = r.sections[60];
    let stride = s.declared.checked_div(s.count).unwrap_or(36);
    let type_offset = if r.version == 0x2011 { 19 } else { 27 };
    if stride < type_offset + 7 || stride * s.count != s.declared {
        return Err(invalid("Invalid pin junction stride"));
    }
    let mut pins = Vec::new();
    let mut vias = Vec::new();
    let mut components = Vec::<ComponentPlacement>::new();
    let mut component_ids = HashMap::new();
    let mut pin_keys = HashSet::new();
    let mut via_keys = HashSet::new();
    for junction in 0..s.count {
        context.check_cancelled()?;
        let at = s.offset + junction * stride;
        let t = at + type_offset;
        if r.u8(t)? == 22 && r.u8(t + 5)? & 31 == 1 {
            let placement = (r.u32(t - 3)? & 0xffffff) as usize;
            let terminal = r.u16(t + 1)? as usize;
            if !pin_keys.insert((placement, terminal)) {
                return Err(invalid("Duplicate pin junction"));
            }
            let part = metadata
                .placements
                .get(placement)
                .ok_or_else(|| invalid("Missing pin placement"))?;
            let fp = instances
                .get(placement)
                .copied()
                .flatten()
                .and_then(|i| fps.get(i))
                .ok_or_else(|| invalid("Missing placed footprint"))?;
            let terminal_info = terminal
                .checked_sub(1)
                .and_then(|i| fp.terminals.get(i))
                .ok_or_else(|| invalid("Invalid terminal ordinal"))?;
            let stack = &stacks[terminal_info.stack];
            let point = place(terminal_info.at, part);
            if point.distance(Point::new(r.length(at)?, r.length(at + 4)?)) > 1e-5 {
                return Err(invalid("Pin placement and junction disagree"));
            }
            let angle = if part.bottom { -1.0 } else { 1.0 }
                * (part.angle
                    + if stack.slot > 0.0 {
                        stack.slot_angle
                    } else {
                        0.0
                    });
            let index = if let Some(&index) = component_ids.get(&placement) {
                index
            } else {
                let index = components.len();
                component_ids.insert(placement, index);
                components.push(ComponentPlacement {
                    id: ObjectId(0x70000000 + placement as u32),
                    source_reference: Some(ObjectId(placement as u32)),
                    reference: part.reference.clone(),
                    at: part.at,
                    angle: if part.bottom { -part.angle } else { part.angle },
                    mirrored: part.bottom,
                    pins: Vec::new(),
                });
                index
            };
            let id = ObjectId(0x3c000000 + junction as u32);
            components[index].pins.push(id);
            let pads = shapes(
                stack,
                physical,
                r.version,
                part.bottom,
                part.angle,
                angle,
                diagnostics,
            )?;
            let has_hole = stack.drill > 0.0 && stack.plated.is_some();
            if stack.slot > 0.0 && stack.slot < stack.drill {
                return Err(invalid("Invalid slot dimensions"));
            }
            let drill = if stack.slot > 0.0 && !has_hole {
                0.0
            } else {
                stack.drill
            };
            pins.push(Pin {
                id,
                owner_id: components[index].id,
                net: NetId(
                    assignments
                        .get(&(placement as u32, terminal as u32))
                        .map_or(0, |n| n + 1),
                ),
                name: if terminal_info.name.is_empty() {
                    terminal.to_string()
                } else {
                    terminal_info.name.clone()
                },
                reference: part.reference.clone(),
                at: point,
                angle,
                mirrored: part.bottom,
                drill,
                drill_shape: DrillShape {
                    width: if has_hole && stack.slot > 0.0 {
                        stack.slot
                    } else {
                        drill
                    },
                    height: drill,
                    plated: stack.plated.unwrap_or(true),
                },
                pads,
                stackup_region: None,
                die: None,
            });
        } else if r.u8(t)? == 14 && r.u8(t + 4)? == 23 && r.u8(t + 5)? & 2 != 0 {
            let definition = r.u8(t - 3)? as usize;
            let fp = fps
                .get(definition)
                .ok_or_else(|| invalid("Missing via footprint"))?;
            let stack_id = fp
                .terminals
                .first()
                .ok_or_else(|| invalid("Missing via terminal"))?
                .stack;
            let stack = &stacks[stack_id];
            let net = *junctions
                .nets
                .get(&junction)
                .ok_or_else(|| invalid("Missing via network evidence"))?;
            if net != r.u16(t + 1)? as u32
                || !stack.active
                || stack.drill < 0.0
                || stack.drill > 0.0 && stack.plated.is_none()
            {
                return Err(invalid("Incomplete via definition or conflicting network"));
            }
            let (mut start, mut end) = if stack.start == 0 && stack.end == 0 {
                (
                    0,
                    physical
                        .len()
                        .checked_sub(1)
                        .ok_or_else(|| invalid("Missing copper layers"))?
                        as u32,
                )
            } else {
                let a = *physical
                    .get(&(stack.start as usize))
                    .ok_or_else(|| invalid("Invalid via span"))?;
                let b = *physical
                    .get(&(stack.end as usize))
                    .ok_or_else(|| invalid("Invalid via span"))?;
                if a == b {
                    return Err(invalid("Invalid via span"));
                }
                (a.min(b), a.max(b))
            };
            let raw = (r.i32(at)?, r.i32(at + 4)?);
            if !via_keys.insert((raw, stack_id, net, start, end)) {
                continue;
            }
            let angle = if stack.slot > 0.0 {
                stack.slot_angle
            } else {
                0.0
            };
            let mut pads = shapes(stack, physical, r.version, false, 0.0, angle, diagnostics)?;
            pads.retain(|p| p.layer.0 >= start && p.layer.0 <= end);
            if stack.drill == 0.0 {
                start = pads
                    .iter()
                    .map(|p| p.layer.0)
                    .min()
                    .ok_or_else(|| invalid("Copper junction has no pad"))?;
                end = pads
                    .iter()
                    .map(|p| p.layer.0)
                    .max()
                    .ok_or_else(|| invalid("Copper junction has no pad"))?;
            }
            if stack.slot > 0.0 && stack.slot < stack.drill {
                return Err(invalid("Invalid via slot dimensions"));
            }
            vias.push(Via {
                id: ObjectId(0x3c000000 + junction as u32),
                net: NetId(net + 1),
                at: Point::new(r.length(at)?, r.length(at + 4)?),
                drill: stack.drill,
                drill_shape: DrillShape {
                    width: if stack.slot > 0.0 {
                        stack.slot
                    } else {
                        stack.drill
                    },
                    height: stack.drill,
                    plated: stack.plated.unwrap_or(true),
                },
                padstack: ObjectId(stack_id as u32),
                padstack_name: fp.name.clone(),
                start_layer: Some(LayerId(start)),
                end_layer: Some(LayerId(end)),
                pads: pads.into(),
                backdrill: None,
                stackup_region: None,
                angle,
                mirrored: false,
                finger: None,
            });
        }
    }
    let expected: usize = instances
        .iter()
        .flatten()
        .map(|&i| fps[i].terminals.len())
        .sum();
    if pins.len() != expected {
        return Err(invalid(format!(
            "Incomplete pin coverage {}/{expected}",
            pins.len()
        )));
    }
    Ok((pins, vias, components))
}
