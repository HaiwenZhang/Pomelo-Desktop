use super::super::SourceZone;
use super::*;
pub(super) fn regions(
    compound: &Compound<'_>,
    layers: &Layers,
    board: &Properties,
    polygons: &[Properties],
    output: &mut ParsedBoard,
    context: &ImportContext<'_>,
) -> Result<HashSet<u16>, ImportError> {
    let count = compound.count("Regions6", context)?;
    let data = compound.stream("Regions6/Data", context)?;
    let mut filled = HashSet::new();
    let mut skipped = 0;
    records(&data, 11, 1, count, context, |index, parts| {
        let bytes = parts[0];
        let v = View(bytes);
        let word = v.u32(18)?;
        let len = (word & 0xffffff) as usize;
        if word >> 24 != 0 {
            return Err(error("Invalid region property flags"));
        }
        let props = properties(v.range(22, len)?)?;
        let mut at = 22 + len;
        let holes = v.u16(14)? as usize;
        let mut rings = Vec::new();
        for _ in 0..=holes {
            let count = v.u32(at)? as usize;
            at += 4;
            v.range(
                at,
                count
                    .checked_mul(16)
                    .ok_or_else(|| error("Region point overflow"))?,
            )?;
            let mut ring = Vec::new();
            for _ in 0..count {
                ring.push(Point::new(v.f64(at)? * MM, -v.f64(at + 8)? * MM));
                at += 16;
            }
            if ring.len() > 1 && ring[0].distance(ring[ring.len() - 1]) < 1e-8 {
                ring.pop();
            }
            rings.push(ring);
        }
        if at != bytes.len() {
            return Err(error("Unexpected region suffix"));
        }
        if number(&props, "KIND", 0.0)? != 0.0
            || v.u8(2)? == 2
            || props.get("LAYER").is_some_and(|l| l == "KEEPOUT")
            || props.get("ISBOARDCUTOUT").is_some_and(|v| v == "TRUE")
        {
            skipped += 1;
            return Ok(());
        }
        let raw = v.u8(0)? as u32;
        let polygon = v.u16(5)?;
        if let Some(layer) = copper(layers, raw, 0) {
            let mut raw_net = v.u16(3)?;
            if raw_net == 0xffff && polygon != 0xffff {
                let p = polygons
                    .get(polygon as usize)
                    .ok_or_else(|| error("Region polygon reference out of bounds"))?;
                let n = number(p, "NET", -1.0)?;
                if (0.0..65535.0).contains(&n) && n.fract() == 0.0 {
                    raw_net = n as u16;
                }
            }
            output.zones.push(SourceZone {
                id: ObjectId(0x78000000 + index as u32),
                layer,
                net: net(raw_net, output)?,
                paths: Vec::new(),
                rings,
            });
            if polygon != 0xffff {
                filled.insert(polygon);
            }
        } else {
            let layer = drawing(output, board, raw);
            let id = ObjectId(0x6d000000 + index as u32);
            let mut segments = Vec::new();
            for ring in rings {
                for (i, &a) in ring.iter().enumerate() {
                    let mut s = geometry::edge(a, ring[(i + 1) % ring.len()], 0.05, None, false);
                    s.id = ObjectId(0x6e000000 + segments.len() as u32);
                    segments.push(s);
                }
            }
            draw(output, id, layer, segments);
        }
        Ok(())
    })?;
    if skipped > 0 {
        output.diagnostics.push(format!(
            "Altium {skipped} cutout/keepout regions not displayed"
        ));
    }
    Ok(filled)
}
pub(super) fn routes(
    compound: &Compound<'_>,
    layers: &Layers,
    board: &Properties,
    polygons: &[Properties],
    filled: &HashSet<u16>,
    output: &mut ParsedBoard,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let mut keepouts = 0;
    let mut unfilled = 0;
    for (family, kind) in [("Tracks6", 4), ("Arcs6", 1)] {
        let count = compound.count(family, context)?;
        let data = compound.stream(&format!("{family}/Data"), context)?;
        records(&data, kind, 1, count, context, |_, parts| {
            let v = View(parts[0]);
            let polygon = v.u16(5)?;
            let mut raw_net = v.u16(3)?;
            if polygon != 65535 && polygon != 65534 {
                let Some(p) = polygons.get(polygon as usize) else {
                    return Ok(());
                };
                if filled.contains(&polygon) {
                    return Ok(());
                }
                unfilled += 1;
                if raw_net == 65535 {
                    let n = number(p, "NET", -1.0)?;
                    if (0.0..65535.0).contains(&n) && n.fract() == 0.0 {
                        raw_net = n as u16;
                    }
                }
            }
            if v.u8(2)? == 2 {
                keepouts += 1;
                return Ok(());
            }
            let width = v.length(if kind == 4 { 29 } else { 41 })?;
            if width <= 0.0 {
                return Ok(());
            }
            let mut s = if kind == 4 {
                geometry::edge(v.point(13)?, v.point(21)?, width, None, false)
            } else {
                let center = v.point(13)?;
                let radius = v.length(21)?;
                let start = v.f64(25)?;
                let end = v.f64(33)?;
                if radius <= 0.0 {
                    return Err(error("Invalid arc radius"));
                }
                let span = (end - start).rem_euclid(360.0);
                let span = if span == 0.0 && start == 0.0 && end == 360.0 {
                    360.0
                } else {
                    span
                };
                if span <= 0.0 {
                    return Ok(());
                }
                let a = Point::new(
                    center.x + radius * end.to_radians().cos(),
                    center.y - radius * end.to_radians().sin(),
                );
                let b = Point::new(
                    center.x + radius * start.to_radians().cos(),
                    center.y - radius * start.to_radians().sin(),
                );
                let mut s = geometry::edge(a, b, width, None, false);
                s.arc = Some(Arc {
                    center,
                    radius,
                    start: -end.to_radians(),
                    sweep: span.to_radians(),
                });
                s
            };
            let raw = v.u8(0)? as u32;
            let v7 = if parts[0].len() >= if kind == 4 { 45 } else { 56 } {
                v.u32(if kind == 4 { 41 } else { 52 })?
            } else {
                0
            };
            let copper = copper(layers, raw, v7);
            let layer = if let Some(l) = copper {
                l
            } else {
                drawing(output, board, raw)
            };
            s.layer = layer;
            s.net = net(raw_net, output)?;
            s.id = ObjectId(
                0x40000000
                    + output.scene.segments.len() as u32
                    + output.scene.drawings.len() as u32,
            );
            s.track_id = s.id;
            if copper.is_some() {
                output.scene.segments.push(s);
            } else {
                let id = ObjectId(0x68000000 + output.scene.drawings.len() as u32);
                draw(output, id, layer, vec![s]);
            }
            Ok(())
        })?;
    }
    let count = compound.count("Vias6", context)?;
    let data = compound.stream("Vias6/Data", context)?;
    records(&data, 3, 1, count, context, |index, parts| {
        let v = View(parts[0]);
        let a = *layers
            .v6
            .get(&(v.u8(29)? as u32))
            .ok_or_else(|| error("Missing via start layer"))?;
        let b = *layers
            .v6
            .get(&(v.u8(30)? as u32))
            .ok_or_else(|| error("Missing via end layer"))?;
        let drill = v.length(25)?;
        if drill < 0.0 {
            return Err(error("Negative via drill"));
        }
        let mode = if parts[0].len() > 74 { v.u8(74)? } else { 0 };
        let diameter = v.length(21)?;
        let mut pads = Vec::new();
        for layer in a.0.min(b.0)..=a.0.max(b.0) {
            let raw = layers.raw[layer as usize];
            let slot = if mode == 1 {
                if layer == 0 {
                    0
                } else if layer as usize == layers.raw.len() - 1 {
                    31
                } else {
                    1
                }
            } else if mode == 2 && raw <= 32 {
                if raw == 32 { 31 } else { raw as usize - 1 }
            } else {
                usize::MAX
            };
            let local = if slot < 32 && parts[0].len() >= 203 {
                let local = v.length(75 + slot * 4)?;
                if local == 0.0 { diameter } else { local }
            } else {
                diameter
            };
            if local > 0.0 {
                pads.push(Pad::circle(LayerId(layer), local));
            }
        }
        let net = net(v.u16(3)?, output)?;
        let at = v.point(13)?;
        let drill_shape = DrillShape {
            width: drill,
            height: drill,
            plated: true,
        };
        if a == b {
            if drill != 0.0 || pads.len() != 1 {
                return Err(error("Invalid single-layer via"));
            }
            output.scene.pins.push(Pin {
                id: ObjectId(0x70000000 + index as u32),
                owner_id: ObjectId(0),
                net,
                name: String::new(),
                reference: String::new(),
                at,
                angle: 0.0,
                mirrored: false,
                drill,
                drill_shape,
                pads,
                stackup_region: None,
                die: None,
            });
        } else {
            output.scene.vias.push(Via {
                id: ObjectId(0x50000000 + output.scene.vias.len() as u32),
                net,
                at,
                drill,
                drill_shape,
                padstack: ObjectId(index as u32),
                padstack_name: String::new(),
                start_layer: Some(LayerId(a.0.min(b.0))),
                end_layer: Some(LayerId(a.0.max(b.0))),
                pads: pads.into(),
                backdrill: None,
                stackup_region: None,
                angle: 0.0,
                mirrored: false,
                finger: None,
            });
        }
        Ok(())
    })?;
    if keepouts > 0 {
        output.diagnostics.push(format!(
            "Altium {keepouts} keepout primitives not displayed"
        ));
    }
    if unfilled > 0 {
        output.diagnostics.push(format!(
            "Altium {unfilled} polygon primitives retained as source strokes"
        ));
    }
    Ok(())
}
pub(super) fn pads(
    compound: &Compound<'_>,
    layers: &Layers,
    board: &Properties,
    components: &[Properties],
    output: &mut ParsedBoard,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let count = compound.count("Pads6", context)?;
    let data = compound.stream("Pads6/Data", context)?;
    let mut component_ids = HashMap::new();
    let mut unsupported = 0;
    let mut eccentric = 0;
    records(&data, 2, 6, count, context, |index, parts| {
        let name = parts[0];
        if name.is_empty() || name[0] as usize != name.len() - 1 {
            return Err(error("Invalid pad name field"));
        }
        let body = parts[4];
        let v = View(body);
        v.range(0, 110)?;
        let ex = View(parts[5]);
        if !parts[5].is_empty() && parts[5].len() < 596 {
            return Err(error("Unsupported pad extension size"));
        }
        let extended = parts[5].len() >= 596;
        let raw = v.u8(0)? as u32;
        let at = v.point(13)?;
        let angle = -v.f64(52)?.to_radians();
        let mode = v.u8(62)?;
        if mode > 2 {
            return Err(error("Unsupported padstack mode"));
        }
        let shape =
            |layer: LayerId, slot: usize, source: usize| -> Result<Option<Pad>, ImportError> {
                let width = if source < 3 {
                    v.length(21 + source * 8)?
                } else {
                    ex.length((source - 3) * 4)?
                };
                let height = if source < 3 {
                    v.length(25 + source * 8)?
                } else {
                    ex.length(116 + (source - 3) * 4)?
                };
                let code = if source < 3 {
                    v.u8(49 + source)?
                } else {
                    ex.u8(232 + source - 3)?
                };
                if width <= 0.0 || height <= 0.0 {
                    return Ok(None);
                }
                let (kind, corner) = match code {
                    1 => {
                        if extended && ex.u8(532 + slot)? == 9 {
                            (27, width.min(height) * ex.u8(564 + slot)? as f64 / 200.0)
                        } else {
                            (if width == height { 2 } else { 11 }, 0.0)
                        }
                    }
                    2 => (6, 0.0),
                    3 => (3, 0.0),
                    _ => return Ok(None),
                };
                let offset = if extended {
                    Point::new(ex.length(275 + slot * 4)?, -ex.length(403 + slot * 4)?)
                } else {
                    Point::default()
                };
                Ok(Some(Pad {
                    layer,
                    width,
                    height,
                    offset,
                    kind: PadKind(kind),
                    corner,
                    inner_diameter: None,
                    custom: None,
                    backdrill: false,
                    backdrill_base: false,
                }))
            };
        let display: Vec<_> = if raw == 74 {
            output.scene.layers.iter().map(|l| l.id).collect()
        } else {
            copper(layers, raw, 0).into_iter().collect()
        };
        if display.is_empty() {
            let layer = drawing(output, board, raw);
            if let Some(pad) = shape(layer, 0, 0)? {
                let paths = geometry::pad_paths(&pad);
                let mut edges = Vec::new();
                for path in paths {
                    for s in path {
                        let mut s = geometry::transform(&s, at, angle, false);
                        s.width = 0.05;
                        s.id = ObjectId(0x6a000000 + index as u32 * 32 + edges.len() as u32);
                        edges.push(s);
                    }
                }
                draw(output, ObjectId(0x69000000 + index as u32), layer, edges);
            }
            return Ok(());
        }
        let mut pads = Vec::new();
        for layer in display {
            let raw = layers.raw[layer.0 as usize];
            let slot = if raw == 32 {
                31
            } else if raw <= 31 {
                raw as usize - 1
            } else {
                1
            };
            let source = if mode == 0 || layer.0 == 0 {
                0
            } else if layer.0 as usize == layers.raw.len() - 1 {
                2
            } else if mode == 1 || raw >= 39 || raw == 2 || !extended {
                1
            } else {
                3 + (raw as usize - 3)
            };
            if let Some(p) = shape(layer, slot, source)? {
                if p.offset != Point::default() {
                    eccentric += 1;
                }
                pads.push(p);
            } else {
                unsupported += 1;
            }
        }
        let drill = v.length(45)?;
        let plated = v.u8(60)? != 0;
        let mut drill_shape = DrillShape {
            width: drill,
            height: drill,
            plated,
        };
        if extended && ex.u8(262)? == 2 && drill > 0.0 {
            let slot = ex.length(263)?;
            let rotation = ex.f64(267)?.rem_euclid(180.0);
            if (rotation - 90.0).abs() < 1e-6 {
                drill_shape.height = slot;
            } else {
                drill_shape.width = slot;
                if rotation.abs() > 1e-6 {
                    output
                        .diagnostics
                        .push("Altium slot local angle approximated".into());
                }
            }
        }
        let component = v.u16(7)?;
        let mut owner_id = ObjectId(0);
        let mut reference = String::new();
        if component != 0xffff {
            let props = components
                .get(component as usize)
                .ok_or_else(|| error("Pad component reference out of bounds"))?;
            reference = props
                .get("SOURCEDESIGNATOR")
                .or_else(|| props.get("DESIGNATOR"))
                .cloned()
                .unwrap_or_default();
            let i = if let Some(&i) = component_ids.get(&component) {
                i
            } else {
                let i = output.scene.components.len();
                component_ids.insert(component, i);
                let component_at = if props.contains_key("X") && props.contains_key("Y") {
                    Point::new(dimension(props, "X")?, -dimension(props, "Y")?)
                } else {
                    at
                };
                output.scene.components.push(ComponentPlacement {
                    id: ObjectId(0x7a000000 + component as u32),
                    source_reference: Some(ObjectId(component as u32)),
                    reference: reference.clone(),
                    at: component_at,
                    angle: -number(props, "ROTATION", 0.0)?.to_radians(),
                    mirrored: raw == 32,
                    pins: Vec::new(),
                });
                i
            };
            owner_id = output.scene.components[i].id;
            output.scene.components[i]
                .pins
                .push(ObjectId(0x60000000 + index as u32));
        }
        output.scene.pins.push(Pin {
            id: ObjectId(0x60000000 + index as u32),
            owner_id,
            net: net(v.u16(3)?, output)?,
            name: latin(&name[1..]),
            reference,
            at,
            angle,
            mirrored: raw == 32,
            drill,
            drill_shape,
            pads,
            stackup_region: None,
            die: None,
        });
        Ok(())
    })?;
    if unsupported > 0 {
        output.diagnostics.push(format!(
            "Altium {unsupported} unsupported or zero-size pad shapes omitted"
        ));
    }
    if eccentric > 0 {
        output.diagnostics.push(format!(
            "Altium {eccentric} eccentric copper shapes retained; drill at pad center"
        ));
    }
    Ok(())
}
pub(super) fn fills(
    compound: &Compound<'_>,
    layers: &Layers,
    board: &Properties,
    output: &mut ParsedBoard,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let count = compound.count("Fills6", context)?;
    let data = compound.stream("Fills6/Data", context)?;
    records(&data, 6, 1, count, context, |index, parts| {
        let v = View(parts[0]);
        if v.u8(2)? == 2 {
            return Ok(());
        }
        let a = v.point(13)?;
        let b = v.point(21)?;
        let center = Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
        let angle = -v.f64(29)?.to_radians();
        let vertices = [a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)].map(|p| {
            let p = Point::new(p.x - center.x, p.y - center.y).rotate(angle);
            Point::new(center.x + p.x, center.y + p.y)
        });
        let raw = v.u8(0)? as u32;
        if let Some(layer) = copper(layers, raw, 0) {
            output.zones.push(SourceZone {
                id: ObjectId(0x79000000 + index as u32),
                layer,
                net: net(v.u16(3)?, output)?,
                paths: Vec::new(),
                rings: vec![vertices.to_vec()],
            });
        } else {
            let layer = drawing(output, board, raw);
            let segments = vertices
                .iter()
                .enumerate()
                .map(|(i, &a)| geometry::edge(a, vertices[(i + 1) % 4], 0.05, None, false))
                .collect();
            draw(output, ObjectId(0x6c000000 + index as u32), layer, segments);
        }
        Ok(())
    })
}
pub(super) fn texts(
    compound: &Compound<'_>,
    layers: &Layers,
    board: &Properties,
    output: &mut ParsedBoard,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    let wide = compound.stream("WideStrings6/Data", context)?;
    let v = View(&wide);
    let mut table = HashMap::new();
    let mut at = 0;
    while at < wide.len() {
        let id = v.u32(at)?;
        let len = v.u32(at + 4)? as usize;
        at += 8;
        let text = if len > 2 {
            let raw = v.range(at, len)?;
            at += len;
            compound::utf16(&raw[..len - 2])?
        } else {
            String::new()
        };
        if table.insert(id, text).is_some() {
            return Err(error("Duplicate Unicode text ordinal"));
        }
    }
    let count = compound.count("Texts6", context)?;
    let data = compound.stream("Texts6/Data", context)?;
    let mut nonstroke = 0;
    records(&data, 5, 2, count, context, |index, parts| {
        let body = View(parts[0]);
        body.range(0, 123)?;
        let label = parts[1];
        if label.is_empty() || label[0] as usize != label.len() - 1 {
            return Err(error("Invalid text label"));
        }
        let text = table
            .get(&body.u32(115)?)
            .cloned()
            .unwrap_or_else(|| latin(&label[1..]));
        let height = body.length(21)?;
        if height < 0.0 {
            return Err(error("Negative text size"));
        }
        if text.is_empty() || height == 0.0 {
            return Ok(());
        }
        let raw = body.u8(0)? as u32;
        let layer = if let Some(l) = copper(layers, raw, 0) {
            l
        } else {
            drawing(output, board, raw)
        };
        if body.u8(43)? != 0 {
            nonstroke += 1;
        }
        let stroke = body.length(36)?;
        let component = body.u16(7)?;
        let owner_id = output
            .scene
            .components
            .iter()
            .find(|c| c.source_reference == Some(ObjectId(component as u32)))
            .map(|c| c.id);
        output.scene.texts.push(BoardText {
            id: ObjectId(0x71000000 + index as u32),
            owner_id,
            layer,
            class_id: 0,
            subclass: raw as u8,
            text,
            at: body.point(13)?,
            angle: -body.f64(27)?.to_radians(),
            mirrored: body.u8(35)? != 0,
            align: TextAlignment::Left,
            font_index: body.u16(25)?.min(255) as u8,
            width: height * 0.65,
            height,
            spacing: 0.0,
            line_spacing: height * 1.3,
            stroke_width: if stroke > 0.0 {
                stroke
            } else {
                0.05f64.min(height * 0.08)
            },
        });
        Ok(())
    })?;
    if nonstroke > 0 {
        output
            .diagnostics
            .push(format!("Altium {nonstroke} source font faces approximated"));
    }
    Ok(())
}
