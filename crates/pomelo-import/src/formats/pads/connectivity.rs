use super::{
    binary::{Reader, invalid},
    definitions::Net,
};
use crate::{ImportContext, ImportError};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Groups {
    ids: HashMap<(u32, u32), usize>,
    keys: Vec<(u32, u32)>,
    parents: Vec<usize>,
}
impl Groups {
    fn intern(&mut self, key: (u32, u32)) -> usize {
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        let id = self.parents.len();
        self.ids.insert(key, id);
        self.keys.push(key);
        self.parents.push(id);
        id
    }
    fn root(&mut self, mut id: usize) -> usize {
        while self.parents[id] != id {
            self.parents[id] = self.parents[self.parents[id]];
            id = self.parents[id];
        }
        id
    }
    fn join(&mut self, a: usize, b: usize) {
        let a = self.root(a);
        let b = self.root(b);
        self.parents[a] = b;
    }
}
pub(super) fn pin_nets(
    r: &Reader<'_>,
    nets: &[Net],
    context: &ImportContext<'_>,
) -> Result<HashMap<(u32, u32), u32>, ImportError> {
    let s = r.sections[24];
    let legacy = r.version == 0x2011;
    let old = r.version <= 0x2022;
    let stride = if legacy { 48 } else { 68 };
    r.stride(24, &[stride])?;
    let base = if old {
        s.offset + 16
    } else {
        s.offset
            .checked_sub(36)
            .ok_or_else(|| invalid("Connection base underflow"))?
    };
    let mut groups = Groups::default();
    let mut edges = Vec::new();
    for index in 0..s.count {
        context.check_cancelled()?;
        let at = base + index * stride;
        let (a, b) = if legacy {
            if r.u16(at)? != 65534 || r.u16(at + 20)? != 0xfe00 {
                return Err(invalid("Invalid legacy connection marker"));
            }
            (
                (r.u16(at + 4)? as u32, r.u16(at + 8)? as u32),
                (r.u16(at + 6)? as u32, r.u16(at + 10)? as u32),
            )
        } else if old {
            if r.u16(at)? != 65534 || r.u32(at + 36)? & 0xffffffc0 != 0xfe000000 {
                return Err(invalid("Invalid connection marker"));
            }
            (
                (r.u32(at + 8)?, r.u32(at + 16)?),
                (r.u32(at + 12)?, r.u32(at + 20)?),
            )
        } else {
            if r.u16(at + 52)? != 65534 || r.u32(at + 88)? & 0xffffffc0 != 0xfe000000 {
                return Err(invalid("Invalid connection marker"));
            }
            (
                (r.u32(at + 60)?, r.u32(at + 68)?),
                (r.u32(at + 64)?, r.u32(at + 72)?),
            )
        };
        let a = groups.intern(a);
        let b = groups.intern(b);
        groups.join(a, b);
        edges.push(a);
    }
    let mut edge_counts = HashMap::<usize, usize>::new();
    for &id in &edges {
        *edge_counts.entry(groups.root(id)).or_default() += 1;
    }
    let mut owners = HashMap::<usize, u32>::new();
    for (ordinal, net) in nets.iter().enumerate() {
        context.check_cancelled()?;
        if net.name.is_empty() || net.name == "___Unassigned_Obstacles_" {
            continue;
        }
        let id = if old {
            let at = r.sections[23].offset
                + if legacy { 12 } else { 20 }
                + ordinal * if legacy { 124 } else { 144 };
            let count = r.u32(at + 92)? as usize;
            if count == 0 {
                continue;
            }
            let edge = if legacy {
                r.u16(at + 10)? as usize
            } else {
                r.u32(at + 8)? as usize
            };
            let key = if legacy {
                (r.u16(at + 6)? as u32, r.u16(at + 8)? as u32)
            } else {
                (r.u32(at)?, r.u32(at + 4)?)
            };
            let id = *groups
                .ids
                .get(&key)
                .ok_or_else(|| invalid("Missing net anchor"))?;
            let anchor = *edges
                .get(edge)
                .ok_or_else(|| invalid("Net edge out of bounds"))?;
            let root = groups.root(id);
            if root != groups.root(anchor) || edge_counts.get(&root) != Some(&count) {
                return Err(invalid("Invalid net connectivity anchor"));
            }
            id
        } else {
            if net.anchors[1] == 0 {
                continue;
            }
            groups.intern((net.anchors[0], net.anchors[1]))
        };
        let root = groups.root(id);
        if let Some(&other) = owners.get(&root) {
            let previous = &nets[other as usize].name;
            if previous != &net.name && !previous.starts_with("$$$") && !net.name.starts_with("$$$")
            {
                return Err(invalid("Conflicting net ownership"));
            }
            if previous.starts_with("$$$") && !net.name.starts_with("$$$") {
                owners.insert(root, ordinal as u32);
            }
        } else {
            owners.insert(root, ordinal as u32);
        }
    }
    let mut result = HashMap::new();
    for id in 0..groups.keys.len() {
        if let Some(&net) = owners.get(&groups.root(id)) {
            result.insert(groups.keys[id], net);
        }
    }
    Ok(result)
}

