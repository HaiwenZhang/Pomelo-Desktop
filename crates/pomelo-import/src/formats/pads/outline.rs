use super::binary::{MM, Reader, invalid};
use crate::{ImportContext, ImportError};
use pomelo_core::model::{Arc, LayerId, NetId, ObjectId, Point, Segment};
pub(super) fn outline(
    r: &Reader<'_>,
    context: &ImportContext<'_>,
) -> Result<Vec<Segment>, ImportError> {
    let legacy = r.version == 0x2011;
    let os = if legacy {
        96
    } else if r.version <= 0x2022 {
        100
    } else {
        112
    };
    let ps = if legacy {
        12
    } else if r.version <= 0x2024 {
        16
    } else {
        20
    };
    r.stride(10, &[os])?;
    r.stride(11, &[ps])?;
    r.stride(12, &[12])?;
    let owner = r.sections[10];
    let pieces = r.sections[11];
    let mut result = Vec::new();
    if owner.count == 0 {
        return Ok(result);
    }
    let word = |index, field| r.wrapped(owner, 68 + index * os + field).map(|v| v as i32);
    let piece = |index, field| {
        r.wrapped(pieces, pieces.bytes - (ps - 8) + index * ps + field)
            .map(|v| v as i32)
    };
    for index in 0..owner.count {
        context.check_cancelled()?;
        let name = if legacy {
            28
        } else if os == 100 {
            32
        } else {
            44
        };
        if r.u8(owner.offset + (68 + index * os + name) % owner.bytes)? == 0 {
            continue;
        }
        let next = (index + 1) % owner.count;
        let packed = word(next, 24)? as u32;
        let kind = if legacy {
            packed >> 16
        } else {
            word(next, 28)? as u32
        };
        if kind & 65535 != 1 {
            continue;
        }
        let start = word(next, 8)?;
        let count = if legacy { packed & 65535 } else { packed } as usize;
        let vertex = word(next, 12)?;
        let arc_start = word(next, 16)?;
        if start < 0 || count > pieces.count || start as usize > pieces.count - count || vertex < 0
        {
            return Err(invalid("Invalid outline piece references"));
        }
        let origin_offset = if os <= 100 { 72 } else { 88 };
        let origin = [
            word(index, origin_offset)? as f64,
            word(index, origin_offset + 4)? as f64,
        ];
        let mut cursor = vertex as usize;
        for p in start as usize..start as usize + count {
            let count = piece(p, ps - 4)?;
            let width = piece(p, ps - 8)? as f64 * MM;
            if count < 2
                || width < 0.0
                || count as usize > r.sections[12].count
                || cursor > r.sections[12].count - count as usize
            {
                return Err(invalid("Invalid outline vertices"));
            }
            for k in 0..count as usize - 1 {
                let at = r.sections[12].offset + (cursor + k) * 12;
                let a = Point::new(
                    (r.i32(at)? as f64 + origin[0]) * MM,
                    (r.i32(at + 4)? as f64 + origin[1]) * MM,
                );
                let b = Point::new(
                    (r.i32(at + 12)? as f64 + origin[0]) * MM,
                    (r.i32(at + 16)? as f64 + origin[1]) * MM,
                );
                let attribute = r.i32(at + 8)?;
                let arc = if attribute >= 0 {
                    let index = arc_start
                        .checked_add(attribute)
                        .ok_or_else(|| invalid("Outline arc index overflow"))?;
                    if arc_start < 0 || index < 0 || index as usize >= r.sections[13].count {
                        return Err(invalid("Invalid outline arc reference"));
                    }
                    r.stride(13, &[20])?;
                    let at = r.sections[13].offset + index as usize * 20;
                    let xmin = r.i32(at)? as f64;
                    let ymin = r.i32(at + 4)? as f64;
                    let xmax = r.i32(at + 8)? as f64;
                    let ymax = r.i32(at + 12)? as f64;
                    let direction = r.i32(at + 16)?;
                    let center = Point::new(
                        ((xmin + xmax) * 0.5 + origin[0]) * MM,
                        ((ymin + ymax) * 0.5 + origin[1]) * MM,
                    );
                    let radius = (xmax - xmin) * 0.5 * MM;
                    let ry = (ymax - ymin) * 0.5 * MM;
                    if radius <= 0.0
                        || ry <= 0.0
                        || (radius - ry).abs() > 0.01
                        || (a.distance(center) - radius).abs() > 0.01
                        || (b.distance(center) - radius).abs() > 0.01
                    {
                        return Err(invalid("Inconsistent outline arc radius"));
                    }
                    let start = (a.y - center.y).atan2(a.x - center.x);
                    let end = (b.y - center.y).atan2(b.x - center.x);
                    let mut sweep = end - start;
                    while sweep > std::f64::consts::PI {
                        sweep -= std::f64::consts::TAU;
                    }
                    while sweep <= -std::f64::consts::PI {
                        sweep += std::f64::consts::TAU;
                    }
                    if (sweep.abs() - std::f64::consts::PI).abs() < 1e-7 && direction != 0 {
                        sweep = (direction as f64).signum() * std::f64::consts::PI;
                    }
                    if sweep.abs() < 1e-9 {
                        return Err(invalid("Unresolved full-circle outline"));
                    }
                    Some(Arc {
                        center,
                        radius,
                        start,
                        sweep,
                    })
                } else {
                    None
                };
                if arc.is_none() && a == b {
                    continue;
                }
                result.push(Segment {
                    id: ObjectId(0x60000000 + result.len() as u32),
                    track_id: ObjectId(0x58000000 + p as u32),
                    layer: LayerId::UNASSIGNED,
                    net: NetId(0),
                    a,
                    b,
                    width,
                    arc,
                    bond_wire: None,
                });
            }
            cursor += count as usize;
        }
    }
    Ok(result)
}
