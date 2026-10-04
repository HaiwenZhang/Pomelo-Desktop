//! Boundary-only counterparts of the frozen Web layouts; no geometry interpretation here.

use super::{header::BrdHeader, index::IndexLimits, reader::Reader};
use crate::{ImportContext, ImportError};

pub(super) fn scan_record(
    reader: &mut Reader<'_>,
    kind: u8,
    header: &BrdHeader,
    limits: &IndexLimits,
    context: &ImportContext<'_>,
) -> Result<Option<u32>, ImportError> {
    let offset = reader.offset() - 1;
    let version = header.version;
    if let Some(length) = fixed_length(kind, version) {
        reader.ensure(length - 1)?;
        if kind == 0x35 {
            reader.skip(length - 1)?;
            return Ok(None);
        }
        reader.skip(3)?;
        let key = reader.u32()?;
        reader.seek(offset + length)?;
        return Ok(Some(key));
    }
    let key = match kind {
        0x03 => scan_field(reader, version, limits, offset)?,
        0x1a => {
            // V165/V166 paired nets retain the 88-byte layout, verified against real records.
            if ![152, 157, 165, 166, 172, 174, 251].contains(&version) {
                return Err(ImportError::UnsupportedRecordLayout {
                    record_type: kind,
                    version,
                    offset,
                });
            }
            reader.skip(3)?;
            let key = reader.u32()?;
            reader.skip(80 + if version >= 174 { 4 } else { 0 })?;
            key
        }
        0x1c => scan_padstack(reader, version, offset)?,
        0x1d => {
            reader.skip(3)?;
            let key = reader.u32()?;
            reader.skip(8 + if version >= 160 { 4 } else { 0 })?;
            let names = usize::from(reader.u16()?);
            let dimensions = usize::from(reader.u16()?);
            skip_entries(reader, names, 256)?;
            skip_entries(reader, dimensions, if version < 160 { 136 } else { 56 })?;
            reader.skip(if version < 160 {
                12
            } else if version >= 172 {
                4
            } else {
                0
            })?;
            key
        }
        0x1e => {
            reader.skip(3)?;
            let key = reader.u32()?;
            reader.skip(4)?;
            if !(160..162).contains(&version) {
                reader.skip(4)?;
            }
            reader.skip(4)?;
            let size = reader.u32()? as usize;
            if version >= 251 {
                reader.skip(8)?;
            }
            scan_text(reader, size, limits)?;
            if (172..251).contains(&version) {
                reader.skip(4)?;
            }
            key
        }
        0x1f => {
            reader.skip(3)?;
            let key = reader.u32()?;
            reader.skip(4 + if version < 160 { 46 } else { 14 })?;
            let count = usize::from(reader.u16()?);
            let stride = if version < 160 {
                500
            } else if version >= 175 {
                384
            } else if version >= 162 {
                280
            } else {
                240
            };
            skip_entries(reader, count, stride)?;
            reader.skip(if !(160..172).contains(&version) { 8 } else { 4 })?;
            key
        }
        0x21 => {
            reader.skip(3)?;
            let size = reader.u32()?;
            if size < 12 {
                return Err(invalid(offset, "BLOB_LENGTH", u64::from(size)));
            }
            let key = reader.u32()?;
            reader.skip(size as usize - 12)?;
            key
        }
        0x27 => {
            let Some(end) = header.constraint_end.checked_sub(1) else {
                return Err(invalid(offset, "CONSTRAINT_END", 0));
            };
            if (end as usize) < reader.offset() {
                return Err(invalid(offset, "CONSTRAINT_END", u64::from(end)));
            }
            reader.seek(end as usize)?;
            return Ok(None);
        }
        0x2a => {
            reader.skip(1)?;
            let count = usize::from(reader.u16()?);
            if version >= 174 {
                reader.skip(4)?;
            }
            if version < 165 {
                for _ in 0..count {
                    context.check_cancelled()?;
                    scan_text(reader, 36, limits)?;
                }
            } else {
                skip_entries(reader, count, 12)?;
            }
            reader.u32()?
        }
        0x31 => {
            reader.skip(3)?;
            let key = reader.u32()?;
            reader.skip(14)?;
            let size = usize::from(reader.u16()?);
            if version >= 174 {
                reader.skip(4)?;
            }
            scan_text(reader, size, limits)?;
            key
        }
        0x36 => scan_definitions(reader, version, offset)?,
        0x3b => {
            scan_property(reader, version, limits)?;
            return Ok(None);
        }
        0x3c => {
            reader.skip(3)?;
            let key = reader.u32()?;
            if version >= 174 {
                reader.skip(4)?;
            }
            let count = reader.u32()?;
            if count > 1_000_000 {
                return Err(invalid(offset, "REFERENCE_COUNT", u64::from(count)));
            }
            skip_entries(reader, count as usize, 4)?;
            key
        }
        // Older fixed records have inline text, which must retain decoding errors.
        0x07 | 0x08 | 0x0d | 0x0f | 0x10 | 0x11 if version < 160 => {
            reader.skip(3)?;
            let key = reader.u32()?;
            scan_text(reader, 32, limits)?;
            reader.skip(match kind {
                0x07 => 24,
                0x10 => 16,
                0x08 => 12,
                0x0d => 28,
                0x0f => 44,
                _ => 12,
            })?;
            key
        }
        0x38 if version < 166 => {
            reader.skip(3)?;
            let key = reader.u32()?;
            reader.skip(8)?;
            scan_text(reader, 20, limits)?;
            reader.skip(28)?;
            key
        }
        _ => {
            return Err(ImportError::UnknownRecord {
                record_type: kind,
                offset,
            });
        }
    };
    Ok(Some(key))
}

