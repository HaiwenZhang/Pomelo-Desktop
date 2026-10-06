use super::{
    binary::{MM, Reader, invalid},
    connectivity::Junctions,
    definitions::SourceLayer,
};
use crate::{ImportContext, ImportError};
use pomelo_core::model::{LayerId, NetId, ObjectId, Point, Segment};
use std::collections::{HashMap, HashSet};

pub(super) fn routes(
    r: &Reader<'_>,
    layers: &[SourceLayer],
    physical: &HashMap<usize, u32>,
    junctions: &Junctions,
    context: &ImportContext<'_>,
) -> Result<Vec<Segment>, ImportError> {
    let objects = r.sections[62];
    let ls = r.sections[63];
    let cells = r.sections[64];
    let nodes = r.sections[61];
    let directory = r.sections[26];
    r.stride(63, &[2])?;
    r.stride(64, &[12])?;
    r.stride(61, &[12])?;
    if objects.count == 0 {
        if cells.count != 0 || objects.declared != 0 {
            return Err(invalid("Empty route table mismatch"));
        }
        return Ok(Vec::new());
    }
    let stride = r.stride(62, &[36, 48])?;
    let permutation: Vec<_> = (0..ls.count)
        .map(|i| r.u16(ls.offset + i * 2).map(usize::from))
        .collect::<Result<_, _>>()?;
    if permutation.iter().copied().collect::<HashSet<_>>().len() != ls.count
        || permutation.iter().any(|&i| i >= ls.count)
    {
        return Err(invalid("Invalid route layer permutation"));
    }
    let control = r.sections[25].offset + 180;
    let first = (r.u16(control)? as usize)
        + (r.u16(control + 2)? as usize)
        + (r.u16(control + 4)? as usize);
    let pages = r.u16(control + 6)? as usize;
    if pages == 0 || first + pages > directory.count {
        return Err(invalid("Invalid route node pages"));
    }
    let mut handles = Vec::new();
    for page in 0..pages {
        let at = directory.offset + (first + page) * 12;
        let base = r.u32(at)?;
        let start = r.u32(at + 8)? as usize;
        let next = if page + 1 < pages {
            r.u32(at + 20)? as usize
        } else {
            nodes.count
        };
        if start.min(nodes.count) != handles.len() || page + 1 < pages && next < start {
            return Err(invalid("Invalid route node page ordinal"));
        }
        for j in 0..next.min(nodes.count).saturating_sub(start) {
            handles.push(
                base.checked_add(
                    (j as u32)
                        .checked_mul(56)
                        .ok_or_else(|| invalid("Handle overflow"))?,
                )
                .ok_or_else(|| invalid("Handle overflow"))?,
            );
        }
    }
    let ordinals: HashMap<_, _> = handles.iter().enumerate().map(|(i, &h)| (h, i)).collect();
    if handles.len() != nodes.count || ordinals.len() != handles.len() {
        return Err(invalid("Incomplete route node coverage"));
    }
    let mut reverse = vec![Vec::new(); handles.len()];
    for index in 0..handles.len() {
        for field in [0, 4] {
            if let Some(&target) = ordinals.get(&r.u32(nodes.offset + index * 12 + field)?) {
                reverse[target].push(index);
            }
        }
    }
    if r.sections[27].count != ls.count {
        return Err(invalid("Route layer object count mismatch"));
    }
    let mut object_layers = Vec::new();
    let mut object_handles = Vec::new();
    let mut handle_layers = HashMap::new();
    let mut cursor = 0;
    for (layer, &source) in permutation.iter().enumerate() {
        let count = r.u32(r.sections[27].offset + layer * 4)? as usize;
        if count > r.sections[29].count.saturating_sub(cursor) {
            return Err(invalid("Route layer reference out of bounds"));
        }
        for _ in 0..count {
            let handle = r.u32(r.sections[29].offset + cursor * 4)?;
            cursor += 1;
            if handle == 0 {
                continue;
            }
            if handle_layers
                .insert(handle, source + 1)
                .is_some_and(|p| p != source + 1)
            {
                return Err(invalid("Route node layer conflict"));
            }
            let &ordinal = ordinals
                .get(&handle)
                .ok_or_else(|| invalid("Missing route node handle"))?;
            if r.u32(nodes.offset + ordinal * 12 + 8)? & 0x800000 != 0 {
                object_layers.push(source + 1);
                object_handles.push(handle);
            }
        }
    }
    if cursor != r.sections[29].count || object_layers.len() != objects.count {
        return Err(invalid("Incomplete route object association"));
    }
    let mut result = Vec::new();
    let mut stamps = vec![usize::MAX; handles.len()];
    let mut cell_cursor = 0;
    for sequence in 0..objects.count {
        context.check_cancelled()?;
        let object = (sequence + objects.count - 1) % objects.count;
        let at = 32 + object * stride;
        let old = stride == 36;
        let width = (r.wrapped(objects, at + if old { 8 } else { 20 })? as i32 as f64) * 4.0;
        let style = r.wrapped(objects, at + if old { 20 } else { 32 })?;
        let count = r.wrapped(objects, at + if old { 24 } else { 36 })? as usize;
        if count > cells.count.saturating_sub(cell_cursor) {
            return Err(invalid("Route coordinate count out of bounds"));
        }
        let begin = cell_cursor;
        cell_cursor += count;
        if width <= 0.0 || style & 0x1100 != 0 {
            continue;
        }
        let source_layer = object_layers[sequence];
        let info = layers
            .get(source_layer)
            .ok_or_else(|| invalid("Missing route layer"))?;
        if !(0..=4).contains(&info.direction) {
            return Err(invalid("Invalid route layer direction"));
        }
        let layer = *physical
            .get(&source_layer)
            .ok_or_else(|| invalid("Non-copper route layer"))?;
        let root = ordinals[&object_handles[sequence]];
        let mut frontier = vec![root];
        stamps[root] = sequence;
        let mut found = HashSet::new();
        while !frontier.is_empty() && found.is_empty() {
            context.check_cancelled()?;
            let mut next = Vec::new();
            for ordinal in frontier {
                if let Some(&net) = junctions.handles.get(&handles[ordinal]) {
                    found.insert(net);
                }
                for &linked in &reverse[ordinal] {
                    if stamps[linked] != sequence {
                        stamps[linked] = sequence;
                        next.push(linked);
                    }
                }
            }
            frontier = next;
        }
        let mut points = Vec::new();
        for j in 0..count {
            if j % 512 == 0 {
                context.check_cancelled()?;
            }
            let pos = cells.offset + (begin + j) * 12;
            let a = r.i32(pos)?;
            let b = r.i32(pos + 4)?;
            let c = r.i32(pos + 8)?;
            let pair = if info.direction == 1 {
                [(a, b), (c, b)]
            } else {
                [(b, a), (b, c)]
            };
            for (x, y) in pair {
                let point = Point::new(x as f64 * MM, y as f64 * MM);
                if points.last() != Some(&point) {
                    points.push(point);
                }
            }
        }
        if points.len() < 2 {
            continue;
        }
        if found.len() != 1 {
            return Err(invalid(format!(
                "Nonunique route network evidence for object {object}"
            )));
        }
        let net = found
            .into_iter()
            .next()
            .ok_or_else(|| invalid("Missing route network"))?;
        for pair in points.windows(2) {
            result.push(Segment {
                id: ObjectId(0x40000000 + result.len() as u32),
                track_id: ObjectId(0x3e000000 + object as u32),
                layer: LayerId(layer),
                net: NetId(net + 1),
                a: pair[0],
                b: pair[1],
                width: width * MM,
                arc: None,
                bond_wire: None,
            });
        }
    }
    if cell_cursor != cells.count {
        return Err(invalid("Unconsumed route coordinates"));
    }
    Ok(result)
}
