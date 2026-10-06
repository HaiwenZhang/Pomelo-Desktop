use super::binary::{Reader, invalid};
use crate::{ImportContext, ImportError};
use pomelo_core::model::Point;

pub(super) struct SourceLayer {
    pub id: usize,
    pub name: String,
    pub kind: i32,
    pub direction: i32,
}
pub(super) struct Net {
    pub name: String,
    pub anchors: [u32; 2],
}
pub(super) struct Placement {
    pub reference: String,
    pub at: Point,
    pub angle: f64,
    pub bottom: bool,
    pub decal: Option<usize>,
    pub part: Option<usize>,
    pub alternate: usize,
}
pub(super) struct Metadata {
    pub layers: Vec<SourceLayer>,
    pub nets: Vec<Net>,
    pub placements: Vec<Placement>,
}
pub(super) struct PadRow {
    pub selector: u8,
    pub code: u8,
    pub width: f64,
    pub second: f64,
}
pub(super) struct Stack {
    pub active: bool,
    pub code: u8,
    pub width: f64,
    pub drill: f64,
    pub finger: f64,
    pub offset: f64,
    pub angle: f64,
    pub start: u8,
    pub end: u8,
    pub slot: f64,
    pub slot_angle: f64,
    pub plated: Option<bool>,
    pub rows: Vec<PadRow>,
}
pub(super) struct Terminal {
    pub name: String,
    pub at: Point,
    pub stack: usize,
}
pub(super) struct Footprint {
    pub active: bool,
    pub name: String,
    pub terminals: Vec<Terminal>,
}

pub(super) fn metadata(
    r: &Reader<'_>,
    context: &ImportContext<'_>,
) -> Result<Metadata, ImportError> {
    let legacy = r.version == 0x2011;
    let old = r.version <= 0x2022;
    let ls = r.sections[69];
    let stride = r.stride(69, if legacy { &[72] } else { &[128, 136, 152] })?;
    let mut layers = Vec::new();
    for id in 0..ls.count {
        let at = ls.offset + if legacy { 8 } else { 12 } + id * stride;
        let kind = if id == 0 { 0 } else { r.i32(at - 4)? };
        if !(0..=6).contains(&kind) {
            return Err(invalid("Invalid layer type"));
        }
        layers.push(SourceLayer {
            id,
            name: r.name(at, 24)?,
            kind,
            direction: r.i32(at + 32)?,
        });
    }
    let ns = r.sections[23];
    let stride = if legacy {
        124
    } else if old {
        144
    } else if r.version == 0x2024 {
        416
    } else {
        424
    };
    r.stride(23, &[stride])?;
    let mut nets = Vec::new();
    for id in 0..ns.count {
        context.check_cancelled()?;
        let at = ns.offset + id * stride;
        let at = if legacy {
            at + 12
        } else if old {
            at + 20
        } else {
            at.checked_sub(44)
                .ok_or_else(|| invalid("Net base underflow"))?
        };
        nets.push(Net {
            name: r.name(at + if old { 12 } else { 76 }, 48)?,
            anchors: if old {
                [
                    if legacy {
                        r.u16(at + 10)? as u32
                    } else {
                        r.u32(at + 8)?
                    },
                    r.u32(at + 92)?,
                ]
            } else {
                [r.u32(at + 64)?, r.u32(at + 68)?]
            },
        });
    }
    let ps = r.sections[22];
    let stride = r.stride(22, if legacy { &[84] } else { &[96, 112] })?;
    let mut placements = Vec::new();
    for id in 0..ps.count {
        context.check_cancelled()?;
        let at = ps.offset + id * stride;
        let (decal, part, alternate) = if legacy {
            (Some(r.u16(at + 70)? as usize), None, 0)
        } else {
            let next = at + stride - 44;
            (
                Some(r.u32(next + if stride == 96 { 24 } else { 8 })? as usize),
                if stride == 112 {
                    Some(r.u32(next + 4)? as usize)
                } else {
                    None
                },
                if stride == 112 {
                    r.u8(next + 17)? as usize
                } else {
                    0
                },
            )
        };
        placements.push(Placement {
            reference: r.name(at, 16)?,
            at: Point::new(r.length(at + 16)?, r.length(at + 20)?),
            angle: r.angle(at + 24)?,
            bottom: r.u8(at + 28)? & 1 != 0,
            decal,
            part,
            alternate,
        });
    }
    Ok(Metadata {
        layers,
        nets,
        placements,
    })
}

