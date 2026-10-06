//! Checked PADS SDB framing and little-endian source access.
use crate::{ImportContext, ImportError};

pub(super) const MM: f64 = 0.0254 / 38100.0;
pub(super) fn invalid(detail: impl Into<String>) -> ImportError {
    ImportError::Format {
        format: "PADS".into(),
        details: detail.into(),
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub(super) struct Section {
    pub count: usize,
    pub declared: usize,
    pub offset: usize,
    pub bytes: usize,
    pub records: usize,
}

pub(super) struct Reader<'a> {
    pub data: &'a [u8],
    pub version: u16,
    pub sections: Vec<Section>,
}
impl<'a> Reader<'a> {
    pub fn range(&self, at: usize, len: usize) -> Result<&'a [u8], ImportError> {
        self.data
            .get(
                at..at
                    .checked_add(len)
                    .ok_or_else(|| invalid("Offset overflow"))?,
            )
            .ok_or_else(|| {
                invalid(format!(
                    "Truncated source field {at}+{len}/{}",
                    self.data.len()
                ))
            })
    }
    pub fn u8(&self, at: usize) -> Result<u8, ImportError> {
        Ok(self.range(at, 1)?[0])
    }
    pub fn u16(&self, at: usize) -> Result<u16, ImportError> {
        Ok(u16::from_le_bytes(
            self.range(at, 2)?.try_into().map_err(|_| invalid("u16"))?,
        ))
    }
    pub fn i16(&self, at: usize) -> Result<i16, ImportError> {
        Ok(self.u16(at)? as i16)
    }
    pub fn u32(&self, at: usize) -> Result<u32, ImportError> {
        Ok(u32::from_le_bytes(
            self.range(at, 4)?.try_into().map_err(|_| invalid("u32"))?,
        ))
    }
    pub fn i32(&self, at: usize) -> Result<i32, ImportError> {
        Ok(self.u32(at)? as i32)
    }
    pub fn length(&self, at: usize) -> Result<f64, ImportError> {
        Ok(self.i32(at)? as f64 * MM)
    }
    pub fn angle(&self, at: usize) -> Result<f64, ImportError> {
        Ok((self.i32(at)? as f64 / 1_800_000.0).to_radians())
    }
    pub fn name(&self, at: usize, len: usize) -> Result<String, ImportError> {
        let raw = self.range(at, len)?;
        let raw = &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())];
        Ok(String::from_utf8_lossy(raw).into_owned())
    }
    pub fn wrapped(&self, s: Section, offset: usize) -> Result<u32, ImportError> {
        if s.bytes == 0 {
            return Err(invalid("Empty circular section"));
        }
        let mut value = 0;
        for n in 0..4 {
            value |= (self.u8(s.offset + (offset + n) % s.bytes)? as u32) << (n * 8);
        }
        Ok(value)
    }
    pub fn stride(&self, tag: usize, allowed: &[usize]) -> Result<usize, ImportError> {
        let s = self.sections[tag];
        if s.count == 0 {
            if s.declared == 0 {
                return Ok(allowed[0]);
            }
            return Err(invalid("Nonempty zero-count section"));
        }
        let stride = s.declared / s.count;
        if stride * s.count != s.declared || !allowed.contains(&stride) {
            return Err(invalid(format!("Invalid section {tag} stride {stride}")));
        }
        Ok(stride)
    }
    pub fn read(data: &'a [u8], context: &ImportContext<'_>) -> Result<Self, ImportError> {
        context.check_cancelled()?;
        let mut r = Self {
            data,
            version: 0,
            sections: Vec::new(),
        };
        r.range(0, 52)?;
        if r.range(0, 2)? != [0, 255] {
            return Err(invalid("Invalid binary signature"));
        }
        r.version = r.u16(2)?;
        if ![
            0x2011, 0x2017, 0x2019, 0x2020, 0x2021, 0x2022, 0x2024, 0x2025, 0x2026, 0x2027,
        ]
        .contains(&r.version)
        {
            return Err(invalid(format!("Unverified version {:x}", r.version)));
        }
        let footer = data
            .len()
            .checked_sub(42)
            .ok_or_else(|| invalid("Missing footer"))?;
        if r.range(footer, 38)? != b"{2FE18320-6448-11d1-A412-000000000000}" {
            return Err(invalid("Invalid footer GUID"));
        }
        let items = r.u32(data.len() - 4)? as usize;
        if items > footer - 4 {
            return Err(invalid("Invalid footer pointer"));
        }
        let count = r.u32(26)? as usize;
        if !(72..=256).contains(&count) {
            return Err(invalid("Invalid section count"));
        }
        r.range(10, count * 16)?;
        for tag in 0..count {
            r.sections.push(Section {
                count: r.u32(10 + tag * 16)? as usize,
                declared: r.u32(14 + tag * 16)? as usize,
                ..Section::default()
            });
        }
        let mut cursor = 6 + count * 16;
        let physical = |r: &mut Self,
                        cursor: &mut usize,
                        tag: usize,
                        bytes: usize,
                        records: usize|
         -> Result<(), ImportError> {
            let end = cursor
                .checked_add(bytes)
                .ok_or_else(|| invalid("Section length overflow"))?;
            if end > footer {
                return Err(invalid(format!("Section {tag} exceeds data area")));
            }
            r.sections[tag].offset = *cursor;
            r.sections[tag].bytes = bytes;
            r.sections[tag].records = records;
            *cursor = end;
            Ok(())
        };
        for tag in 2..=27 {
            let s = r.sections[tag];
            physical(&mut r, &mut cursor, tag, s.declared, s.count)?;
        }
        let tags: Vec<_> = [65, 66, 45, 46, 47, 48, 41, 42, 74]
            .into_iter()
            .filter(|&t| t < count)
            .collect();
        let directory = r.sections[26];
        let pages: usize = tags.iter().map(|&t| r.sections[t].declared).sum();
        if directory.count * 12 != directory.bytes || pages > directory.count {
            return Err(invalid("Invalid page directory"));
        }
        let mut descriptor = directory.offset + (directory.count - pages) * 12;
        for &tag in &tags {
            let mut records = 0usize;
            for _ in 0..r.sections[tag].declared {
                records = records
                    .checked_add(r.u32(descriptor + 8)? as usize)
                    .ok_or_else(|| invalid("Page count overflow"))?;
                descriptor += 12;
            }
            r.sections[tag].records = records;
        }
        let v = r.version;
        let page_stride = |tag| match tag {
            41 => {
                if v == 0x2011 {
                    176
                } else if v == 0x2017 {
                    180
                } else {
                    188
                }
            }
            42 => {
                if v == 0x2011 {
                    72
                } else {
                    80
                }
            }
            45 => {
                if v == 0x2017 {
                    116
                } else {
                    124
                }
            }
            46 => {
                if v == 0x2011 {
                    28
                } else if v <= 0x2019 {
                    32
                } else {
                    40
                }
            }
            47 => 24,
            48 => {
                if v <= 0x2019 {
                    48
                } else if v <= 0x2022 {
                    856
                } else {
                    864
                }
            }
            65 => 28,
            66 => {
                if v <= 0x2022 {
                    28
                } else {
                    280
                }
            }
            74 => 276,
            _ => 0,
        };
        let s = r.sections[29];
        physical(&mut r, &mut cursor, 29, s.declared, s.count)?;
        for tag in [41, 42, 45, 46, 47, 48] {
            let s = r.sections[tag];
            physical(
                &mut r,
                &mut cursor,
                tag,
                s.records
                    .checked_mul(page_stride(tag))
                    .ok_or_else(|| invalid("Page size overflow"))?,
                s.records,
            )?;
        }
        let rules = r.sections[46];
        let mut live = 0;
        for i in 0..rules.records {
            let at = rules.offset + i * page_stride(46);
            if r.u32(at)? < 0x80000000 && r.u32(at + 4)? != 0 {
                live += 1;
            }
        }
        let s = r.sections[49];
        physical(&mut r, &mut cursor, 49, s.declared, s.count)?;
        r.range(cursor, live * 4)?;
        cursor += live * 4;
        for tag in [51, 50, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64] {
            let s = r.sections[tag];
            physical(&mut r, &mut cursor, tag, s.declared, s.count)?;
        }
        for tag in [65, 66] {
            let s = r.sections[tag];
            physical(
                &mut r,
                &mut cursor,
                tag,
                s.records
                    .checked_mul(page_stride(tag))
                    .ok_or_else(|| invalid("Page size overflow"))?,
                s.records,
            )?;
        }
        for tag in [67, 68, 69, 70, 71, 72, 73, 74] {
            if tag >= count {
                continue;
            }
            let s = r.sections[tag];
            let bytes = match tag {
                69 => s.declared + 12,
                70 => 4,
                71 => s
                    .declared
                    .checked_sub(4)
                    .ok_or_else(|| invalid("Invalid display parameters"))?,
                74 => s
                    .records
                    .checked_mul(page_stride(tag))
                    .ok_or_else(|| invalid("Page size overflow"))?,
                _ => s.declared,
            };
            physical(
                &mut r,
                &mut cursor,
                tag,
                bytes,
                if tag == 74 { s.records } else { s.count },
            )?;
        }
        if cursor + 15 > items || r.u8(cursor + 4)? != 8 || r.range(cursor + 5, 8)? != b"PowerSYS" {
            return Err(invalid("Invalid post-layer marker"));
        }
        Ok(r)
    }
}
