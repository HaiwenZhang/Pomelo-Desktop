//! Read-only OLE Compound File reader, including trimmed final Altium sectors.
use super::{View, error};
use crate::{ImportContext, ImportError};
use std::collections::{HashMap, HashSet};
const FREE: u32 = 0xffffffff;
const END: u32 = 0xfffffffe;
const FAT: u32 = 0xfffffffd;
const DIFAT: u32 = 0xfffffffc;
struct Entry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: usize,
}
pub(super) struct Compound<'a> {
    bytes: &'a [u8],
    unit: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    entries: Vec<Entry>,
    paths: HashMap<String, usize>,
    mini: Vec<u8>,
}
fn chain(
    bytes: &[u8],
    start: u32,
    size: Option<usize>,
    table: &[u32],
    unit: usize,
    header: bool,
    context: &ImportContext<'_>,
) -> Result<Vec<u8>, ImportError> {
    if size == Some(0) {
        return Ok(Vec::new());
    }
    if size.is_some_and(|s| s > 512 * 1024 * 1024) {
        return Err(error("Stream exceeds 512 MiB"));
    }
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut sector = start;
    while sector != END {
        context.check_cancelled()?;
        if sector >= DIFAT || sector as usize >= table.len() || !seen.insert(sector) {
            return Err(error("Invalid sector chain"));
        }
        ids.push(sector as usize);
        if ids.len() * unit > 512 * 1024 * 1024 {
            return Err(error("Sector chain exceeds limit"));
        }
        sector = table[sector as usize];
        if size.is_some_and(|s| ids.len() * unit >= s) {
            if sector != END {
                return Err(error("Stream chain longer than declaration"));
            }
            break;
        }
    }
    let size = size.unwrap_or(ids.len() * unit);
    if ids.len() * unit < size {
        return Err(error("Truncated stream chain"));
    }
    let mut result = Vec::with_capacity(size);
    for id in ids {
        let at = (id + usize::from(header)) * unit;
        let len = unit.min(size - result.len());
        let data = bytes
            .get(at..at + len)
            .ok_or_else(|| error("Sector outside source"))?;
        result.extend_from_slice(data);
    }
    Ok(result)
}
impl<'a> Compound<'a> {
    pub fn read(bytes: &'a [u8], context: &ImportContext<'_>) -> Result<Self, ImportError> {
        let v = View(bytes);
        if v.range(0, 8)? != [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1] {
            return Err(error("Invalid compound signature"));
        }
        let major = v.u16(26)?;
        let shift = v.u16(30)?;
        if !((major == 3 && shift == 9) || (major == 4 && shift == 12))
            || v.u16(32)? != 6
            || v.u16(28)? != 0xfffe
            || v.u32(56)? != 4096
        {
            return Err(error("Invalid sector format"));
        }
        let unit = 1usize << shift;
        if bytes.len() < unit * 2 {
            return Err(error("Compound file too small"));
        }
        let sectors = bytes.len().div_ceil(unit) - 1;
        let fat_count = v.u32(44)? as usize;
        let difat_count = v.u32(72)? as usize;
        if fat_count == 0 || fat_count > sectors || difat_count > sectors {
            return Err(error("Invalid FAT count"));
        }
        let mut fat_ids = Vec::new();
        for i in 0..109 {
            if fat_ids.len() >= fat_count {
                break;
            }
            let id = v.u32(76 + i * 4)?;
            if id != FREE {
                fat_ids.push(id);
            }
        }
        let mut difat = v.u32(68)?;
        let mut used = HashSet::new();
        for _ in 0..difat_count {
            if difat as usize >= sectors || !used.insert(difat) {
                return Err(error("Invalid DIFAT chain"));
            }
            let at = (difat as usize + 1) * unit;
            for j in 0..unit / 4 - 1 {
                if fat_ids.len() >= fat_count {
                    break;
                }
                let id = v.u32(at + j * 4)?;
                if id != FREE {
                    fat_ids.push(id);
                }
            }
            difat = v.u32(at + unit - 4)?;
        }
        if difat != END
            || fat_ids.len() != fat_count
            || fat_ids.iter().copied().collect::<HashSet<_>>().len() != fat_count
            || fat_ids
                .iter()
                .any(|&n| n as usize >= sectors || used.contains(&n))
        {
            return Err(error("Invalid FAT directory"));
        }
        let mut fat = Vec::new();
        for &id in &fat_ids {
            for j in 0..unit / 4 {
                fat.push(v.u32((id as usize + 1) * unit + j * 4)?);
            }
        }
        for id in fat_ids {
            if fat.get(id as usize) != Some(&FAT) {
                return Err(error("Invalid FAT self marker"));
            }
        }
        for id in used {
            if fat.get(id as usize) != Some(&DIFAT) {
                return Err(error("Invalid DIFAT self marker"));
            }
        }
        let directory = chain(bytes, v.u32(48)?, None, &fat, unit, true, context)?;
        if directory.len() % 128 != 0 {
            return Err(error("Invalid directory size"));
        }
        let d = View(&directory);
        let mut entries = Vec::new();
        for at in (0..directory.len()).step_by(128) {
            let len = d.u16(at + 64)? as usize;
            let kind = d.u8(at + 66)?;
            if len > 64 || !len.is_multiple_of(2) || [1, 2, 5].contains(&kind) && len < 2 {
                return Err(error("Invalid directory name"));
            }
            let name = if kind == 0 {
                String::new()
            } else {
                utf16(d.range(at, len - 2)?)?
            };
            let low = d.u32(at + 120)? as u64;
            let high = if major == 3 {
                0
            } else {
                d.u32(at + 124)? as u64
            };
            let size =
                usize::try_from(low + (high << 32)).map_err(|_| error("Stream size overflow"))?;
            entries.push(Entry {
                name,
                kind,
                left: d.u32(at + 68)?,
                right: d.u32(at + 72)?,
                child: d.u32(at + 76)?,
                start: d.u32(at + 116)?,
                size,
            });
        }
        let root = entries
            .first()
            .filter(|r| r.kind == 5)
            .ok_or_else(|| error("Invalid root directory"))?;
        let mini_count = v.u32(64)? as usize;
        if mini_count > sectors {
            return Err(error("Invalid MiniFAT count"));
        }
        let mini_bytes = chain(
            bytes,
            v.u32(60)?,
            Some(mini_count * unit),
            &fat,
            unit,
            true,
            context,
        )?;
        let mini_fat = mini_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let mini = chain(
            bytes,
            root.start,
            Some(root.size),
            &fat,
            unit,
            true,
            context,
        )?;
        let mut pending = vec![(root.child, String::new(), 0)];
        let mut visited = HashSet::new();
        let mut paths = HashMap::new();
        while let Some((id, prefix, depth)) = pending.pop() {
            context.check_cancelled()?;
            if id == FREE {
                continue;
            }
            if depth > 256 || !visited.insert(id) {
                return Err(error("Invalid directory tree cycle/depth"));
            }
            let e = entries
                .get(id as usize)
                .ok_or_else(|| error("Directory reference out of bounds"))?;
            if e.kind != 1 && e.kind != 2 {
                return Err(error("Invalid directory entry kind"));
            }
            let path = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            if paths
                .insert(path.to_ascii_lowercase(), id as usize)
                .is_some()
            {
                return Err(error("Duplicate stream path"));
            }
            pending.push((e.right, prefix.clone(), depth + 1));
            pending.push((e.left, prefix, depth + 1));
            if e.kind == 1 {
                pending.push((e.child, path, depth + 1));
            }
        }
        Ok(Self {
            bytes,
            unit,
            fat,
            mini_fat,
            entries,
            paths,
            mini,
        })
    }
    pub fn stream(&self, path: &str, context: &ImportContext<'_>) -> Result<Vec<u8>, ImportError> {
        let &id = self
            .paths
            .get(&path.to_ascii_lowercase())
            .ok_or_else(|| error(format!("Missing stream {path}")))?;
        let e = &self.entries[id];
        if e.kind != 2 {
            return Err(error("Requested storage as stream"));
        }
        if e.size < 4096 {
            chain(
                &self.mini,
                e.start,
                Some(e.size),
                &self.mini_fat,
                64,
                false,
                context,
            )
        } else {
            chain(
                self.bytes,
                e.start,
                Some(e.size),
                &self.fat,
                self.unit,
                true,
                context,
            )
        }
    }
    pub fn count(&self, family: &str, context: &ImportContext<'_>) -> Result<usize, ImportError> {
        let path = format!("{family}/Header");
        if !self.paths.contains_key(&path.to_ascii_lowercase()) {
            return Ok(0);
        }
        let bytes = self.stream(&path, context)?;
        Ok(View(&bytes).u32(0)? as usize)
    }
}
pub(super) fn utf16(bytes: &[u8]) -> Result<String, ImportError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(error("Odd UTF-16 byte length"));
    }
    String::from_utf16(
        &bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect::<Vec<_>>(),
    )
    .map_err(|_| error("Invalid UTF-16"))
}