pub(super) fn stacks(
    r: &Reader<'_>,
    context: &ImportContext<'_>,
) -> Result<Vec<Stack>, ImportError> {
    let legacy = r.version == 0x2011;
    let through = r.version <= 0x2021;
    let v22 = r.version == 0x2022;
    let (stride, rotation, o) = if legacy {
        (40, 0, [0, 4, 8, 12, 16, 20, 22, 23, 24])
    } else if through {
        (52, 24, [24, 28, 32, 36, 40, 44, 48, 49, 50])
    } else if v22 {
        (56, 20, [20, 24, 28, 32, 40, 44, 48, 49, 50])
    } else {
        (64, 28, [28, 32, 36, 44, 48, 52, 56, 57, 58])
    };
    r.stride(4, &[stride])?;
    let s = r.sections[4];
    let base = s
        .offset
        .checked_sub(rotation)
        .ok_or_else(|| invalid("Padstack base underflow"))?;
    let layer_stride = if through { 20 } else { 24 };
    r.stride(5, &[layer_stride])?;
    let layer_base = if legacy {
        base + s.count * stride - 4
    } else {
        base + s.count * stride
            + if through {
                20
            } else if v22 {
                64
            } else {
                24
            }
    };
    let total = r.sections[5].count;
    let mut result = Vec::new();
    for index in 0..s.count {
        context.check_cancelled()?;
        let at = base + index * stride;
        let mut stack = Stack {
            active: r.u8(at + o[6])? == 254,
            code: r.u8(at + o[7])?,
            width: r.length(at + o[0])?,
            drill: r.length(at + o[1])?,
            finger: r.length(at + o[2])?,
            offset: r.length(at + o[3])?,
            angle: r.angle(at + o[4])?,
            start: if through {
                0
            } else {
                r.u8(at + if v22 { 51 } else { 59 })?
            },
            end: if through {
                0
            } else {
                r.u8(at + if v22 { 52 } else { 60 })?
            },
            slot: 0.0,
            slot_angle: 0.0,
            plated: None,
            rows: Vec::new(),
        };
        if stack.active {
            if (r.version >= 0x2024 || legacy || r.version == 0x2020 || r.version == 0x2021)
                && stack.drill > 0.0
            {
                let hole = at
                    + if legacy {
                        28
                    } else if through {
                        stride + 4
                    } else {
                        stride
                    };
                let flags = r.u32(hole)?;
                if flags & !11 == 0 {
                    stack.plated = Some(flags & 2 == 0);
                }
                if flags & 8 != 0 {
                    if legacy {
                        return Err(invalid("Unverified legacy slot carrier"));
                    }
                    stack.slot = r.length(hole + 8)?;
                    stack.slot_angle = r.angle(hole + 12)?;
                }
            }
            let start = if legacy {
                r.u16(at + o[5])? as usize
            } else {
                r.u32(at + o[5])? as usize
            };
            let count = r.u8(at + o[8])? as usize;
            if count > 0
                && (count > total
                    || if v22 {
                        start > total
                    } else {
                        start >= total || count > total - start
                    })
            {
                return Err(invalid("Pad layer reference out of bounds"));
            }
            for j in 0..count {
                let geometry = if v22 {
                    (start + total - (2 % total) + j) % total
                } else {
                    start + j
                };
                let successor = if v22 {
                    (geometry + 1) % total
                } else {
                    geometry + 1
                };
                let implicit = [0x2011, 0x2020, 0x2021, 0x2024, 0x2026].contains(&r.version);
                let metadata = if v22 || implicit || [0x2020, 0x2024, 0x2027].contains(&r.version) {
                    successor
                } else {
                    geometry
                };
                let geo = layer_base + geometry * layer_stride;
                let meta = layer_base + metadata * layer_stride;
                r.range(meta, layer_stride)?;
                stack.rows.push(PadRow {
                    selector: if implicit && j < 2 {
                        if j == 0 { 0 } else { 255 }
                    } else {
                        r.u8(meta)?
                    },
                    code: r.u8(meta + 1)?,
                    width: r.length(geo + 4)?,
                    second: r.length(geo + 8)?,
                });
            }
        }
        result.push(stack);
    }
    Ok(result)
}