fn scan_text(
    reader: &mut Reader<'_>,
    size: usize,
    limits: &IndexLimits,
) -> Result<(), ImportError> {
    if size > limits.max_text_bytes {
        return Err(ImportError::IndexLimit {
            actual: size as u64,
            limit: limits.max_text_bytes as u64,
        });
    }
    // Decode even discarded inline strings: skipping bytes would hide encoding failures.
    reader.fixed_string(size)?;
    Ok(())
}

fn skip_entries(reader: &mut Reader<'_>, count: usize, stride: usize) -> Result<(), ImportError> {
    let bytes = count.checked_mul(stride).ok_or(ImportError::OutOfBounds {
        offset: reader.offset(),
        requested: usize::MAX,
        length: reader.length(),
    })?;
    reader.skip(bytes)
}

fn invalid(offset: usize, field: &'static str, value: u64) -> ImportError {
    ImportError::InvalidRecord {
        offset,
        field,
        value,
    }
}

fn scan_field(
    reader: &mut Reader<'_>,
    version: u16,
    limits: &IndexLimits,
    offset: usize,
) -> Result<u32, ImportError> {
    reader.skip(1)?;
    let property = reader.u16()?;
    let key = reader.u32()?;
    reader.skip(4 + if version >= 172 { 4 } else { 0 })?;
    let subtype = reader.u8()?;
    reader.skip(1)?;
    let size = usize::from(reader.u16()?);
    if version >= 172 {
        reader.skip(4)?;
    }
    if [174, 175].contains(&version) && property == 755 && subtype == 0x73 && size == 80 {
        reader.skip(80)?;
        return Ok(key);
    }
    match subtype {
        0x65 => {}
        0x64 | 0x66 | 0x67 | 0x6a => reader.skip(4)?,
        0x69 => reader.skip(8)?,
        0x68 | 0x6b | 0x6d | 0x6e | 0x6f | 0x71 | 0x73 | 0x78 => scan_text(reader, size, limits)?,
        0x6c | 0x72 => {
            let count = reader.u32()?;
            if subtype == 0x72 && count > 1_000_000 {
                return Err(invalid(offset, "FIELD_WORD_COUNT", u64::from(count)));
            }
            skip_entries(reader, count as usize, 4)?;
        }
        0x70 | 0x74 => {
            let words = usize::from(reader.u16()?);
            let bytes = usize::from(reader.u16()?);
            skip_entries(reader, words, 4)?;
            reader.skip(bytes)?;
        }
        0xf6 => reader.skip(80)?,
        _ if size == 4 || size == 8 => reader.skip(size)?,
        _ => return Err(invalid(offset, "FIELD_SUBTYPE", u64::from(subtype))),
    }
    Ok(key)
}

fn scan_padstack(reader: &mut Reader<'_>, version: u16, offset: usize) -> Result<u32, ImportError> {
    reader.skip(1)?;
    let trailing = usize::from(reader.u8()?);
    reader.skip(1)?;
    let key = reader.u32()?;
    reader.skip(8)?;
    let layers;
    if version < 172 {
        reader.skip(34)?;
        layers = usize::from(reader.u16()?);
        reader.skip(if version >= 165 { 36 } else { 32 })?;
    } else {
        reader.skip(28)?;
        layers = usize::from(reader.u16()?);
        reader.skip(if version >= 180 { 178 } else { 146 })?;
    }
    if layers > 256 {
        return Err(invalid(offset, "PADSTACK_LAYER_COUNT", layers as u64));
    }
    let fixed = if version < 165 {
        10
    } else if version < 172 {
        11
    } else {
        21
    };
    let components = fixed + layers * if version < 172 { 3 } else { 4 };
    if version < 172 {
        // The final legacy component omits its last Z2 word.
        reader.skip(components * 28 - 4)?;
    } else {
        skip_entries(reader, components, 36)?;
    }
    skip_entries(reader, trailing, if version < 172 { 32 } else { 40 })?;
    Ok(key)
}

