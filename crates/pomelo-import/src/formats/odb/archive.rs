use super::error;
use crate::{ImportContext, ImportError};
use std::{borrow::Cow, collections::HashMap};
pub(super) struct Archive<'a> {
    files: HashMap<String, &'a [u8]>,
    root: String,
    pub opaque: usize,
}
fn string(bytes: &[u8]) -> Result<&str, ImportError> {
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..len]).map_err(|_| error("Invalid TAR UTF-8"))
}
fn octal(bytes: &[u8]) -> Result<usize, ImportError> {
    let value = string(bytes)?.trim();
    if value.is_empty() {
        Ok(0)
    } else {
        usize::from_str_radix(value, 8).map_err(|_| error("Invalid TAR octal field"))
    }
}
fn path(name: &str) -> Result<String, ImportError> {
    let name = name.trim_start_matches("./").trim_end_matches('/');
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', ':'])
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(error("Invalid archive path"));
    }
    Ok(name.to_ascii_lowercase())
}
impl<'a> Archive<'a> {
    pub fn read(bytes: &'a [u8], context: &ImportContext<'_>) -> Result<Self, ImportError> {
        if bytes.len() > 512 * 1024 * 1024 {
            return Err(error("Archive exceeds 512 MiB"));
        }
        let mut cursor = 0usize;
        let mut count = 0;
        let mut files = HashMap::new();
        let mut long = String::new();
        let mut ended = false;
        while cursor + 512 <= bytes.len() {
            context.check_cancelled()?;
            let h = &bytes[cursor..cursor + 512];
            if h.iter().all(|&b| b == 0) {
                ended = true;
                break;
            }
            count += 1;
            if count > 100_000 {
                return Err(error("Too many archive entries"));
            }
            let checksum = h
                .iter()
                .enumerate()
                .map(|(i, &b)| {
                    if (148..156).contains(&i) {
                        32
                    } else {
                        b as usize
                    }
                })
                .sum::<usize>();
            if checksum != octal(&h[148..156])? {
                return Err(error("TAR checksum mismatch"));
            }
            let size = octal(&h[124..136])?;
            let body = cursor + 512;
            let end = body
                .checked_add(size)
                .ok_or_else(|| error("Archive entry overflow"))?;
            if size > 256 * 1024 * 1024 || end > bytes.len() {
                return Err(error("Truncated or excessive archive entry"));
            }
            let mut name = if long.is_empty() {
                string(&h[..100])?.to_owned()
            } else {
                std::mem::take(&mut long)
            };
            let prefix = string(&h[345..500])?;
            if !prefix.is_empty() && h[257..263].starts_with(b"ustar") {
                name = format!("{prefix}/{name}");
            }
            match h[156] {
                b'L' => long = string(&bytes[body..end])?.into(),
                0 | b'0' => {
                    if files.insert(path(&name)?, &bytes[body..end]).is_some() {
                        return Err(error("Duplicate archive entry"));
                    }
                }
                b'5' => {}
                _ => return Err(error("Unsupported archive entry type")),
            }
            cursor = body + size.div_ceil(512) * 512;
        }
        if !ended || !long.is_empty() {
            return Err(error("Missing TAR terminator"));
        }
        let matrices: Vec<_> = files
            .keys()
            .filter(|p| p.as_str() == "matrix/matrix" || p.ends_with("/matrix/matrix"))
            .collect();
        if matrices.len() != 1 {
            return Err(error("Need one matrix/matrix"));
        }
        let root = matrices[0][..matrices[0].len() - 13].to_owned();
        Ok(Self {
            files,
            root,
            opaque: 0,
        })
    }
    pub fn text(&mut self, path: &str, required: bool) -> Result<Cow<'a, str>, ImportError> {
        let Some(&bytes) = self
            .files
            .get(&(self.root.clone() + &path.to_ascii_lowercase()))
        else {
            if required {
                return Err(error(format!("Missing file {path}")));
            }
            return Ok(Cow::Borrowed(""));
        };
        if let Ok(text) = std::str::from_utf8(bytes) {
            return Ok(Cow::Borrowed(text));
        }
        let mut result = String::new();
        for line in bytes.split(|&b| b == b'\n') {
            match std::str::from_utf8(line) {
                Ok(text) => result.push_str(text),
                Err(_) => {
                    self.opaque += 1;
                    if line.starts_with(b"PRP ")
                        && (path.ends_with("/components") || path.ends_with("/eda/data"))
                    {
                    } else if line.starts_with(b"&") {
                        let end = line
                            .iter()
                            .position(|b| b.is_ascii_whitespace())
                            .ok_or_else(|| error("Invalid attribute dictionary"))?;
                        result.push_str(
                            std::str::from_utf8(&line[..end])
                                .map_err(|_| error("Invalid attribute key"))?,
                        );
                        result.push_str(" [opaque]");
                    } else if line.starts_with(b"CMP ") && path.ends_with("/components") {
                        let spaces: Vec<_> = line
                            .iter()
                            .enumerate()
                            .filter(|(_, b)| **b == b' ')
                            .map(|(i, _)| i)
                            .collect();
                        let end = *spaces
                            .get(6)
                            .ok_or_else(|| error("Invalid component record"))?;
                        result.push_str(
                            std::str::from_utf8(&line[..end])
                                .map_err(|_| error("Invalid component reference"))?,
                        );
                        result.push_str(" [opaque]");
                    } else {
                        return Err(error(format!("Invalid geometry/reference UTF-8 in {path}")));
                    }
                }
            }
            result.push('\n');
        }
        Ok(Cow::Owned(result))
    }
}
