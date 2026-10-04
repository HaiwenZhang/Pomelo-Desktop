//! Header offsets follow the Web parser's binary layout families.

use serde::{Deserialize, Serialize};

use super::reader::Reader;
use crate::{ImportError, TextEncoding};

/// Enough bytes for all supported header layouts, including the V15 layer map.
pub const HEADER_BYTES: usize = 0x538;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordList {
    pub head: u32,
    pub tail: u32,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerMapEntry {
    pub class_id: u32,
    pub record_id: u32,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrdHeader {
    pub magic: u32,
    pub version: u16,
    pub writer_version: String,
    pub object_count: u32,
    pub units: u16,
    pub divisor: u32,
    pub string_count: u32,
    pub constraint_end: u32,
    pub layer_map: Vec<LayerMapEntry>,
    pub text_list: RecordList,
    pub graphic_list: RecordList,
    pub sentinel_keys: Vec<u32>,
}

pub fn resolve_version(magic: u32) -> Result<u16, ImportError> {
    match magic & 0xffff_ff00 {
        0x120500 => Ok(152),
        0x120f00 => Ok(157),
        0x130000 => Ok(160),
        0x130400 => Ok(162),
        0x130c00 => Ok(164),
        0x131000 => Ok(165),
        0x131500 => Ok(166),
        0x140400 | 0x140500 | 0x140600 | 0x140700 => Ok(172),
        0x140900 | 0x140e00 => Ok(174),
        0x141400 | 0x141500 => Ok(175),
        0x150000 => Ok(180),
        0x150200 => Ok(181),
        0x160100 => Ok(251),
        _ => Err(ImportError::UnsupportedMagic(magic)),
    }
}

impl BrdHeader {
    pub fn read(bytes: &[u8], encoding: TextEncoding) -> Result<Self, ImportError> {
        let mut reader = Reader::new(bytes, encoding);
        Self::read_with_reader(&mut reader)
    }

    pub(super) fn read_with_reader(reader: &mut Reader<'_>) -> Result<Self, ImportError> {
        reader.seek(0)?;
        let magic = reader.u32()?;
        let version = resolve_version(magic)?;
        let (
            text_offset,
            graphic_offset,
            writer_offset,
            units_offset,
            constraint_offset,
            string_offset,
            divisor_offset,
            head_first,
        ) = match version {
            251 => (0xb0, 0x80, 0x144, 0x1ac, 0x28, 0x34, 0x28c, false),
            181 => (0xb4, 0x84, 0x144, 0x1ac, 0x28, 0x34, 0x28c, true),
            180 => (0xb4, 0x84, 0x124, 0x18c, 0x28, 0x34, 0x26c, true),
            _ => (0x8c, 0x5c, 0xf8, 0x180, 0x18c, 0x194, 0x26c, false),
        };
        let object_count = read_u32_at(reader, 0x14)?;
        let mut sentinel_keys = Vec::new();
        if version >= 180 {
            reader.seek(if version >= 251 { 0x60 } else { 0x3c })?;
            for _ in 0..28 {
                let first = reader.u32()?;
                let second = reader.u32()?;
                let tail = if version >= 251 { first } else { second };
                if tail != 0 && !sentinel_keys.contains(&tail) {
                    sentinel_keys.push(tail);
                }
            }
        }
        let text_list = read_list(reader, text_offset, head_first)?;
        let graphic_list = read_list(reader, graphic_offset, head_first)?;
        reader.seek(writer_offset)?;
        let writer_version = reader.fixed_string(60)?;
        reader.seek(units_offset)?;
        let units = reader.u8()?;
        let constraint_end = read_u32_at(reader, constraint_offset)?;
        let string_count = read_u32_at(reader, string_offset)?;
        let divisor = read_u32_at(reader, divisor_offset)?;
        if divisor == 0 {
            return Err(ImportError::InvalidDivisor);
        }
        reader.seek(if version < 160 { 0x470 } else { 0x428 })?;
        let mut layer_map = Vec::with_capacity(25);
        for _ in 0..25 {
            layer_map.push(LayerMapEntry {
                class_id: reader.u32()?,
                record_id: reader.u32()?,
            });
        }
        Ok(Self {
            magic,
            version,
            writer_version,
            object_count,
            units,
            divisor,
            string_count,
            constraint_end,
            layer_map,
            text_list,
            graphic_list,
            sentinel_keys,
        })
    }
}

fn read_u32_at(reader: &mut Reader<'_>, offset: usize) -> Result<u32, ImportError> {
    reader.seek(offset)?;
    reader.u32()
}

fn read_list(
    reader: &mut Reader<'_>,
    offset: usize,
    head_first: bool,
) -> Result<RecordList, ImportError> {
    reader.seek(offset)?;
    let first = reader.u32()?;
    let second = reader.u32()?;
    Ok(if head_first {
        RecordList {
            head: first,
            tail: second,
        }
    } else {
        RecordList {
            head: second,
            tail: first,
        }
    })
}
