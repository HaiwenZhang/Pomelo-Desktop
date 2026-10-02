//! Variable source layouts, with per-record allocation budgets and cancellable collections.

use super::{
    super::{header::BrdHeader, index::RecordKey, reader::Reader},
    DecodeLimits,
};
use crate::{ImportContext, ImportError};
use serde::Serialize;
use std::mem::size_of;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FieldValue {
    Word(u32),
    Words(Vec<u32>),
    Text(String),
}

/// 0x03. Some payloads are opaque binary rather than locale-dependent text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Field {
    pub hdr1: u16,
    pub key: RecordKey,
    pub next: u32,
    pub sub_type: u16,
    pub size: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<FieldValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PairedNetMember {
    pub net: u32,
    pub next: u32,
    pub metadata: Vec<u32>,
}
/// 0x1a. Member words remain opaque; layout support follows the frozen Web baseline.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PairedNets {
    pub r#type: u16,
    pub t2: u16,
    pub key: RecordKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown: Option<u32>,
    pub members: [PairedNetMember; 2],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ConstraintSet {
    pub key: RecordKey,
    pub next: u32,
    pub name_str_key: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_ptr: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct SignalIntegrityModel {
    pub r#type: u16,
    pub t2: u16,
    pub key: RecordKey,
    pub next: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u16>,
    pub str_ptr: u32,
    pub size: u32,
    pub string: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown4: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown5: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PadstackDimensions {
    pub key: RecordKey,
    pub next: u32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Blob {
    pub size: u32,
    pub key: RecordKey,
}
#[derive(Debug, Serialize)]
pub struct ConstraintRegion {}

#[derive(Debug, Serialize)]
#[serde(untagged, rename_all_fields = "PascalCase")]
pub enum LayerEntry {
    Inline {
        name: String,
    },
    Reference {
        name_id: u32,
        properties: u32,
        unknown: u32,
    },
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct LayerList {
    pub num_entries: u16,
    pub entries: Vec<LayerEntry>,
    pub key: RecordKey,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TextGraphic {
    pub t: u16,
    pub layer: u16,
    pub key: RecordKey,
    pub str_graphic_wrapper_ptr: u32,
    pub coords_x: i32,
    pub coords_y: i32,
    pub len: u16,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct FontDefinition {
    pub height: u32,
    pub width: u32,
    pub character_space: u32,
    pub line_space: u32,
    pub stroke_width: u32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DefinitionTable {
    pub code: u16,
    pub key: RecordKey,
    pub next: u32,
    pub num_items: u32,
    pub count: u32,
    pub last_idx: u32,
    pub items_offset: usize,
    pub stride: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fonts: Option<Vec<FontDefinition>>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Property {
    pub t: u16,
    pub sub_type: u16,
    pub len: u32,
    pub name: String,
    pub r#type: String,
    pub unknown1: u32,
    pub unknown2: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_bytes: Option<Vec<u8>>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct KeyList {
    pub key: RecordKey,
    pub num_entries: u32,
    pub entries: Vec<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PadstackComponent {
    pub r#type: u16,
    #[serde(rename = "W")]
    pub width: i32,
    #[serde(rename = "H")]
    pub height: i32,
    pub z1: i32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub z2: u32,
    pub shape_ptr: u32,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Padstack {
    pub start_layer: u32,
    pub key: RecordKey,
    pub next: u32,
    pub pad_str: u32,
    pub drill_size: u32,
    pub drill_mark_size_x: u32,
    pub drill_mark_size_y: u32,
    pub drill_mark_shape: u32,
    pub flags: u16,
    pub drill_chars: u32,
    #[serde(rename = "ArrayNX")]
    pub array_nx: u16,
    #[serde(rename = "ArrayNY")]
    pub array_ny: u16,
    pub layer_count: u16,
    pub clearance_x: u32,
    pub clearance_y: u32,
    pub slot_x: u32,
    pub slot_y: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pad_type: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restricted_layer_span: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drill_metadata_words: Option<Vec<u32>>,
    pub num_fixed_comp_entries: usize,
    pub num_comps_per_layer: usize,
    pub plated: bool,
    pub components: Vec<PadstackComponent>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum VariableRecord {
    Field(Field),
    PairedNets(PairedNets),
    Padstack(Padstack),
    ConstraintSet(ConstraintSet),
    SignalIntegrityModel(SignalIntegrityModel),
    PadstackDimensions(PadstackDimensions),
    Blob(Blob),
    ConstraintRegion(ConstraintRegion),
    LayerList(LayerList),
    TextGraphic(TextGraphic),
    DefinitionTable(DefinitionTable),
    Property(Property),
    KeyList(KeyList),
}
impl VariableRecord {
    /// Preserve an absent `Next` instead of confusing another pointer with a list link.
    pub fn next_key(&self) -> Option<RecordKey> {
        let next = match self {
            Self::Field(r) => r.next,
            Self::Padstack(r) => r.next,
            Self::ConstraintSet(r) => r.next,
            Self::SignalIntegrityModel(r) => r.next,
            Self::PadstackDimensions(r) => r.next,
            Self::DefinitionTable(r) => r.next,
            Self::PairedNets(_)
            | Self::Blob(_)
            | Self::ConstraintRegion(_)
            | Self::LayerList(_)
            | Self::TextGraphic(_)
            | Self::Property(_)
            | Self::KeyList(_) => return None,
        };
        Some(RecordKey(next))
    }

    pub fn key(&self) -> RecordKey {
        match self {
            Self::Field(r) => r.key,
            Self::PairedNets(r) => r.key,
            Self::Padstack(r) => r.key,
            Self::ConstraintSet(r) => r.key,
            Self::SignalIntegrityModel(r) => r.key,
            Self::PadstackDimensions(r) => r.key,
            Self::Blob(r) => r.key,
            Self::ConstraintRegion(_) | Self::Property(_) => RecordKey(0),
            Self::LayerList(r) => r.key,
            Self::TextGraphic(r) => r.key,
            Self::DefinitionTable(r) => r.key,
            Self::KeyList(r) => r.key,
        }
    }
}

pub fn supports(kind: u8) -> bool {
    matches!(
        kind,
        0x03 | 0x1a | 0x1c..=0x1f | 0x21 | 0x27 | 0x2a | 0x31 | 0x36 | 0x3b | 0x3c
    )
}

pub(super) fn read(
    reader: &mut Reader<'_>,
    kind: u8,
    header: &BrdHeader,
    limits: &DecodeLimits,
    context: &ImportContext<'_>,
) -> Result<VariableRecord, ImportError> {
    let mut state = State {
        offset: reader.offset() - 1,
        reader,
        version: header.version,
        limits,
        context,
        allocated: 0,
    };
    match kind {
        0x03 => state.field().map(VariableRecord::Field),
        0x1a => state.paired_nets().map(VariableRecord::PairedNets),
        0x1c => state.padstack().map(VariableRecord::Padstack),
        0x1d => state.constraint_set().map(VariableRecord::ConstraintSet),
        0x1e => state
            .signal_model()
            .map(VariableRecord::SignalIntegrityModel),
        0x1f => state
            .padstack_dimensions()
            .map(VariableRecord::PadstackDimensions),
        0x21 => state.blob().map(VariableRecord::Blob),
        0x27 => {
            let end = header
                .constraint_end
                .checked_sub(1)
                .filter(|&end| end as usize >= state.reader.offset())
                .ok_or_else(|| state.invalid("CONSTRAINT_END", u64::from(header.constraint_end)))?;
            state.reader.seek(end as usize)?;
            Ok(VariableRecord::ConstraintRegion(ConstraintRegion {}))
        }
        0x2a => state.layers().map(VariableRecord::LayerList),
        0x31 => state.text().map(VariableRecord::TextGraphic),
        0x36 => state.definitions().map(VariableRecord::DefinitionTable),
        0x3b => state.property().map(VariableRecord::Property),
        0x3c => state.keys().map(VariableRecord::KeyList),
        _ => Err(ImportError::UnsupportedRecordLayout {
            record_type: kind,
            version: header.version,
            offset: state.offset,
        }),
    }
}

struct State<'r, 'b, 'c, 'l> {
    reader: &'r mut Reader<'b>,
    version: u16,
    limits: &'l DecodeLimits,
    context: &'c ImportContext<'c>,
    offset: usize,
    allocated: usize,
}

impl State<'_, '_, '_, '_> {
    fn invalid(&self, field: &'static str, value: u64) -> ImportError {
        ImportError::InvalidRecord {
            offset: self.offset,
            field,
            value,
        }
    }

    fn reserve(&mut self, bytes: usize) -> Result<(), ImportError> {
        let actual = self.allocated.saturating_add(bytes);
        if actual > self.limits.max_allocation_bytes {
            return Err(ImportError::DecodeLimit {
                offset: self.offset,
                actual: actual as u64,
                limit: self.limits.max_allocation_bytes as u64,
            });
        }
        self.allocated = actual;
        Ok(())
    }

    fn collection(
        &mut self,
        count: usize,
        source_width: usize,
        owned_width: usize,
    ) -> Result<(), ImportError> {
        if count > self.limits.max_entries {
            return Err(self.invalid("COLLECTION_COUNT", count as u64));
        }
        let source_bytes = count
            .checked_mul(source_width)
            .ok_or_else(|| self.invalid("COLLECTION_COUNT", count as u64))?;
        self.reader.ensure(source_bytes)?;
        let bytes = count
            .checked_mul(owned_width)
            .ok_or_else(|| self.invalid("COLLECTION_COUNT", count as u64))?;
        self.reserve(bytes)
    }

    fn skip_entries(&mut self, count: usize, stride: usize) -> Result<(), ImportError> {
        let bytes = count
            .checked_mul(stride)
            .ok_or_else(|| self.invalid("COLLECTION_COUNT", count as u64))?;
        self.reader.skip(bytes)
    }

    fn string(&mut self, bytes: usize) -> Result<String, ImportError> {
        self.context.check_cancelled()?;
        if bytes > self.limits.max_text_bytes {
            return Err(ImportError::DecodeLimit {
                offset: self.offset,
                actual: bytes as u64,
                limit: self.limits.max_text_bytes as u64,
            });
        }
        self.reader.ensure(bytes)?;
        self.reserve(bytes.saturating_mul(3))?;
        self.reader.fixed_string(bytes)
    }

    fn bytes(&mut self, bytes: usize) -> Result<Vec<u8>, ImportError> {
        self.context.check_cancelled()?;
        self.reader.ensure(bytes)?;
        self.reserve(bytes)?;
        Ok(self.reader.bytes(bytes)?.to_vec())
    }

    fn words(&mut self, count: usize) -> Result<Vec<u32>, ImportError> {
        self.collection(count, 4, size_of::<u32>())?;
        let mut words = Vec::with_capacity(count);
        for index in 0..count {
            if index.is_multiple_of(1024) {
                self.context.check_cancelled()?;
            }
            words.push(self.reader.u32()?);
        }
        Ok(words)
    }

    fn field(&mut self) -> Result<Field, ImportError> {
        self.reader.skip(1)?;
        let hdr1 = self.reader.u16()?;
        let key = RecordKey(self.reader.u32()?);
        let next = self.reader.u32()?;
        if self.version >= 172 {
            self.reader.skip(4)?;
        }
        let sub_type = self.reader.u8()?;
        self.reader.skip(1)?;
        let size = self.reader.u16()?;
        if self.version >= 172 {
            self.reader.skip(4)?;
        }
        let mut record = Field {
            hdr1,
            key,
            next,
            sub_type,
            size,
            value: None,
            words: None,
            payload_kind: None,
            value_bytes: None,
        };
        if [174, 175].contains(&self.version) && hdr1 == 755 && sub_type == 0x73 && size == 80 {
            record.payload_kind = Some("dimension-settings");
            record.value_bytes = Some(self.bytes(80)?);
            return Ok(record);
        }
        match sub_type {
            0x65 => {}
            0x64 | 0x66 | 0x67 | 0x6a => record.value = Some(FieldValue::Word(self.reader.u32()?)),
            0x69 => record.value = Some(FieldValue::Words(self.words(2)?)),
            0x68 | 0x6b | 0x6d | 0x6e | 0x6f | 0x71 | 0x73 | 0x78 => {
                record.value = Some(FieldValue::Text(self.string(usize::from(size))?))
            }
            0x6c => {
                let count = self.reader.u32()?;
                self.skip_entries(count as usize, 4)?;
            }
            0x72 => {
                let count = self.reader.u32()?;
                if count > 1_000_000 {
                    return Err(self.invalid("FIELD_WORD_COUNT", u64::from(count)));
                }
                record.words = Some(self.words(count as usize)?);
            }
            0x70 | 0x74 => {
                let words = self.reader.u16()?;
                let bytes = self.reader.u16()?;
                self.skip_entries(usize::from(words), 4)?;
                self.reader.skip(usize::from(bytes))?;
            }
            0xf6 => self.reader.skip(80)?,
            _ if size == 4 || size == 8 => {
                record.value = Some(FieldValue::Words(self.words(usize::from(size) / 4)?))
            }
            _ => return Err(self.invalid("FIELD_SUBTYPE", u64::from(sub_type))),
        }
        Ok(record)
    }

    fn paired_nets(&mut self) -> Result<PairedNets, ImportError> {
        if ![152, 157, 172, 174, 251].contains(&self.version) {
            return Err(ImportError::UnsupportedRecordLayout {
                record_type: 0x1a,
                version: self.version,
                offset: self.offset,
            });
        }
        let r#type = self.reader.u8()?;
        let t2 = self.reader.u16()?;
        let key = RecordKey(self.reader.u32()?);
        let unknown = if self.version >= 174 {
            Some(self.reader.u32()?)
        } else {
            None
        };
        Ok(PairedNets {
            r#type,
            t2,
            key,
            unknown,
            members: [self.net_member()?, self.net_member()?],
        })
    }
    fn net_member(&mut self) -> Result<PairedNetMember, ImportError> {
        Ok(PairedNetMember {
            net: self.reader.u32()?,
            next: self.reader.u32()?,
            metadata: self.words(8)?,
        })
    }

    fn constraint_set(&mut self) -> Result<ConstraintSet, ImportError> {
        self.reader.skip(3)?;
        let key = RecordKey(self.reader.u32()?);
        let next = self.reader.u32()?;
        let name_str_key = self.reader.u32()?;
        let field_ptr = if self.version >= 160 {
            Some(self.reader.u32()?)
        } else {
            None
        };
        let names = self.reader.u16()?;
        let dimensions = self.reader.u16()?;
        self.skip_entries(usize::from(names), 256)?;
        self.skip_entries(
            usize::from(dimensions),
            if self.version < 160 { 136 } else { 56 },
        )?;
        self.reader.skip(if self.version < 160 {
            12
        } else if self.version >= 172 {
            4
        } else {
            0
        })?;
        Ok(ConstraintSet {
            key,
            next,
            name_str_key,
            field_ptr,
        })
    }

    fn signal_model(&mut self) -> Result<SignalIntegrityModel, ImportError> {
        let r#type = self.reader.u8()?;
        let t2 = self.reader.u16()?;
        let key = RecordKey(self.reader.u32()?);
        let next = self.reader.u32()?;
        let (unknown2, unknown3) = if !(160..162).contains(&self.version) {
            (Some(self.reader.u16()?), Some(self.reader.u16()?))
        } else {
            (None, None)
        };
        let str_ptr = self.reader.u32()?;
        let size = self.reader.u32()?;
        let (mut unknown4, unknown5) = if self.version >= 251 {
            (Some(self.reader.u32()?), Some(self.reader.u32()?))
        } else {
            (None, None)
        };
        let string = self.string(size as usize)?;
        if (172..251).contains(&self.version) {
            unknown4 = Some(self.reader.u32()?);
        }
        Ok(SignalIntegrityModel {
            r#type,
            t2,
            key,
            next,
            unknown2,
            unknown3,
            str_ptr,
            size,
            string,
            unknown4,
            unknown5,
        })
    }

    fn padstack_dimensions(&mut self) -> Result<PadstackDimensions, ImportError> {
        self.reader.skip(3)?;
        let key = RecordKey(self.reader.u32()?);
        let next = self.reader.u32()?;
        self.reader.skip(if self.version < 160 { 46 } else { 14 })?;
        let count = self.reader.u16()?;
        let stride = if self.version < 160 {
            500
        } else if self.version >= 175 {
            384
        } else if self.version >= 162 {
            280
        } else {
            240
        };
        self.skip_entries(usize::from(count), stride)?;
        self.reader.skip(if !(160..172).contains(&self.version) {
            8
        } else {
            4
        })?;
        Ok(PadstackDimensions { key, next })
    }

    fn blob(&mut self) -> Result<Blob, ImportError> {
        self.reader.skip(3)?;
        let size = self.reader.u32()?;
        if size < 12 {
            return Err(self.invalid("BLOB_LENGTH", u64::from(size)));
        }
        let key = RecordKey(self.reader.u32()?);
        self.reader.skip(size as usize - 12)?;
        Ok(Blob { size, key })
    }

    fn layers(&mut self) -> Result<LayerList, ImportError> {
        self.reader.skip(1)?;
        let num_entries = self.reader.u16()?;
        if self.version >= 174 {
            self.reader.skip(4)?;
        }
        self.collection(
            usize::from(num_entries),
            if self.version < 165 { 36 } else { 12 },
            size_of::<LayerEntry>(),
        )?;
        let mut entries = Vec::with_capacity(usize::from(num_entries));
        for _ in 0..num_entries {
            self.context.check_cancelled()?;
            entries.push(if self.version < 165 {
                LayerEntry::Inline {
                    name: self.string(36)?,
                }
            } else {
                LayerEntry::Reference {
                    name_id: self.reader.u32()?,
                    properties: self.reader.u32()?,
                    unknown: self.reader.u32()?,
                }
            });
        }
        Ok(LayerList {
            num_entries,
            entries,
            key: RecordKey(self.reader.u32()?),
        })
    }

    fn text(&mut self) -> Result<TextGraphic, ImportError> {
        let t = self.reader.u8()?;
        let layer = self.reader.u16()?;
        let key = RecordKey(self.reader.u32()?);
        let str_graphic_wrapper_ptr = self.reader.u32()?;
        let coords_x = self.reader.i32()?;
        let coords_y = self.reader.i32()?;
        self.reader.skip(2)?;
        let len = self.reader.u16()?;
        if self.version >= 174 {
            self.reader.skip(4)?;
        }
        let value = self.string(usize::from(len))?;
        Ok(TextGraphic {
            t,
            layer,
            key,
            str_graphic_wrapper_ptr,
            coords_x,
            coords_y,
            len,
            value,
        })
    }

    fn definitions(&mut self) -> Result<DefinitionTable, ImportError> {
        self.reader.skip(1)?;
        let code = self.reader.u16()?;
        let key = RecordKey(self.reader.u32()?);
        let next = self.reader.u32()?;
        if self.version >= 172 {
            self.reader.skip(4)?;
        }
        let num_items = self.reader.u32()?;
        let count = self.reader.u32()?;
        let last_idx = self.reader.u32()?;
        self.reader
            .skip(4 + if self.version >= 174 { 4 } else { 0 })?;
        if num_items > 1_000_000 || count > num_items {
            return Err(self.invalid("DEFINITION_CAPACITY", u64::from(num_items)));
        }
        let stride = self.definition_stride(code)?;
        let items_offset = self.reader.offset();
        let fonts = if code == 8 {
            self.collection(num_items as usize, stride, 0)?;
            self.collection(count as usize, stride, size_of::<FontDefinition>())?;
            let mut fonts = Vec::with_capacity(count as usize);
            for index in 0..num_items {
                if index.is_multiple_of(1024) {
                    self.context.check_cancelled()?;
                }
                let entry = self.reader.offset();
                let font = self.font()?;
                if index < count {
                    fonts.push(font);
                }
                self.reader.seek(entry + stride)?;
            }
            Some(fonts)
        } else {
            self.skip_entries(num_items as usize, stride)?;
            None
        };
        Ok(DefinitionTable {
            code,
            key,
            next,
            num_items,
            count,
            last_idx,
            items_offset,
            stride,
            fonts,
        })
    }

    fn definition_stride(&self, code: u16) -> Result<usize, ImportError> {
        let v = self.version;
        Ok(match code {
            2 => 88 + usize::from(v >= 164) * 12 + usize::from(v >= 172) * 8,
            3 => (if v >= 172 { 64 } else { 32 }) + usize::from(v >= 174) * 4,
            4 if v < 160 => 20,
            5 => {
                if v < 160 {
                    16
                } else {
                    28 + usize::from(v >= 175) * 4
                }
            }
            6 => {
                if v >= 172 {
                    8
                } else {
                    208
                }
            }
            8 => {
                if v >= 251 {
                    64
                } else {
                    32 + usize::from(v >= 174) * 4 + usize::from(v >= 172) * 32
                }
            }
            11 => {
                if v < 160 {
                    254
                } else {
                    1016
                }
            }
            12 => 232,
            13 => 200,
            15 => 20,
            16 => 108 + usize::from(v >= 180) * 4,
            18 => 1052,
            _ => return Err(self.invalid("DEFINITION_CODE", u64::from(code))),
        })
    }
    fn font(&mut self) -> Result<FontDefinition, ImportError> {
        self.reader.skip(8)?;
        let before_extra = [174, 175].contains(&self.version) || self.version >= 251;
        let height = self.reader.u32()?;
        let width = self.reader.u32()?;
        if self.version >= 174 && !before_extra {
            self.reader.skip(4)?;
        }
        let character_space = self.reader.u32()?;
        let line_space = self.reader.u32()?;
        self.reader.skip(if before_extra && self.version < 251 {
            8
        } else {
            4
        })?;
        let stroke_width = self.reader.u32()?;
        Ok(FontDefinition {
            height,
            width,
            character_space,
            line_space,
            stroke_width,
        })
    }

    fn property(&mut self) -> Result<Property, ImportError> {
        let t = self.reader.u8()?;
        let sub_type = self.reader.u16()?;
        let len = self.reader.u32()?;
        let name = self.string(128)?;
        let r#type = self.string(32)?;
        let unknown1 = self.reader.u32()?;
        let unknown2 = self.reader.u32()?;
        let unknown3 = if self.version >= 172 {
            Some(self.reader.u32()?)
        } else {
            None
        };
        let model_name =
            name.starts_with("STEP3D_") || name.to_ascii_lowercase().ends_with(".sab.z");
        let attachment = if self.version >= 172 {
            unknown2 == 0 && unknown3 == Some(0x10000)
        } else {
            unknown2 == 0x10000
        };
        let embedded = t == 0 && sub_type == 0 && r#type.is_empty() && model_name && attachment;
        let (value, payload_kind, value_bytes) = if embedded {
            (
                None,
                Some("embedded-model"),
                Some(self.bytes(len as usize)?),
            )
        } else {
            (Some(self.string(len as usize)?), None, None)
        };
        Ok(Property {
            t,
            sub_type,
            len,
            name,
            r#type,
            unknown1,
            unknown2,
            unknown3,
            value,
            payload_kind,
            value_bytes,
        })
    }

    fn keys(&mut self) -> Result<KeyList, ImportError> {
        self.reader.skip(3)?;
        let key = RecordKey(self.reader.u32()?);
        if self.version >= 174 {
            self.reader.skip(4)?;
        }
        let num_entries = self.reader.u32()?;
        if num_entries > 1_000_000 {
            return Err(self.invalid("REFERENCE_COUNT", u64::from(num_entries)));
        }
        let entries = self.words(num_entries as usize)?;
        Ok(KeyList {
            key,
            num_entries,
            entries,
        })
    }

    fn padstack(&mut self) -> Result<Padstack, ImportError> {
        self.reader.skip(1)?;
        let trailing_entries = self.reader.u8()?;
        let mut record = Padstack {
            start_layer: u32::from(self.reader.u8()?),
            key: RecordKey(self.reader.u32()?),
            next: self.reader.u32()?,
            pad_str: self.reader.u32()?,
            ..Default::default()
        };
        if self.version < 172 {
            self.legacy_padstack_header(&mut record)?;
        } else {
            self.modern_padstack_header(&mut record)?;
        }
        if record.layer_count > 256 {
            return Err(self.invalid("PADSTACK_LAYER_COUNT", u64::from(record.layer_count)));
        }
        record.num_fixed_comp_entries = if self.version < 165 {
            10
        } else if self.version < 172 {
            11
        } else {
            21
        };
        record.num_comps_per_layer = if self.version < 172 { 3 } else { 4 };
        record.plated = record.flags & if self.version < 172 { 1 } else { 0x20 } != 0;
        let count = record.num_fixed_comp_entries
            + usize::from(record.layer_count) * record.num_comps_per_layer;
        let source_bytes = if self.version < 172 {
            count * 28 - 4
        } else {
            count * 36
        };
        self.reader.ensure(source_bytes)?;
        self.collection(count, 0, size_of::<PadstackComponent>())?;
        record.components = Vec::with_capacity(count);
        for index in 0..count {
            self.context.check_cancelled()?;
            record
                .components
                .push(self.padstack_component(index == count - 1)?);
        }
        self.skip_entries(
            usize::from(trailing_entries),
            if self.version < 172 { 32 } else { 40 },
        )?;
        if record.restricted_layer_span == Some(true) {
            let fixed = record.num_fixed_comp_entries;
            let width = record.num_comps_per_layer;
            let mut populated = (0..usize::from(record.layer_count)).filter(|&layer| {
                record.components[fixed + layer * width..fixed + (layer + 1) * width]
                    .iter()
                    .any(|component| component.r#type != 0)
            });
            if let Some(first) = populated.next() {
                let last = populated.next_back().unwrap_or(first);
                record.components.truncate(fixed + (last + 1) * width);
                record.components.drain(fixed..fixed + first * width);
                record.start_layer += first as u32;
                record.layer_count = (last - first + 1) as u16;
            }
        }
        Ok(record)
    }

    fn legacy_padstack_header(&mut self, record: &mut Padstack) -> Result<(), ImportError> {
        record.drill_size = self.reader.u32()?;
        self.reader.skip(4)?;
        record.drill_mark_size_x = self.reader.u32()?;
        record.drill_mark_size_y = self.reader.u32()?;
        self.reader.skip(8)?;
        record.drill_mark_shape = u32::from(self.reader.u8()?);
        record.flags = self.reader.u8()?;
        record.drill_chars = u32::from(self.reader.u8()?);
        self.reader.skip(1)?;
        let layer_flags = self.reader.u16()?;
        if self.version < 160 {
            record.restricted_layer_span = Some(layer_flags & 1 != 0);
        }
        record.array_nx = self.reader.u16()?;
        record.array_ny = self.reader.u16()?;
        record.layer_count = self.reader.u16()?;
        record.clearance_x = self.reader.u32()?;
        record.clearance_y = self.reader.u32()?;
        self.reader.skip(12)?;
        record.slot_x = self.reader.u32()?;
        record.slot_y = self.reader.u32()?;
        self.reader.skip(if self.version >= 165 { 8 } else { 4 })
    }

    fn modern_padstack_header(&mut self, record: &mut Padstack) -> Result<(), ImportError> {
        self.reader.skip(12)?;
        record.pad_type = Some(self.reader.u8()?);
        self.reader.skip(1)?;
        record.flags = self.reader.u8()?;
        self.reader.skip(9)?;
        record.array_nx = self.reader.u16()?;
        record.array_ny = self.reader.u16()?;
        record.layer_count = self.reader.u16()?;
        self.reader.skip(2)?;
        record.clearance_x = self.reader.u32()?;
        record.clearance_y = self.reader.u32()?;
        self.reader.skip(8)?;
        record.drill_size = self.reader.u32()?;
        self.reader.skip(8)?;
        record.slot_x = self.reader.u32()?;
        record.slot_y = self.reader.u32()?;
        self.reader.skip(8)?;
        record.drill_mark_size_x = self.reader.u32()?;
        record.drill_mark_size_y = self.reader.u32()?;
        record.drill_mark_shape = self.reader.u32()?;
        record.drill_chars = self.reader.u32()?;
        record.drill_metadata_words =
            Some(self.words(if self.version >= 180 { 29 } else { 21 })?);
        Ok(())
    }

    fn padstack_component(&mut self, last: bool) -> Result<PadstackComponent, ImportError> {
        let r#type = self.reader.u8()?;
        self.reader.skip(if self.version < 172 { 3 } else { 7 })?;
        let width = self.reader.i32()?;
        let height = self.reader.i32()?;
        let z1 = if self.version >= 172 {
            self.reader.i32()?
        } else {
            0
        };
        let offset_x = self.reader.i32()?;
        let offset_y = self.reader.i32()?;
        let (shape_ptr, z2) = if self.version < 172 {
            let shape = self.reader.u32()?;
            (shape, if last { 0 } else { self.reader.u32()? })
        } else {
            let z2 = self.reader.u32()?;
            (self.reader.u32()?, z2)
        };
        Ok(PadstackComponent {
            r#type,
            width,
            height,
            z1,
            offset_x,
            offset_y,
            z2,
            shape_ptr,
        })
    }
}