pub(super) struct Junctions {
    pub nets: HashMap<usize, u32>,
    pub handles: HashMap<u32, u32>,
}
pub(super) fn junctions(
    r: &Reader<'_>,
    nets: &[Net],
    context: &ImportContext<'_>,
) -> Result<Junctions, ImportError> {
    let s = r.sections[49];
    let end = s.offset + s.bytes;
    let mut cursor = s.offset;
    let mut sources = Vec::new();
    let mut source_net = 0;
    let read = |cursor: &mut usize| -> Result<u32, ImportError> {
        if *cursor + 4 > end {
            return Err(invalid("Truncated junction relation"));
        }
        let value = r.u32(*cursor)?;
        *cursor += 4;
        Ok(value)
    };
    while cursor < end {
        context.check_cancelled()?;
        if source_net >= nets.len() {
            return Err(invalid("Excess junction networks"));
        }
        for direction in 0..2 {
            let count = read(&mut cursor)? as usize;
            if count > (end - cursor) / 8 {
                return Err(invalid("Junction relation count out of bounds"));
            }
            for _ in 0..count {
                let object = read(&mut cursor)?;
                let values = read(&mut cursor)? as usize;
                let tag = if direction == 0 {
                    0x3c000000
                } else {
                    0x18000000
                };
                let index = (object & 0xffffff) as usize;
                if object & 0xff000000 != tag
                    || index >= r.sections[if direction == 0 { 60 } else { 24 }].count
                    || values > (end - cursor) / 4
                {
                    return Err(invalid("Invalid junction relation reference"));
                }
                for _ in 0..values {
                    let member = read(&mut cursor)?;
                    if member & 0xff000000
                        != if direction == 0 {
                            0x18000000
                        } else {
                            0x3c000000
                        }
                        || (member & 0xffffff) as usize
                            >= r.sections[if direction == 0 { 24 } else { 60 }].count
                    {
                        return Err(invalid("Invalid junction relation member"));
                    }
                }
                if direction == 0 {
                    sources.push((index, source_net));
                }
            }
        }
        source_net += 1;
    }
    let mut seen = HashSet::new();
    let named: Vec<_> = nets
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            !n.name.is_empty()
                && n.name != "___Unassigned_Obstacles_"
                && seen.insert(n.name.as_str())
        })
        .map(|(i, _)| i)
        .collect();
    if source_net != nets.len() && source_net != named.len() {
        return Err(invalid("Junction network table mismatch"));
    }
    let mut result = Junctions {
        nets: HashMap::new(),
        handles: HashMap::new(),
    };
    for (junction, source) in sources {
        let net = if source_net == nets.len() {
            source
        } else {
            named[source]
        } as u32;
        if result
            .nets
            .insert(junction, net)
            .is_some_and(|previous| previous != net)
        {
            return Err(invalid("Conflicting junction networks"));
        }
    }
    let js = r.sections[60];
    let stride = js.declared.checked_div(js.count).unwrap_or(64);
    if stride < 31 || js.count * stride != js.declared {
        return Err(invalid("Invalid junction stride"));
    }
    for (&junction, &net) in &result.nets {
        let handle = r.u32(js.offset + junction * stride + 8)?;
        if handle != 0
            && result
                .handles
                .insert(handle, net)
                .is_some_and(|previous| previous != net)
        {
            return Err(invalid("Conflicting handle networks"));
        }
    }
    Ok(result)
}
