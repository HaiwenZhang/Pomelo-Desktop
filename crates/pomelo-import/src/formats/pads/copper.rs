use super::super::SourceZone;
use super::saved_fill::{self, Contour, Fill, Thermal};
use super::{
    binary::{MM, Reader, invalid},
    connectivity::Junctions,
    definitions::Net,
};
use crate::{ImportContext, ImportError};
use pomelo_core::{
    geometry::{arc_sweep, flatten_path},
    model::{Arc, LayerId, NetId, ObjectId, Point, Segment},
};
use std::collections::{HashMap, HashSet};

struct Owner {
    kind: u8,
    links: [i32; 3],
    origin: [i32; 2],
    piece_start: usize,
    piece_count: usize,
    vertex_start: usize,
    arc_start: usize,
}
struct Piece {
    kind: u8,
    layer: usize,
    width: f64,
    path: Vec<Segment>,
}
fn owned(
    r: &Reader<'_>,
    owner: &Owner,
    context: &ImportContext<'_>,
) -> Result<Vec<Piece>, ImportError> {
    let s = &r.sections;
    let mut cursor = owner.vertex_start;
    let mut arc_cursor = owner.arc_start;
    let mut pieces = Vec::new();
    if owner.piece_count > s[53].count || owner.piece_start > s[53].count - owner.piece_count {
        return Err(invalid("Copper piece reference out of bounds"));
    }
    for index in owner.piece_start..owner.piece_start + owner.piece_count {
        context.check_cancelled()?;
        let at = s[53].offset + index * 16;
        let count = r.u32(at)? as usize;
        let arc_count = r.u32(at + 4)? as usize;
        if count == 0
            || count > s[54].count
            || cursor > s[54].count - count
            || arc_count > s[55].count
            || arc_cursor > s[55].count - arc_count
        {
            return Err(invalid("Copper point/arc reference out of bounds"));
        }
        let mut points = Vec::new();
        for k in 0..count {
            if k % 512 == 0 {
                context.check_cancelled()?;
            }
            let p = s[54].offset + (cursor + k) * 8;
            points.push(Point::new(
                (r.i32(p)? as f64 + owner.origin[0] as f64) * MM,
                (r.i32(p + 4)? as f64 + owner.origin[1] as f64) * MM,
            ));
        }
        let mut arcs = HashMap::new();
        for k in 0..arc_count {
            let p = s[55].offset + (arc_cursor + k) * 16;
            let vertex = r.u32(p + 8)? as usize;
            if vertex >= count - 1 || arcs.contains_key(&vertex) {
                return Err(invalid("Invalid copper arc vertex"));
            }
            arcs.insert(
                vertex,
                (
                    Point::new(
                        (r.i32(p)? as f64 + owner.origin[0] as f64) * MM,
                        (r.i32(p + 4)? as f64 + owner.origin[1] as f64) * MM,
                    ),
                    r.i16(p + 14)?,
                ),
            );
        }
        cursor += count;
        arc_cursor += arc_count;
        let kind = r.u8(at + 12)?;
        let layer = r.u8(at + 13)? as usize;
        let width = r.length(at + 8)?;
        let segment = |a, b, arc| Segment {
            id: ObjectId(index as u32),
            track_id: ObjectId(0),
            layer: LayerId::UNASSIGNED,
            net: NetId(0),
            a,
            b,
            width: if kind == 52 { width } else { 0.0 },
            arc,
            bond_wire: None,
        };
        let mut path = Vec::new();
        if kind == 51 {
            if points.len() != 2 || !arcs.is_empty() {
                return Err(invalid("Invalid circular copper endpoints"));
            }
            let a = points[0];
            let b = points[1];
            let center = Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
            let radius = a.distance(b) * 0.5;
            if radius <= 0.0 {
                return Err(invalid("Invalid copper radius"));
            }
            let start = (a.y - center.y).atan2(a.x - center.x);
            path.push(segment(
                a,
                b,
                Some(Arc {
                    center,
                    radius,
                    start,
                    sweep: std::f64::consts::PI,
                }),
            ));
            path.push(segment(
                b,
                a,
                Some(Arc {
                    center,
                    radius,
                    start: start + std::f64::consts::PI,
                    sweep: std::f64::consts::PI,
                }),
            ));
        } else {
            if kind != 50 && kind != 52
                || kind == 52 && (!arcs.is_empty() || !count.is_multiple_of(2))
            {
                return Err(invalid("Unsupported copper piece type"));
            }
            for k in (0..count - 1).step_by(if kind == 52 { 2 } else { 1 }) {
                let a = points[k];
                let b = points[k + 1];
                let arc = if let Some(&(center, tenths)) = arcs.get(&k) {
                    let radius = a.distance(center);
                    let start = (a.y - center.y).atan2(a.x - center.x);
                    let end = (b.y - center.y).atan2(b.x - center.x);
                    let tiny = (end - start).sin().atan2((end - start).cos());
                    if radius <= 0.0
                        || (tenths as i32).abs() > 3600
                        || tenths == 0 && tiny.abs() > std::f64::consts::PI / 1800.0 + 1e-9
                    {
                        return Err(invalid("Invalid copper arc parameters"));
                    }
                    Some(Arc {
                        center,
                        radius,
                        start,
                        sweep: if tenths == 0 {
                            tiny
                        } else {
                            arc_sweep(start, end, tenths < 0)
                        },
                    })
                } else {
                    None
                };
                if arc.is_some() || a != b {
                    path.push(segment(a, b, arc));
                } else if kind == 52 && !(width == 0.0 && count == 2) {
                    return Err(invalid("Unresolved thermal geometry"));
                }
            }
            if kind != 52 && points[0] != points[count - 1] {
                path.push(segment(points[count - 1], points[0], None));
            }
        }
        pieces.push(Piece {
            kind,
            layer,
            width,
            path,
        });
    }
    Ok(pieces)
}
fn contour(
    piece: &Piece,
    layer: usize,
    context: &ImportContext<'_>,
) -> Result<Contour, ImportError> {
    if piece.kind == 52 || piece.layer != layer {
        return Err(invalid("Invalid contour layer/type"));
    }
    let ring = flatten_path(&piece.path, 0.001, 8_000_000, context.cancellation).map_err(|e| {
        if context.cancellation.is_cancelled() {
            ImportError::Cancelled
        } else {
            invalid(e.to_string())
        }
    })?;
    Ok(Contour {
        width: piece.width,
        ring: ring.into_iter().map(|p| [p.x, p.y]).collect(),
    })
}
fn walk(
    owners: &[Owner],
    claimed: &mut HashSet<usize>,
    parent: usize,
    start: i32,
    field: usize,
    kinds: &[u8],
    context: &ImportContext<'_>,
) -> Result<Vec<usize>, ImportError> {
    let mut cursor = start;
    let mut result = Vec::new();
    if cursor == parent as i32 {
        return Ok(result);
    }
    while cursor >= 0 {
        context.check_cancelled()?;
        let index = cursor as usize;
        let o = owners
            .get(index)
            .ok_or_else(|| invalid("Copper owner link out of bounds"))?;
        if !kinds.contains(&o.kind) || !claimed.insert(index) {
            return Err(invalid("Invalid copper owner link/cycle"));
        }
        result.push(index);
        cursor = o.links[field];
    }
    if cursor != -(parent as i32) - 1 {
        return Err(invalid("Copper chain terminator mismatch"));
    }
    Ok(result)
}
pub(super) fn zones(
    r: &Reader<'_>,
    nets: &[Net],
    junctions: &Junctions,
    physical: &HashMap<usize, u32>,
    context: &ImportContext<'_>,
    diagnostics: &mut Vec<String>,
) -> Result<Vec<SourceZone>, ImportError> {
    for (tag, stride) in [(52, 88), (53, 16), (54, 8), (55, 16)] {
        r.stride(tag, &[stride])?;
    }
    let s = r.sections[52];
    let legacy = r.version == 0x2011;
    let mut owners = Vec::new();
    for index in 0..s.count {
        let at = s.offset + index * 88;
        owners.push(Owner {
            kind: r.u8(at + if legacy { 85 } else { 87 })?,
            links: [r.i32(at + 12)?, r.i32(at + 16)?, r.i32(at + 20)?],
            origin: [r.i32(at + 24)?, r.i32(at + 28)?],
            piece_start: r.u32(at)? as usize,
            vertex_start: r.u32(at + 4)? as usize,
            arc_start: r.u32(at + 8)? as usize,
            piece_count: if legacy {
                r.u16(at + 64)? as usize
            } else {
                r.u32(at + 64)? as usize
            },
        });
    }
    let mut claimed = HashSet::new();
    let mut result = Vec::new();
    let mut missing_witnesses = 0;
    let mut boundary_only = 0;
    for (boundary, owner) in owners.iter().enumerate() {
        context.check_cancelled()?;
        if owner.kind != 50 {
            continue;
        }
        claimed.insert(boundary);
        let net = owner.links[0];
        if net < -1 || net >= 0 && !nets.get(net as usize).is_some_and(|n| !n.name.is_empty()) {
            return Err(invalid("Invalid copper network reference"));
        }
        let fills = walk(
            &owners,
            &mut claimed,
            boundary,
            owner.links[1],
            1,
            &[51],
            context,
        )?;
        if fills.is_empty() {
            boundary_only += 1;
        }
        for fill_id in fills {
            let fill_owner = &owners[fill_id];
            let outer = owned(r, fill_owner, context)?;
            if outer.len() != 1 {
                return Err(invalid("Unverified multi-exterior fill"));
            }
            let layer = outer[0].layer;
            let mut fill = Fill {
                owner: fill_id as u32,
                outer: contour(&outer[0], layer, context)?,
                holes: Vec::new(),
                thermals: Vec::new(),
            };
            for hole in walk(
                &owners,
                &mut claimed,
                fill_id,
                fill_owner.links[0],
                0,
                &[52],
                context,
            )? {
                for p in owned(r, &owners[hole], context)? {
                    fill.holes.push(contour(&p, layer, context)?);
                }
            }
            for thermal in walk(
                &owners,
                &mut claimed,
                fill_id,
                fill_owner.links[2],
                2,
                &[53, 54],
                context,
            )? {
                let o = &owners[thermal];
                let witness = usize::try_from(o.links[0])
                    .ok()
                    .and_then(|j| junctions.nets.get(&j));
                if let Some(&w) = witness {
                    if net < 0 || w != net as u32 {
                        return Err(invalid("Copper/thermal network conflict"));
                    }
                } else {
                    missing_witnesses += 1;
                }
                for p in owned(r, o, context)? {
                    if p.kind != 52 || p.layer != layer {
                        return Err(invalid("Invalid thermal piece"));
                    }
                    for edge in p.path {
                        if edge.width > 0.0 {
                            fill.thermals.push(Thermal {
                                a: [edge.a.x, edge.a.y],
                                b: [edge.b.x, edge.b.y],
                                width: edge.width,
                            });
                        }
                    }
                }
            }
            let display = *physical
                .get(&layer)
                .ok_or_else(|| invalid("Invalid fill copper layer"))?;
            for region in saved_fill::build(&fill, context.cancellation)? {
                let rings = std::iter::once(region.outer)
                    .chain(region.holes)
                    .map(|ring| ring.into_iter().map(|p| Point::new(p[0], p[1])).collect())
                    .collect();
                result.push(SourceZone {
                    id: ObjectId(0x50000000 + result.len() as u32),
                    layer: LayerId(display),
                    net: NetId(if net < 0 { 0 } else { net as u32 + 1 }),
                    paths: Vec::new(),
                    rings,
                });
            }
        }
    }
    if claimed.len() != owners.len() {
        return Err(invalid("Unclaimed or unknown copper owners"));
    }
    if missing_witnesses > 0 {
        diagnostics.push(format!("PADS {missing_witnesses} thermal junctions lack independent network evidence; saved fill network retained"));
    }
    if boundary_only > 0 {
        diagnostics.push(format!(
            "PADS {boundary_only} design boundaries have no saved fill; no repour performed"
        ));
    }
    Ok(result)
}