pub(super) fn footprints(
    r: &Reader<'_>,
    stacks: &[Stack],
    placements: &[Placement],
    context: &ImportContext<'_>,
) -> Result<(Vec<Footprint>, Vec<Option<usize>>), ImportError> {
    let legacy = r.version == 0x2011;
    let old = r.version <= 0x2020;
    let ds = r.sections[14];
    let stride = r.stride(14, if legacy { &[92] } else { &[100, 112] })?;
    let ts = r.sections[15];
    let tstride = if old { 20 } else { 36 };
    r.stride(15, &[tstride])?;
    let tbase = ts.offset + if !legacy && old { 16 } else { 0 };
    let pairs = ts.offset + ts.declared;
    let pstride = if legacy { 4 } else { 8 };
    let mut fps = Vec::new();
    for index in 0..ds.count {
        context.check_cancelled()?;
        let at = ds.offset + index * stride;
        let mut fp = Footprint {
            active: r.u16(at + if legacy { 60 } else { 64 })? == 65534,
            name: r.name(at, if r.version <= 0x2022 { 40 } else { 41 })?,
            terminals: Vec::new(),
        };
        if fp.active {
            let raw = if legacy {
                r.u16(at + 64)? as i32
            } else {
                r.i32(at + 68)?
            };
            let start = if !legacy && old && raw > 0 {
                raw - 1
            } else {
                raw
            };
            let count = if legacy {
                r.u16(at + 66)? as i32
            } else {
                r.i32(at + 72)?
            };
            if count < 0
                || count as usize > ts.count
                || count > 0 && (start < 0 || start as usize > ts.count - count as usize)
            {
                return Err(invalid("Footprint terminal reference out of bounds"));
            }
            let pstart = if legacy {
                r.u16(at + 42)? as i32
            } else {
                r.i32(at + 44)?
            };
            let pcount = r.i32(at + if legacy { 80 } else { 88 })?;
            if pcount < 0 || pcount > 0 && pstart < 0 {
                return Err(invalid("Invalid padstack mapping"));
            }
            r.range(
                pairs + pstart.max(0) as usize * pstride,
                pcount as usize * pstride,
            )?;
            let mut mapping = std::collections::HashMap::from([(0, 0usize)]);
            for j in 0..pcount {
                let p = pairs + (pstart + j) as usize * pstride;
                let ordinal = if legacy { r.u16(p)? as i32 } else { r.i32(p)? };
                let pad = if legacy {
                    r.u16(p + 2)? as i32
                } else {
                    r.i32(p + 4)?
                };
                if ordinal < 0
                    || ordinal > count
                    || pad < 0
                    || !stacks.get(pad as usize).is_some_and(|s| s.active)
                {
                    return Err(invalid("Invalid terminal padstack reference"));
                }
                mapping.insert(ordinal, pad as usize);
            }
            for j in 0..count {
                let pos = tbase + (start + j) as usize * tstride;
                let stack = *mapping.get(&(j + 1)).unwrap_or(&mapping[&0]);
                if !stacks.get(stack).is_some_and(|s| s.active) {
                    return Err(invalid("Missing default terminal padstack"));
                }
                fp.terminals.push(Terminal {
                    name: if old {
                        String::new()
                    } else {
                        r.name(pos + 20, 4)?
                    },
                    at: Point::new(
                        r.length(pos + if legacy || !old { 0 } else { 4 })?,
                        r.length(pos + if legacy || !old { 4 } else { 8 })?,
                    ),
                    stack,
                });
            }
        }
        fps.push(fp);
    }
    let pt = r.sections[17];
    let mut part_decals = Vec::new();
    if r.version != 0x2021 && pt.count > 0 {
        let stride = r.stride(17, if legacy { &[128] } else { &[208, 224] })?;
        for index in 0..pt.count {
            let pos = pt.offset + index * stride;
            let at = if legacy {
                pos
            } else {
                pos.checked_sub(44)
                    .ok_or_else(|| invalid("Part base underflow"))?
            };
            let mut decals = Vec::new();
            if legacy {
                for off in (48..116).step_by(4) {
                    let d = r.i16(at + off)?;
                    if d < 0 || r.i16(at + off + 2)? != d {
                        break;
                    }
                    decals.push(d as usize);
                }
            } else if stride == 208 {
                let d = r.i32(at + 112)?;
                if d >= 0 {
                    decals.push(d as usize);
                }
            } else {
                for off in (96..=stride - 8).step_by(8) {
                    let d = r.i32(at + off)?;
                    if d < 0 || r.i32(at + off + 4)? != d {
                        break;
                    }
                    decals.push(d as usize);
                }
            }
            part_decals.push(decals);
        }
    }
    let mut instances = Vec::new();
    for part in placements {
        if part.reference.is_empty() {
            instances.push(None);
            continue;
        }
        let choices = part.part.and_then(|i| part_decals.get(i));
        let decal = part
            .decal
            .or_else(|| choices.and_then(|c| c.get(part.alternate).or(c.first()).copied()));
        if !decal.and_then(|i| fps.get(i)).is_some_and(|f| f.active) {
            return Err(invalid("Unresolved placed footprint"));
        }
        instances.push(decal);
    }
    Ok((fps, instances))
}