fn scan_property(
    reader: &mut Reader<'_>,
    version: u16,
    limits: &IndexLimits,
) -> Result<(), ImportError> {
    let flags = reader.u8()?;
    let subtype = reader.u16()?;
    let size = reader.u32()? as usize;
    let name = reader.fixed_string(128)?;
    let property_type = reader.fixed_string(32)?;
    reader.skip(4)?;
    let unknown2 = reader.u32()?;
    let unknown3 = if version >= 172 { reader.u32()? } else { 0 };
    let model_name = name.starts_with("STEP3D_") || name.to_ascii_lowercase().ends_with(".sab.z");
    let attachment = if version >= 172 {
        unknown2 == 0 && unknown3 == 0x10000
    } else {
        unknown2 == 0x10000
    };
    if flags == 0 && subtype == 0 && property_type.is_empty() && model_name && attachment {
        reader.skip(size)?;
        reader.align()
    } else {
        scan_text(reader, size, limits)
    }
}

fn scan_definitions(
    reader: &mut Reader<'_>,
    version: u16,
    offset: usize,
) -> Result<u32, ImportError> {
    reader.skip(1)?;
    let code = reader.u16()?;
    let key = reader.u32()?;
    reader.skip(4 + if version >= 172 { 4 } else { 0 })?;
    let capacity = reader.u32()?;
    let count = reader.u32()?;
    reader.skip(8 + if version >= 174 { 4 } else { 0 })?;
    if capacity > 1_000_000 || count > capacity {
        return Err(invalid(offset, "DEFINITION_CAPACITY", u64::from(capacity)));
    }
    let stride = match code {
        2 => 88 + if version >= 164 { 12 } else { 0 } + if version >= 172 { 8 } else { 0 },
        3 => (if version >= 172 { 64 } else { 32 }) + if version >= 174 { 4 } else { 0 },
        4 if version < 160 => 20,
        5 => {
            if version < 160 {
                16
            } else {
                28 + if version >= 175 { 4 } else { 0 }
            }
        }
        6 => {
            if version >= 172 {
                8
            } else {
                208
            }
        }
        8 => {
            if version >= 251 {
                64
            } else {
                32 + if version >= 174 { 4 } else { 0 } + if version >= 172 { 32 } else { 0 }
            }
        }
        11 => {
            if version < 160 {
                254
            } else {
                1016
            }
        }
        12 => 232,
        13 => 200,
        15 => 20,
        16 => 108 + if version >= 180 { 4 } else { 0 },
        18 => 1052,
        _ => return Err(invalid(offset, "DEFINITION_CODE", u64::from(code))),
    };
    skip_entries(reader, capacity as usize, stride)?;
    Ok(key)
}

fn fixed_length(kind: u8, version: u16) -> Option<usize> {
    let modern = version >= 160;
    let v172 = if version >= 172 { 4 } else { 0 };
    let v174 = if version >= 174 { 4 } else { 0 };
    Some(match kind {
        0x01 => (if modern { 80 } else { 68 }) + v172,
        0x04 => 20 + v174,
        0x05 => (if modern { 60 } else { 48 }) + 2 * v172,
        0x06 => 36 + v172,
        0x07 if modern => 40 + 2 * v172,
        0x08 if modern => 24 + 2 * v172,
        0x09 => (if modern { 44 } else { 36 }) + v172 + v174,
        0x0a => (if modern { 68 } else { 64 }) + v172 + v174,
        0x0c => (if modern { 56 } else { 48 }) + 2 * v172 + v174,
        0x0d if modern => 40 + v172 + v174,
        0x0e => (if modern { 60 } else { 56 }) + 2 * v172,
        0x0f if modern => (if version >= 190 { 28 } else { 56 }) + v172 + v174,
        0x10 if modern => 32 + v172 + v174,
        0x11 if modern => 24 + v174,
        0x12 => 24 + if version >= 165 { 4 } else { 0 } + v174,
        0x14 => (if modern { 32 } else { 28 }) + v172,
        0x15..=0x17 => 40 + v172,
        0x1b => (if modern { 56 } else { 52 }) + v172,
        0x20 => {
            if version >= 174 {
                80
            } else {
                40
            }
        }
        0x22 => 40 + v172,
        0x23 => (if modern { 68 } else { 64 }) + if version >= 164 { 16 } else { 0 } + v174,
        0x24 => 52 + v172,
        0x26 => 20 + v172 + v174,
        0x28 => (if modern { 68 } else { 64 }) + 2 * v172,
        0x29 => 56,
        0x2b => 68 + if version >= 164 { 4 } else { 0 } + v172,
        0x2c => (if modern { 36 } else { 28 }) + 2 * v172,
        0x2d => (if modern { 64 } else { 60 }) + 2 * v172,
        0x2e => 36 + v172,
        0x2f => 32,
        0x30 => (if modern { 44 } else { 40 }) + 3 * v172 + v174,
        0x32 => (if modern { 76 } else { 72 }) + 2 * v172,
        0x33 => (if modern { 72 } else { 68 }) + 2 * v172,
        0x34 => (if modern { 32 } else { 28 }) + v172,
        0x35 => 124,
        0x37 => 428 + v174,
        0x38 if version >= 166 => 52 + v174,
        0x39 => 60,
        0x3a => 16 + v174,
        0x3e => 44,
        _ => return None,
    })
}
