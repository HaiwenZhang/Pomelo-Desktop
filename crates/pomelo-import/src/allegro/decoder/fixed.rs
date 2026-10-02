//! Type-specific fixed layouts ported from the frozen Web binary/layouts sources.
//! Unknown fields retain their source names; no complete-board AST is retained.
use super::super::{index::RecordKey, reader::Reader};
use crate::ImportError;
use serde::Serialize;

/// Legacy inline name or modern string-table identity.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum InlineOrReference {
    Inline(String),
    Reference(u32),
}
impl Default for InlineOrReference {
    fn default() -> Self {
        Self::Reference(0)
    }
}
/// Source fields for 0x01 (Arc); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Arc {
    #[serde(rename = "UnknownByte")]
    pub unknown_byte: u16,
    #[serde(rename = "SubType")]
    pub sub_type: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Parent")]
    pub parent: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "Unknown6", skip_serializing_if = "Option::is_none")]
    pub unknown6: Option<u32>,
    #[serde(rename = "Width")]
    pub width: u32,
    #[serde(rename = "StartX")]
    pub start_x: i32,
    #[serde(rename = "StartY")]
    pub start_y: i32,
    #[serde(rename = "EndX")]
    pub end_x: i32,
    #[serde(rename = "EndY")]
    pub end_y: i32,
    #[serde(rename = "CenterX")]
    pub center_x: f64,
    #[serde(rename = "CenterY")]
    pub center_y: f64,
    #[serde(rename = "Radius")]
    pub radius: f64,
    #[serde(rename = "BoundingBoxCoords")]
    pub bounding_box_coords: Vec<i32>,
}
fn read_arc(reader: &mut Reader<'_>, version: u16) -> Result<Arc, ImportError> {
    let mut record = Arc::default();
    reader.skip(1)?;
    record.unknown_byte = reader.u8()?;
    record.sub_type = reader.u8()?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.parent = reader.u32()?;
    record.unknown1 = reader.u32()?;
    if version >= 172 {
        record.unknown6 = Some(reader.u32()?);
    }
    record.width = reader.u32()?;
    if version < 160 {
        std::mem::swap(&mut record.width, &mut record.unknown1);
    }
    record.start_x = reader.i32()?;
    record.start_y = reader.i32()?;
    record.end_x = reader.i32()?;
    record.end_y = reader.i32()?;
    record.center_x = if version < 160 {
        f64::from(reader.i32()?)
    } else {
        reader.float()?
    };
    record.center_y = if version < 160 {
        f64::from(reader.i32()?)
    } else {
        reader.float()?
    };
    record.radius = if version < 160 {
        f64::from(reader.i32()?)
    } else {
        reader.float()?
    };
    record.bounding_box_coords = reader.i32s(4)?;
    Ok(record)
}
/// Source fields for 0x0e (FootprintRectangle); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FootprintRectangle {
    #[serde(rename = "T")]
    pub t: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "FpPtr")]
    pub fp_ptr: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "Unknown4", skip_serializing_if = "Option::is_none")]
    pub unknown4: Option<u32>,
    #[serde(rename = "Unknown5", skip_serializing_if = "Option::is_none")]
    pub unknown5: Option<u32>,
    #[serde(rename = "Coords")]
    pub coords: Vec<i32>,
    #[serde(rename = "UnknownArr")]
    pub unknown_arr: Vec<u32>,
    #[serde(rename = "Rotation")]
    pub rotation: u32,
}
fn read_footprint_rectangle(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<FootprintRectangle, ImportError> {
    let mut record = FootprintRectangle {
        t: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        fp_ptr: reader.u32()?,
        unknown1: reader.u32()?,
        unknown2: reader.u32()?,
        ..Default::default()
    };
    if version >= 160 {
        record.unknown3 = Some(reader.u32()?);
    }
    if version >= 172 {
        record.unknown4 = Some(reader.u32()?);
        record.unknown5 = Some(reader.u32()?);
    }
    record.coords = reader.i32s(4)?;
    record.unknown_arr = reader.u32s(3)?;
    record.rotation = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x14 (Graphic); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Graphic {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Parent")]
    pub parent: u32,
    #[serde(rename = "Flags")]
    pub flags: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "SegmentPtr")]
    pub segment_ptr: u32,
    #[serde(rename = "Ptr0x03")]
    pub ptr0x03: u32,
    #[serde(rename = "Ptr0x26")]
    pub ptr0x26: u32,
}
fn read_graphic(reader: &mut Reader<'_>, version: u16) -> Result<Graphic, ImportError> {
    let mut record = Graphic {
        r#type: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        parent: reader.u32()?,
        flags: if version < 160 { 0 } else { reader.u32()? },
        ..Default::default()
    };
    if version >= 172 {
        record.unknown2 = Some(reader.u32()?);
    }
    record.segment_ptr = reader.u32()?;
    record.ptr0x03 = reader.u32()?;
    record.ptr0x26 = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x15 (Segment); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Segment {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Parent")]
    pub parent: u32,
    #[serde(rename = "Flags")]
    pub flags: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Width")]
    pub width: u32,
    #[serde(rename = "StartX")]
    pub start_x: i32,
    #[serde(rename = "StartY")]
    pub start_y: i32,
    #[serde(rename = "EndX")]
    pub end_x: i32,
    #[serde(rename = "EndY")]
    pub end_y: i32,
}
fn read_segment(reader: &mut Reader<'_>, version: u16) -> Result<Segment, ImportError> {
    let mut record = Segment::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.parent = reader.u32()?;
    record.flags = reader.u32()?;
    if version >= 172 {
        record.unknown2 = Some(reader.u32()?);
    }
    record.width = reader.u32()?;
    if version < 160 {
        std::mem::swap(&mut record.width, &mut record.flags);
    }
    record.start_x = reader.i32()?;
    record.start_y = reader.i32()?;
    record.end_x = reader.i32()?;
    record.end_y = reader.i32()?;
    Ok(record)
}
/// Source fields for 0x24 (Rectangle); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Rectangle {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Parent")]
    pub parent: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Coords")]
    pub coords: Vec<i32>,
    #[serde(rename = "Ptr2")]
    pub ptr2: u32,
    #[serde(rename = "Unknown3")]
    pub unknown3: u32,
    #[serde(rename = "Unknown4")]
    pub unknown4: u32,
    #[serde(rename = "Rotation")]
    pub rotation: u32,
}
fn read_rectangle(reader: &mut Reader<'_>, version: u16) -> Result<Rectangle, ImportError> {
    let mut record = Rectangle {
        r#type: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        parent: reader.u32()?,
        unknown1: reader.u32()?,
        ..Default::default()
    };
    if version >= 172 {
        record.unknown2 = Some(reader.u32()?);
    }
    record.coords = reader.i32s(4)?;
    record.ptr2 = reader.u32()?;
    record.unknown3 = reader.u32()?;
    record.unknown4 = reader.u32()?;
    record.rotation = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x28 (Shape); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Shape {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Ptr1")]
    pub ptr1: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "Ptr2")]
    pub ptr2: u32,
    #[serde(rename = "Ptr3")]
    pub ptr3: u32,
    #[serde(rename = "FirstKeepoutPtr")]
    pub first_keepout_ptr: u32,
    #[serde(rename = "FirstSegmentPtr")]
    pub first_segment_ptr: u32,
    #[serde(rename = "Unknown4")]
    pub unknown4: u32,
    #[serde(rename = "Unknown5")]
    pub unknown5: u32,
    #[serde(rename = "TablePtr", skip_serializing_if = "Option::is_none")]
    pub table_ptr: Option<u32>,
    #[serde(rename = "Ptr6")]
    pub ptr6: u32,
    #[serde(rename = "TablePtr_16x", skip_serializing_if = "Option::is_none")]
    pub table_ptr_16x: Option<u32>,
    #[serde(rename = "Coords")]
    pub coords: Vec<i32>,
}
fn read_shape(reader: &mut Reader<'_>, version: u16) -> Result<Shape, ImportError> {
    let mut record = Shape {
        r#type: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        ptr1: reader.u32()?,
        unknown1: if version < 160 { 0 } else { reader.u32()? },
        ..Default::default()
    };
    if version >= 172 {
        record.unknown2 = Some(reader.u32()?);
        record.unknown3 = Some(reader.u32()?);
    }
    record.ptr2 = reader.u32()?;
    record.ptr3 = reader.u32()?;
    record.first_keepout_ptr = reader.u32()?;
    record.first_segment_ptr = reader.u32()?;
    record.unknown4 = reader.u32()?;
    record.unknown5 = reader.u32()?;
    if version >= 172 {
        record.table_ptr = Some(reader.u32()?);
    }
    record.ptr6 = reader.u32()?;
    if version < 172 {
        record.table_ptr_16x = Some(reader.u32()?);
    }
    record.coords = reader.i32s(4)?;
    Ok(record)
}
/// Source fields for 0x34 (Keepout); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Keepout {
    #[serde(rename = "T")]
    pub t: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Ptr1")]
    pub ptr1: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Flags")]
    pub flags: u32,
    #[serde(rename = "FirstSegmentPtr")]
    pub first_segment_ptr: u32,
    #[serde(rename = "Ptr3")]
    pub ptr3: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
}
fn read_keepout(reader: &mut Reader<'_>, version: u16) -> Result<Keepout, ImportError> {
    let mut record = Keepout {
        t: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        ptr1: reader.u32()?,
        ..Default::default()
    };
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    record.flags = if version < 160 { 0 } else { reader.u32()? };
    record.first_segment_ptr = reader.u32()?;
    record.ptr3 = reader.u32()?;
    record.unknown2 = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x06 (Component); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Component {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "CompDeviceType")]
    pub comp_device_type: u32,
    #[serde(rename = "SymbolName")]
    pub symbol_name: u32,
    #[serde(rename = "FirstInstPtr")]
    pub first_inst_ptr: u32,
    #[serde(rename = "PtrFunctionSlot")]
    pub ptr_function_slot: u32,
    #[serde(rename = "PtrPinNumber")]
    pub ptr_pin_number: u32,
    #[serde(rename = "Fields")]
    pub fields: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
}
fn read_component(reader: &mut Reader<'_>, version: u16) -> Result<Component, ImportError> {
    let mut record = Component::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.comp_device_type = reader.u32()?;
    record.symbol_name = reader.u32()?;
    record.first_inst_ptr = reader.u32()?;
    record.ptr_function_slot = reader.u32()?;
    record.ptr_pin_number = reader.u32()?;
    record.fields = reader.u32()?;
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x07 (ComponentInstance); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct ComponentInstance {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "RefDes", skip_serializing_if = "Option::is_none")]
    pub ref_des: Option<String>,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "FpInstPtr")]
    pub fp_inst_ptr: u32,
    #[serde(rename = "FunctionInstPtr")]
    pub function_inst_ptr: u32,
    #[serde(rename = "X03Ptr")]
    pub x03_ptr: u32,
    #[serde(rename = "Unknown5")]
    pub unknown5: u32,
    #[serde(rename = "FirstPadPtr")]
    pub first_pad_ptr: u32,
    #[serde(rename = "UnknownPtr1", skip_serializing_if = "Option::is_none")]
    pub unknown_ptr1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "Unknown4", skip_serializing_if = "Option::is_none")]
    pub unknown4: Option<u32>,
    #[serde(rename = "RefDesStrPtr", skip_serializing_if = "Option::is_none")]
    pub ref_des_str_ptr: Option<u32>,
}
fn read_component_instance(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<ComponentInstance, ImportError> {
    let mut record = ComponentInstance::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    if version < 160 {
        record.ref_des = Some(reader.fixed_string(32)?);
        record.next = reader.u32()?;
        record.fp_inst_ptr = reader.u32()?;
        record.function_inst_ptr = reader.u32()?;
        record.x03_ptr = reader.u32()?;
        record.unknown5 = reader.u32()?;
        record.first_pad_ptr = reader.u32()?;
        return Ok(record);
    }
    record.next = reader.u32()?;
    if version >= 172 {
        record.unknown_ptr1 = Some(reader.u32()?);
        record.unknown2 = Some(reader.u32()?);
        record.unknown3 = Some(reader.u32()?);
    }
    record.fp_inst_ptr = reader.u32()?;
    if version < 172 {
        record.unknown4 = Some(reader.u32()?);
    }
    record.ref_des_str_ptr = Some(reader.u32()?);
    record.function_inst_ptr = reader.u32()?;
    record.x03_ptr = reader.u32()?;
    record.unknown5 = reader.u32()?;
    record.first_pad_ptr = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x0f (FunctionSlot); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FunctionSlot {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "SlotName")]
    pub slot_name: InlineOrReference,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "CompDeviceTypePtr", skip_serializing_if = "Option::is_none")]
    pub comp_device_type_ptr: Option<u32>,
    #[serde(rename = "Next", skip_serializing_if = "Option::is_none")]
    pub next: Option<u32>,
    #[serde(rename = "Ptr0x06")]
    pub ptr0x06: u32,
    #[serde(rename = "Ptr0x11")]
    pub ptr0x11: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
}
fn read_function_slot(reader: &mut Reader<'_>, version: u16) -> Result<FunctionSlot, ImportError> {
    let mut record = FunctionSlot::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.slot_name = if version < 160 {
        InlineOrReference::Inline(reader.fixed_string(32)?)
    } else {
        InlineOrReference::Reference(reader.u32()?)
    };
    if version >= 174 {
        record.unknown1 = Some(reader.u32()?);
    }
    if version < 190 {
        reader.skip(32)?;
    }
    if version >= 190 {
        record.comp_device_type_ptr = Some(reader.u32()?);
    }
    if version >= 172 {
        record.next = Some(reader.u32()?);
    }
    record.ptr0x06 = reader.u32()?;
    record.ptr0x11 = reader.u32()?;
    record.unknown2 = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x10 (FunctionInstance); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FunctionInstance {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "FunctionName")]
    pub function_name: InlineOrReference,
    #[serde(rename = "ComponentInstPtr")]
    pub component_inst_ptr: u32,
    #[serde(rename = "PtrX12")]
    pub ptr_x12: u32,
    #[serde(rename = "Slots")]
    pub slots: u32,
    #[serde(rename = "Fields")]
    pub fields: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
}
fn read_function_instance(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<FunctionInstance, ImportError> {
    let mut record = FunctionInstance::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    if version < 160 {
        record.function_name = InlineOrReference::Inline(reader.fixed_string(32)?);
        record.component_inst_ptr = reader.u32()?;
        record.ptr_x12 = reader.u32()?;
        record.slots = reader.u32()?;
        record.fields = reader.u32()?;
        return Ok(record);
    }
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    record.component_inst_ptr = reader.u32()?;
    if version >= 174 {
        record.unknown2 = Some(reader.u32()?);
    }
    record.ptr_x12 = reader.u32()?;
    record.unknown3 = Some(reader.u32()?);
    record.function_name = InlineOrReference::Reference(reader.u32()?);
    record.slots = reader.u32()?;
    record.fields = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x2b (FootprintDefinition); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FootprintDefinition {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "FpStrRef")]
    pub fp_str_ref: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "Coords")]
    pub coords: Vec<u32>,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "FirstInstPtr")]
    pub first_inst_ptr: u32,
    #[serde(rename = "UnknownPtr3")]
    pub unknown_ptr3: u32,
    #[serde(rename = "UnknownPtr4")]
    pub unknown_ptr4: u32,
    #[serde(rename = "UnknownPtr5")]
    pub unknown_ptr5: u32,
    #[serde(rename = "FieldsPtr")]
    pub fields_ptr: u32,
    #[serde(rename = "UnknownPtr6")]
    pub unknown_ptr6: u32,
    #[serde(rename = "UnknownPtr7")]
    pub unknown_ptr7: u32,
    #[serde(rename = "UnknownPtr8")]
    pub unknown_ptr8: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
}
fn read_footprint_definition(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<FootprintDefinition, ImportError> {
    let mut record = FootprintDefinition::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.fp_str_ref = reader.u32()?;
    record.unknown1 = reader.u32()?;
    record.coords = reader.u32s(4)?;
    record.next = reader.u32()?;
    record.first_inst_ptr = reader.u32()?;
    record.unknown_ptr3 = reader.u32()?;
    record.unknown_ptr4 = reader.u32()?;
    record.unknown_ptr5 = reader.u32()?;
    record.fields_ptr = reader.u32()?;
    record.unknown_ptr6 = reader.u32()?;
    record.unknown_ptr7 = reader.u32()?;
    record.unknown_ptr8 = reader.u32()?;
    if version >= 164 {
        record.unknown2 = Some(reader.u32()?);
    }
    if version >= 172 {
        record.unknown3 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x2d (FootprintInstance); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FootprintInstance {
    #[serde(rename = "UnknownByte1")]
    pub unknown_byte1: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "UnknownByte2")]
    pub unknown_byte2: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Flags")]
    pub flags: u32,
    #[serde(rename = "Rotation")]
    pub rotation: u32,
    #[serde(rename = "CoordX")]
    pub coord_x: i32,
    #[serde(rename = "CoordY")]
    pub coord_y: i32,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "InstRef16x", skip_serializing_if = "Option::is_none")]
    pub inst_ref16x: Option<u32>,
    #[serde(rename = "GraphicPtr")]
    pub graphic_ptr: u32,
    #[serde(rename = "FirstPadPtr")]
    pub first_pad_ptr: u32,
    #[serde(rename = "TextPtr")]
    pub text_ptr: u32,
    #[serde(rename = "AssemblyPtr")]
    pub assembly_ptr: u32,
    #[serde(rename = "AreasPtr")]
    pub areas_ptr: u32,
    #[serde(rename = "UnknownPtr1")]
    pub unknown_ptr1: u32,
    #[serde(rename = "UnknownPtr2")]
    pub unknown_ptr2: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u16>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u16>,
    #[serde(rename = "Unknown4", skip_serializing_if = "Option::is_none")]
    pub unknown4: Option<u32>,
    #[serde(rename = "InstRef", skip_serializing_if = "Option::is_none")]
    pub inst_ref: Option<u32>,
}
fn read_footprint_instance(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<FootprintInstance, ImportError> {
    let mut record = FootprintInstance {
        unknown_byte1: reader.u8()?,
        layer: reader.u8()?,
        unknown_byte2: reader.u8()?,
        key: RecordKey(reader.u32()?),
        ..Default::default()
    };
    if version < 160 {
        record.flags = reader.u32()?;
        record.rotation = reader.u32()?;
        record.coord_x = reader.i32()?;
        record.coord_y = reader.i32()?;
        record.next = reader.u32()?;
        record.inst_ref16x = Some(reader.u32()?);
        record.graphic_ptr = reader.u32()?;
        record.first_pad_ptr = reader.u32()?;
        record.text_ptr = reader.u32()?;
        record.assembly_ptr = reader.u32()?;
        record.areas_ptr = reader.u32()?;
        record.unknown_ptr1 = reader.u32()?;
        record.unknown_ptr2 = reader.u32()?;
        return Ok(record);
    }
    record.next = reader.u32()?;
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    if version < 172 {
        record.inst_ref16x = Some(reader.u32()?);
    }
    record.unknown2 = Some(reader.u16()?);
    record.unknown3 = Some(reader.u16()?);
    if version >= 172 {
        record.unknown4 = Some(reader.u32()?);
    }
    record.flags = reader.u32()?;
    record.rotation = reader.u32()?;
    record.coord_x = reader.i32()?;
    record.coord_y = reader.i32()?;
    if version >= 172 {
        record.inst_ref = Some(reader.u32()?);
    }
    record.graphic_ptr = reader.u32()?;
    record.first_pad_ptr = reader.u32()?;
    record.text_ptr = reader.u32()?;
    record.assembly_ptr = reader.u32()?;
    record.areas_ptr = reader.u32()?;
    record.unknown_ptr1 = reader.u32()?;
    record.unknown_ptr2 = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x08 (PinNumber); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct PinNumber {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "R")]
    pub r: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Previous", skip_serializing_if = "Option::is_none")]
    pub previous: Option<u32>,
    #[serde(rename = "Number", skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(rename = "StrPtr16x", skip_serializing_if = "Option::is_none")]
    pub str_ptr16x: Option<u32>,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "StrPtr", skip_serializing_if = "Option::is_none")]
    pub str_ptr: Option<u32>,
    #[serde(rename = "PinNamePtr")]
    pub pin_name_ptr: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Ptr4")]
    pub ptr4: u32,
}
fn read_pin_number(reader: &mut Reader<'_>, version: u16) -> Result<PinNumber, ImportError> {
    let mut record = PinNumber {
        r#type: reader.u8()?,
        r: reader.u16()?,
        key: RecordKey(reader.u32()?),
        ..Default::default()
    };
    if version >= 172 {
        record.previous = Some(reader.u32()?);
    }
    if version < 172 {
        if version < 160 {
            record.number = Some(reader.fixed_string(32)?);
        } else {
            record.str_ptr16x = Some(reader.u32()?);
        }
    }
    record.next = reader.u32()?;
    if version >= 172 {
        record.str_ptr = Some(reader.u32()?);
    }
    record.pin_name_ptr = reader.u32()?;
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    record.ptr4 = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x0c (PinDefinition); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct PinDefinition {
    #[serde(rename = "T")]
    pub t: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Shape", skip_serializing_if = "Option::is_none")]
    pub shape: Option<u16>,
    #[serde(rename = "DrillChar", skip_serializing_if = "Option::is_none")]
    pub drill_char: Option<u16>,
    #[serde(rename = "UnknownPadding", skip_serializing_if = "Option::is_none")]
    pub unknown_padding: Option<u16>,
    #[serde(rename = "Shape16x", skip_serializing_if = "Option::is_none")]
    pub shape16x: Option<u32>,
    #[serde(rename = "DrillChars", skip_serializing_if = "Option::is_none")]
    pub drill_chars: Option<u32>,
    #[serde(rename = "Unknown_16x", skip_serializing_if = "Option::is_none")]
    pub unknown_16x: Option<u32>,
    #[serde(rename = "Unknown4")]
    pub unknown4: u32,
    #[serde(rename = "Unknown5", skip_serializing_if = "Option::is_none")]
    pub unknown5: Option<u32>,
    #[serde(rename = "Coords")]
    pub coords: Vec<i32>,
    #[serde(rename = "Size")]
    pub size: Vec<i32>,
    #[serde(rename = "GroupPtr")]
    pub group_ptr: u32,
    #[serde(rename = "Unknown6")]
    pub unknown6: u32,
    #[serde(rename = "Unknown7")]
    pub unknown7: u32,
    #[serde(rename = "Unknown8", skip_serializing_if = "Option::is_none")]
    pub unknown8: Option<u32>,
}
fn read_pin_definition(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<PinDefinition, ImportError> {
    let mut record = PinDefinition {
        t: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        ..Default::default()
    };
    if version >= 160 {
        record.unknown1 = Some(reader.u32()?);
        record.unknown2 = Some(reader.u32()?);
    }
    if version < 172 {
        record.shape = Some(reader.u8()?);
        record.drill_char = Some(reader.u8()?);
        record.unknown_padding = Some(reader.u16()?);
    }
    if version >= 172 {
        record.shape16x = Some(reader.u32()?);
        record.drill_chars = Some(reader.u32()?);
        record.unknown_16x = Some(reader.u32()?);
    }
    record.unknown4 = reader.u32()?;
    if version >= 180 {
        record.unknown5 = Some(reader.u32()?);
    }
    record.coords = reader.i32s(2)?;
    record.size = reader.i32s(2)?;
    record.group_ptr = reader.u32()?;
    record.unknown6 = reader.u32()?;
    record.unknown7 = reader.u32()?;
    if (174..180).contains(&version) {
        record.unknown8 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x0d (Pad); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Pad {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "NameStrId", skip_serializing_if = "Option::is_none")]
    pub name_str_id: Option<u32>,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "CoordsX")]
    pub coords_x: i32,
    #[serde(rename = "CoordsY")]
    pub coords_y: i32,
    #[serde(rename = "PadStack")]
    pub pad_stack: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "Flags")]
    pub flags: u32,
    #[serde(rename = "Rotation")]
    pub rotation: u32,
}
fn read_pad(reader: &mut Reader<'_>, version: u16) -> Result<Pad, ImportError> {
    let mut record = Pad::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    if version < 160 {
        record.name = Some(reader.fixed_string(32)?);
    } else {
        record.name_str_id = Some(reader.u32()?);
    }
    record.next = reader.u32()?;
    if version >= 174 {
        record.unknown1 = Some(reader.u32()?);
    }
    record.coords_x = reader.i32()?;
    record.coords_y = reader.i32()?;
    record.pad_stack = reader.u32()?;
    record.unknown2 = reader.u32()?;
    if version >= 172 {
        record.unknown3 = Some(reader.u32()?);
    }
    record.flags = reader.u32()?;
    record.rotation = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x11 (PinName); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct PinName {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "R")]
    pub r: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "PinName", skip_serializing_if = "Option::is_none")]
    pub pin_name: Option<String>,
    #[serde(rename = "PinNameStrPtr", skip_serializing_if = "Option::is_none")]
    pub pin_name_str_ptr: Option<u32>,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "PinNumberPtr")]
    pub pin_number_ptr: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
}
fn read_pin_name(reader: &mut Reader<'_>, version: u16) -> Result<PinName, ImportError> {
    let mut record = PinName {
        r#type: reader.u8()?,
        r: reader.u16()?,
        key: RecordKey(reader.u32()?),
        ..Default::default()
    };
    if version < 160 {
        record.pin_name = Some(reader.fixed_string(32)?);
    } else {
        record.pin_name_str_ptr = Some(reader.u32()?);
    }
    record.next = reader.u32()?;
    record.pin_number_ptr = reader.u32()?;
    record.unknown1 = reader.u32()?;
    if version >= 174 {
        record.unknown2 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x29 (Pin); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Pin {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "T")]
    pub t: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Ptr1")]
    pub ptr1: u32,
    #[serde(rename = "Ptr2")]
    pub ptr2: u32,
    #[serde(rename = "Null")]
    pub null: u32,
    #[serde(rename = "Ptr3")]
    pub ptr3: u32,
    #[serde(rename = "Coord1")]
    pub coord1: i32,
    #[serde(rename = "Coord2")]
    pub coord2: i32,
    #[serde(rename = "PtrPadstack")]
    pub ptr_padstack: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "PtrX30")]
    pub ptr_x30: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
    #[serde(rename = "Unknown3")]
    pub unknown3: u32,
    #[serde(rename = "Unknown4")]
    pub unknown4: u32,
}
fn read_pin(reader: &mut Reader<'_>, _version: u16) -> Result<Pin, ImportError> {
    let record = Pin {
        r#type: reader.u8()?,
        t: reader.u16()?,
        key: RecordKey(reader.u32()?),
        ptr1: reader.u32()?,
        ptr2: reader.u32()?,
        null: reader.u32()?,
        ptr3: reader.u32()?,
        coord1: reader.i32()?,
        coord2: reader.i32()?,
        ptr_padstack: reader.u32()?,
        unknown1: reader.u32()?,
        ptr_x30: reader.u32()?,
        unknown2: reader.u32()?,
        unknown3: reader.u32()?,
        unknown4: reader.u32()?,
    };
    Ok(record)
}
/// Source fields for 0x32 (PlacedPad); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct PlacedPad {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "NetPtr")]
    pub net_ptr: u32,
    #[serde(rename = "Flags")]
    pub flags: u32,
    #[serde(rename = "Prev", skip_serializing_if = "Option::is_none")]
    pub prev: Option<u32>,
    #[serde(rename = "NextInFp")]
    pub next_in_fp: u32,
    #[serde(rename = "ParentFp")]
    pub parent_fp: u32,
    #[serde(rename = "Track")]
    pub track: u32,
    #[serde(rename = "PadPtr")]
    pub pad_ptr: u32,
    #[serde(rename = "Ptr6")]
    pub ptr6: u32,
    #[serde(rename = "Ratline")]
    pub ratline: u32,
    #[serde(rename = "PtrPinNumber")]
    pub ptr_pin_number: u32,
    #[serde(rename = "NextInCompInst")]
    pub next_in_comp_inst: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "NameText")]
    pub name_text: u32,
    #[serde(rename = "Ptr11")]
    pub ptr11: u32,
    #[serde(rename = "Coords")]
    pub coords: Vec<i32>,
}
fn read_placed_pad(reader: &mut Reader<'_>, version: u16) -> Result<PlacedPad, ImportError> {
    let mut record = PlacedPad {
        r#type: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        net_ptr: reader.u32()?,
        flags: if version < 160 { 0 } else { reader.u32()? },
        ..Default::default()
    };
    if version >= 172 {
        record.prev = Some(reader.u32()?);
    }
    record.next_in_fp = reader.u32()?;
    record.parent_fp = reader.u32()?;
    record.track = reader.u32()?;
    record.pad_ptr = reader.u32()?;
    record.ptr6 = reader.u32()?;
    record.ratline = reader.u32()?;
    record.ptr_pin_number = reader.u32()?;
    record.next_in_comp_inst = reader.u32()?;
    if version >= 172 {
        record.unknown2 = Some(reader.u32()?);
    }
    record.name_text = reader.u32()?;
    record.ptr11 = reader.u32()?;
    record.coords = reader.i32s(4)?;
    Ok(record)
}
/// Source fields for 0x33 (Via); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Via {
    #[serde(rename = "LayerInfo")]
    pub layer_info: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "NetPtr")]
    pub net_ptr: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "UnknownPtr1")]
    pub unknown_ptr1: u32,
    #[serde(rename = "UnknownPtr2", skip_serializing_if = "Option::is_none")]
    pub unknown_ptr2: Option<u32>,
    #[serde(rename = "CoordsX")]
    pub coords_x: i32,
    #[serde(rename = "CoordsY")]
    pub coords_y: i32,
    #[serde(rename = "Connection")]
    pub connection: u32,
    #[serde(rename = "Padstack")]
    pub padstack: u32,
    #[serde(rename = "UnknownPtr5")]
    pub unknown_ptr5: u32,
    #[serde(rename = "UnknownPtr6")]
    pub unknown_ptr6: u32,
    #[serde(rename = "Unknown4")]
    pub unknown4: u32,
    #[serde(rename = "Unknown5")]
    pub unknown5: u32,
    #[serde(rename = "BoundingBoxCoords")]
    pub bounding_box_coords: Vec<i32>,
}
fn read_via(reader: &mut Reader<'_>, version: u16) -> Result<Via, ImportError> {
    let mut record = Via::default();
    reader.skip(1)?;
    record.layer_info = reader.u16()?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.net_ptr = reader.u32()?;
    if version >= 160 {
        record.unknown2 = Some(reader.u32()?);
    }
    if version >= 172 {
        record.unknown3 = Some(reader.u32()?);
    }
    record.unknown_ptr1 = reader.u32()?;
    if version >= 172 {
        record.unknown_ptr2 = Some(reader.u32()?);
    }
    record.coords_x = reader.i32()?;
    record.coords_y = reader.i32()?;
    record.connection = reader.u32()?;
    record.padstack = reader.u32()?;
    record.unknown_ptr5 = reader.u32()?;
    record.unknown_ptr6 = reader.u32()?;
    record.unknown4 = reader.u32()?;
    record.unknown5 = reader.u32()?;
    record.bounding_box_coords = reader.i32s(4)?;
    Ok(record)
}
/// Source fields for 0x04 (NetAssignment); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct NetAssignment {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "R")]
    pub r: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Net")]
    pub net: u32,
    #[serde(rename = "ConnItem")]
    pub conn_item: u32,
    #[serde(rename = "Unknown", skip_serializing_if = "Option::is_none")]
    pub unknown: Option<u32>,
}
fn read_net_assignment(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<NetAssignment, ImportError> {
    let mut record = NetAssignment {
        r#type: reader.u8()?,
        r: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        net: reader.u32()?,
        conn_item: reader.u32()?,
        ..Default::default()
    };
    if version >= 174 {
        record.unknown = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x05 (Track); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Track {
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "NetAssignment")]
    pub net_assignment: u32,
    #[serde(rename = "UnknownPtr1")]
    pub unknown_ptr1: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "UnknownPtr2a")]
    pub unknown_ptr2a: u32,
    #[serde(rename = "UnknownPtr2b")]
    pub unknown_ptr2b: u32,
    #[serde(rename = "Unknown4", skip_serializing_if = "Option::is_none")]
    pub unknown4: Option<u32>,
    #[serde(rename = "UnknownPtr3a")]
    pub unknown_ptr3a: u32,
    #[serde(rename = "UnknownPtr3b")]
    pub unknown_ptr3b: u32,
    #[serde(rename = "Unknown5a", skip_serializing_if = "Option::is_none")]
    pub unknown5a: Option<u32>,
    #[serde(rename = "Unknown5b", skip_serializing_if = "Option::is_none")]
    pub unknown5b: Option<u32>,
    #[serde(rename = "FirstSegPtr")]
    pub first_seg_ptr: u32,
    #[serde(rename = "UnknownPtr5")]
    pub unknown_ptr5: u32,
    #[serde(rename = "Unknown6")]
    pub unknown6: u32,
}
fn read_track(reader: &mut Reader<'_>, version: u16) -> Result<Track, ImportError> {
    let mut record = Track::default();
    reader.skip(1)?;
    record.layer = reader.u16()?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.net_assignment = reader.u32()?;
    record.unknown_ptr1 = reader.u32()?;
    if version >= 160 {
        record.unknown2 = Some(reader.u32()?);
        record.unknown3 = Some(reader.u32()?);
    }
    record.unknown_ptr2a = reader.u32()?;
    record.unknown_ptr2b = reader.u32()?;
    if version >= 160 {
        record.unknown4 = Some(reader.u32()?);
    }
    record.unknown_ptr3a = reader.u32()?;
    record.unknown_ptr3b = reader.u32()?;
    if version >= 172 {
        record.unknown5a = Some(reader.u32()?);
        record.unknown5b = Some(reader.u32()?);
    }
    record.first_seg_ptr = reader.u32()?;
    record.unknown_ptr5 = reader.u32()?;
    record.unknown6 = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x09 (FillLink); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FillLink {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "UnknownArray")]
    pub unknown_array: Vec<u32>,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "UnknownPtr1")]
    pub unknown_ptr1: u32,
    #[serde(rename = "UnknownPtr2")]
    pub unknown_ptr2: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
    #[serde(rename = "UnknownPtr3", skip_serializing_if = "Option::is_none")]
    pub unknown_ptr3: Option<u32>,
    #[serde(rename = "UnknownPtr4", skip_serializing_if = "Option::is_none")]
    pub unknown_ptr4: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
}
fn read_fill_link(reader: &mut Reader<'_>, version: u16) -> Result<FillLink, ImportError> {
    let mut record = FillLink::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.unknown_array = reader.u32s(4)?;
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    record.unknown_ptr1 = reader.u32()?;
    record.unknown_ptr2 = reader.u32()?;
    record.unknown2 = reader.u32()?;
    if version >= 160 {
        record.unknown_ptr3 = Some(reader.u32()?);
        record.unknown_ptr4 = Some(reader.u32()?);
    }
    if version >= 174 {
        record.unknown3 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x1b (Net); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Net {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "NetName")]
    pub net_name: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Type")]
    pub r#type: u32,
    #[serde(rename = "Assignment")]
    pub assignment: u32,
    #[serde(rename = "Ratline")]
    pub ratline: u32,
    #[serde(rename = "FieldsPtr")]
    pub fields_ptr: u32,
    #[serde(rename = "MatchGroupPtr")]
    pub match_group_ptr: u32,
    #[serde(rename = "ModelPtr")]
    pub model_ptr: u32,
    #[serde(rename = "UnknownPtr4")]
    pub unknown_ptr4: u32,
    #[serde(rename = "UnknownPtr5")]
    pub unknown_ptr5: u32,
    #[serde(rename = "UnknownPtr6")]
    pub unknown_ptr6: u32,
}
fn read_net(reader: &mut Reader<'_>, version: u16) -> Result<Net, ImportError> {
    let mut record = Net::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.net_name = reader.u32()?;
    if version >= 160 {
        record.unknown1 = Some(reader.u32()?);
    }
    if version >= 172 {
        record.unknown2 = Some(reader.u32()?);
    }
    record.r#type = reader.u32()?;
    record.assignment = reader.u32()?;
    record.ratline = reader.u32()?;
    record.fields_ptr = reader.u32()?;
    record.match_group_ptr = reader.u32()?;
    record.model_ptr = reader.u32()?;
    record.unknown_ptr4 = reader.u32()?;
    record.unknown_ptr5 = reader.u32()?;
    record.unknown_ptr6 = reader.u32()?;
    Ok(record)
}
/// Source fields for 0x23 (Ratline); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Ratline {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Flags")]
    pub flags: Vec<u32>,
    #[serde(rename = "Ptr1")]
    pub ptr1: u32,
    #[serde(rename = "Ptr2")]
    pub ptr2: u32,
    #[serde(rename = "Ptr3")]
    pub ptr3: u32,
    #[serde(rename = "Coords")]
    pub coords: Vec<i32>,
    #[serde(rename = "Unknown1")]
    pub unknown1: Vec<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<Vec<u32>>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
}
fn read_ratline(reader: &mut Reader<'_>, version: u16) -> Result<Ratline, ImportError> {
    let mut record = Ratline {
        r#type: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        flags: reader.u32s(if version < 160 { 1 } else { 2 })?,
        ptr1: reader.u32()?,
        ptr2: reader.u32()?,
        ptr3: reader.u32()?,
        coords: reader.i32s(5)?,
        unknown1: reader.u32s(4)?,
        ..Default::default()
    };
    if version >= 164 {
        record.unknown2 = Some(reader.u32s(4)?);
    }
    if version >= 174 {
        record.unknown3 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x2e (Connection); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Connection {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "T2")]
    pub t2: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "NetAssignment")]
    pub net_assignment: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "CoordX")]
    pub coord_x: u32,
    #[serde(rename = "CoordY")]
    pub coord_y: u32,
    #[serde(rename = "Connection")]
    pub connection: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
}
fn read_connection(reader: &mut Reader<'_>, version: u16) -> Result<Connection, ImportError> {
    let mut record = Connection {
        r#type: reader.u8()?,
        t2: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        net_assignment: reader.u32()?,
        unknown1: reader.u32()?,
        coord_x: reader.u32()?,
        coord_y: reader.u32()?,
        connection: reader.u32()?,
        unknown2: reader.u32()?,
        ..Default::default()
    };
    if version >= 172 {
        record.unknown3 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x30 (TextWrapper); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct TextWrapper {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Unknown4", skip_serializing_if = "Option::is_none")]
    pub unknown4: Option<u32>,
    #[serde(rename = "Rotation")]
    pub rotation: u32,
    #[serde(rename = "Font16x", skip_serializing_if = "Option::is_none")]
    pub font16x: Option<u32>,
    #[serde(rename = "CoordsX")]
    pub coords_x: u32,
    #[serde(rename = "CoordsY")]
    pub coords_y: u32,
    #[serde(rename = "StrGraphicPtr")]
    pub str_graphic_ptr: u32,
    #[serde(rename = "PtrGroup_16x", skip_serializing_if = "Option::is_none")]
    pub ptr_group_16x: Option<u32>,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Font", skip_serializing_if = "Option::is_none")]
    pub font: Option<u32>,
    #[serde(rename = "Ptr1", skip_serializing_if = "Option::is_none")]
    pub ptr1: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "PtrGroup_17x", skip_serializing_if = "Option::is_none")]
    pub ptr_group_17x: Option<u32>,
    #[serde(rename = "Ptr2", skip_serializing_if = "Option::is_none")]
    pub ptr2: Option<u32>,
    #[serde(rename = "Unknown5", skip_serializing_if = "Option::is_none")]
    pub unknown5: Option<u32>,
}
fn read_text_wrapper(reader: &mut Reader<'_>, version: u16) -> Result<TextWrapper, ImportError> {
    let mut record = TextWrapper {
        r#type: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        ..Default::default()
    };
    if version < 160 {
        record.unknown4 = Some(reader.u32()?);
        record.rotation = reader.u32()?;
        record.font16x = Some(reader.u32()?);
        record.coords_x = reader.u32()?;
        record.coords_y = reader.u32()?;
        record.str_graphic_ptr = reader.u32()?;
        record.ptr_group_16x = Some(reader.u32()?);
        return Ok(record);
    }
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
        record.unknown2 = Some(reader.u32()?);
        record.font = Some(reader.u32()?);
        record.ptr1 = Some(reader.u32()?);
    }
    if version >= 174 {
        record.unknown3 = Some(reader.u32()?);
    }
    record.str_graphic_ptr = reader.u32()?;
    if version >= 172 {
        record.ptr_group_17x = Some(reader.u32()?);
    }
    if version < 172 {
        record.unknown4 = Some(reader.u32()?);
        record.font16x = Some(reader.u32()?);
    }
    if version >= 172 {
        record.ptr2 = Some(reader.u32()?);
    }
    record.coords_x = reader.u32()?;
    record.coords_y = reader.u32()?;
    record.unknown5 = Some(reader.u32()?);
    record.rotation = reader.u32()?;
    if version < 172 {
        record.ptr_group_16x = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x38 (Film); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Film {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "LayerList")]
    pub layer_list: u32,
    #[serde(rename = "FilmName", skip_serializing_if = "Option::is_none")]
    pub film_name: Option<String>,
    #[serde(rename = "LayerNameStr", skip_serializing_if = "Option::is_none")]
    pub layer_name_str: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "UnknownArray1")]
    pub unknown_array1: Vec<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
}
fn read_film(reader: &mut Reader<'_>, version: u16) -> Result<Film, ImportError> {
    let mut record = Film::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.layer_list = reader.u32()?;
    if version < 166 {
        record.film_name = Some(reader.fixed_string(20)?);
    }
    if version >= 166 {
        record.layer_name_str = Some(reader.u32()?);
        record.unknown2 = Some(reader.u32()?);
    }
    record.unknown_array1 = reader.u32s(7)?;
    if version >= 174 {
        record.unknown3 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x39 (FilmLayerList); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FilmLayerList {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Parent")]
    pub parent: u32,
    #[serde(rename = "Head")]
    pub head: u32,
    #[serde(rename = "X")]
    pub x: Vec<u16>,
}
fn read_film_layer_list(
    reader: &mut Reader<'_>,
    _version: u16,
) -> Result<FilmLayerList, ImportError> {
    let mut record = FilmLayerList::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.parent = reader.u32()?;
    record.head = reader.u32()?;
    record.x = reader.u16s(22)?;
    Ok(record)
}
/// Source fields for 0x3a (FilmListNode); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FilmListNode {
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Unknown")]
    pub unknown: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
}
fn read_film_list_node(reader: &mut Reader<'_>, version: u16) -> Result<FilmListNode, ImportError> {
    let mut record = FilmListNode::default();
    reader.skip(1)?;
    record.layer = reader.u16()?;
    record.key = RecordKey(reader.u32()?);
    record.next = reader.u32()?;
    record.unknown = reader.u32()?;
    if version >= 174 {
        record.unknown1 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x0a (DesignRuleCheck); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct DesignRuleCheck {
    #[serde(rename = "T")]
    pub t: u16,
    #[serde(rename = "Layer")]
    pub layer: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Coords")]
    pub coords: Vec<i32>,
    #[serde(rename = "Unknown4")]
    pub unknown4: Vec<u32>,
    #[serde(rename = "Unknown5")]
    pub unknown5: Vec<u32>,
    #[serde(rename = "Unknown6", skip_serializing_if = "Option::is_none")]
    pub unknown6: Option<u32>,
}
fn read_design_rule_check(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<DesignRuleCheck, ImportError> {
    let mut record = DesignRuleCheck {
        t: reader.u8()?,
        layer: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        ..Default::default()
    };
    if version >= 160 {
        record.unknown1 = Some(reader.u32()?);
    }
    if version >= 172 {
        record.unknown2 = Some(reader.u32()?);
    }
    record.coords = reader.i32s(4)?;
    record.unknown4 = reader.u32s(4)?;
    record.unknown5 = reader.u32s(5)?;
    if version >= 174 {
        record.unknown6 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x12 (CrossReference); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct CrossReference {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "R")]
    pub r: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Ptr1")]
    pub ptr1: u32,
    #[serde(rename = "Ptr2")]
    pub ptr2: u32,
    #[serde(rename = "Ptr3")]
    pub ptr3: u32,
    #[serde(rename = "Unknown1")]
    pub unknown1: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
}
fn read_cross_reference(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<CrossReference, ImportError> {
    let mut record = CrossReference {
        r#type: reader.u8()?,
        r: reader.u16()?,
        key: RecordKey(reader.u32()?),
        ptr1: reader.u32()?,
        ptr2: reader.u32()?,
        ptr3: reader.u32()?,
        unknown1: reader.u32()?,
        ..Default::default()
    };
    if version >= 165 {
        record.unknown2 = Some(reader.u32()?);
    }
    if version >= 174 {
        record.unknown3 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x20 (UnknownRecord0x20); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct UnknownRecord0x20 {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "R")]
    pub r: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "UnknownArray1")]
    pub unknown_array1: Vec<u32>,
    #[serde(rename = "UnknownArray2", skip_serializing_if = "Option::is_none")]
    pub unknown_array2: Option<Vec<u32>>,
}
fn read_unknown_record0x20(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<UnknownRecord0x20, ImportError> {
    let mut record = UnknownRecord0x20 {
        r#type: reader.u8()?,
        r: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        unknown_array1: reader.u32s(7)?,
        ..Default::default()
    };
    if version >= 174 {
        record.unknown_array2 = Some(reader.u32s(10)?);
    }
    Ok(record)
}
/// Source fields for 0x22 (UnknownRecord0x22); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct UnknownRecord0x22 {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "T2")]
    pub t2: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "UnknownArray")]
    pub unknown_array: Vec<u32>,
}
fn read_unknown_record0x22(
    reader: &mut Reader<'_>,
    version: u16,
) -> Result<UnknownRecord0x22, ImportError> {
    let mut record = UnknownRecord0x22 {
        r#type: reader.u8()?,
        t2: reader.u16()?,
        key: RecordKey(reader.u32()?),
        ..Default::default()
    };
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    record.unknown_array = reader.u32s(8)?;
    Ok(record)
}
/// Source fields for 0x26 (MatchGroup); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct MatchGroup {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "R")]
    pub r: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "MemberPtr")]
    pub member_ptr: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "GroupPtr")]
    pub group_ptr: u32,
    #[serde(rename = "ConstPtr")]
    pub const_ptr: u32,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
}
fn read_match_group(reader: &mut Reader<'_>, version: u16) -> Result<MatchGroup, ImportError> {
    let mut record = MatchGroup {
        r#type: reader.u8()?,
        r: reader.u16()?,
        key: RecordKey(reader.u32()?),
        member_ptr: reader.u32()?,
        ..Default::default()
    };
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
    }
    record.group_ptr = reader.u32()?;
    record.const_ptr = reader.u32()?;
    if version >= 174 {
        record.unknown2 = Some(reader.u32()?);
    }
    Ok(record)
}
/// Source fields for 0x2c (Table); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct Table {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "SubType")]
    pub sub_type: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Unknown1", skip_serializing_if = "Option::is_none")]
    pub unknown1: Option<u32>,
    #[serde(rename = "Unknown2", skip_serializing_if = "Option::is_none")]
    pub unknown2: Option<u32>,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "StringPtr")]
    pub string_ptr: u32,
    #[serde(rename = "Unknown4", skip_serializing_if = "Option::is_none")]
    pub unknown4: Option<u32>,
    #[serde(rename = "Ptr1")]
    pub ptr1: u32,
    #[serde(rename = "Ptr2")]
    pub ptr2: u32,
    #[serde(rename = "Ptr3")]
    pub ptr3: u32,
    #[serde(rename = "Flags")]
    pub flags: u32,
}
fn read_table(reader: &mut Reader<'_>, version: u16) -> Result<Table, ImportError> {
    let mut record = Table {
        r#type: reader.u8()?,
        sub_type: reader.u16()?,
        key: RecordKey(reader.u32()?),
        next: reader.u32()?,
        ..Default::default()
    };
    if version >= 172 {
        record.unknown1 = Some(reader.u32()?);
        record.unknown2 = Some(reader.u32()?);
        record.unknown3 = Some(reader.u32()?);
    }
    record.string_ptr = reader.u32()?;
    if (160..172).contains(&version) {
        record.unknown4 = Some(reader.u32()?);
    }
    record.ptr1 = reader.u32()?;
    record.ptr2 = reader.u32()?;
    record.ptr3 = reader.u32()?;
    record.flags = if version < 160 { 0 } else { reader.u32()? };
    Ok(record)
}
/// Source fields for 0x2f (UnknownRecord0x2f); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct UnknownRecord0x2f {
    #[serde(rename = "Type")]
    pub r#type: u16,
    #[serde(rename = "T2")]
    pub t2: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "UnknownArray")]
    pub unknown_array: Vec<u32>,
}
fn read_unknown_record0x2f(
    reader: &mut Reader<'_>,
    _version: u16,
) -> Result<UnknownRecord0x2f, ImportError> {
    let record = UnknownRecord0x2f {
        r#type: reader.u8()?,
        t2: reader.u16()?,
        key: RecordKey(reader.u32()?),
        unknown_array: reader.u32s(6)?,
    };
    Ok(record)
}
/// Source fields for 0x35 (FileReference); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct FileReference {
    #[serde(rename = "T2")]
    pub t2: u16,
    #[serde(rename = "T3")]
    pub t3: u16,
}
fn read_file_reference(
    reader: &mut Reader<'_>,
    _version: u16,
) -> Result<FileReference, ImportError> {
    let record = FileReference {
        t2: reader.u8()?,
        t3: reader.u16()?,
    };
    reader.skip(120)?;
    Ok(record)
}
/// Source fields for 0x37 (PointerArray); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct PointerArray {
    #[serde(rename = "T")]
    pub t: u16,
    #[serde(rename = "T2")]
    pub t2: u16,
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "GroupPtr")]
    pub group_ptr: u32,
    #[serde(rename = "Next")]
    pub next: u32,
    #[serde(rename = "Capacity")]
    pub capacity: u32,
    #[serde(rename = "Count")]
    pub count: u32,
    #[serde(rename = "Unknown2")]
    pub unknown2: u32,
    #[serde(rename = "Unknown3", skip_serializing_if = "Option::is_none")]
    pub unknown3: Option<u32>,
    #[serde(rename = "Ptrs")]
    pub ptrs: Vec<u32>,
}
fn read_pointer_array(reader: &mut Reader<'_>, version: u16) -> Result<PointerArray, ImportError> {
    let mut record = PointerArray {
        t: reader.u8()?,
        t2: reader.u16()?,
        key: RecordKey(reader.u32()?),
        group_ptr: reader.u32()?,
        next: reader.u32()?,
        capacity: reader.u32()?,
        count: reader.u32()?,
        unknown2: reader.u32()?,
        ..Default::default()
    };
    if version >= 174 {
        record.unknown3 = Some(reader.u32()?);
    }
    record.ptrs = reader.u32s(100)?;
    Ok(record)
}
/// Source fields for 0x3e (OrderedKeyList); units remain in Allegro source coordinates.
#[derive(Debug, Default, Serialize)]
pub struct OrderedKeyList {
    #[serde(rename = "Key")]
    pub key: RecordKey,
    #[serde(rename = "Unknown")]
    pub unknown: Vec<u32>,
}
fn read_ordered_key_list(
    reader: &mut Reader<'_>,
    _version: u16,
) -> Result<OrderedKeyList, ImportError> {
    let mut record = OrderedKeyList::default();
    reader.skip(3)?;
    record.key = RecordKey(reader.u32()?);
    record.unknown = reader.u32s(9)?;
    Ok(record)
}
/// One fixed record, decoded on demand. The selected variant owns only this record.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FixedRecord {
    Arc(Arc),
    FootprintRectangle(FootprintRectangle),
    Graphic(Graphic),
    Segment(Segment),
    Rectangle(Rectangle),
    Shape(Shape),
    Keepout(Keepout),
    Component(Component),
    ComponentInstance(ComponentInstance),
    FunctionSlot(FunctionSlot),
    FunctionInstance(FunctionInstance),
    FootprintDefinition(FootprintDefinition),
    FootprintInstance(FootprintInstance),
    PinNumber(PinNumber),
    PinDefinition(PinDefinition),
    Pad(Pad),
    PinName(PinName),
    Pin(Pin),
    PlacedPad(PlacedPad),
    Via(Via),
    NetAssignment(NetAssignment),
    Track(Track),
    FillLink(FillLink),
    Net(Net),
    Ratline(Ratline),
    Connection(Connection),
    TextWrapper(TextWrapper),
    Film(Film),
    FilmLayerList(FilmLayerList),
    FilmListNode(FilmListNode),
    DesignRuleCheck(DesignRuleCheck),
    CrossReference(CrossReference),
    UnknownRecord0x20(UnknownRecord0x20),
    UnknownRecord0x22(UnknownRecord0x22),
    MatchGroup(MatchGroup),
    Table(Table),
    UnknownRecord0x2f(UnknownRecord0x2f),
    FileReference(FileReference),
    PointerArray(PointerArray),
    OrderedKeyList(OrderedKeyList),
}
impl FixedRecord {
    /// Source `Next` only; footprint and component lists have separate link fields.
    pub fn next_key(&self) -> Option<RecordKey> {
        let next = match self {
            Self::Arc(r) => r.next,
            Self::FootprintRectangle(r) => r.next,
            Self::Graphic(r) => r.next,
            Self::Segment(r) => r.next,
            Self::Rectangle(r) => r.next,
            Self::Shape(r) => r.next,
            Self::Keepout(r) => r.next,
            Self::Component(r) => r.next,
            Self::ComponentInstance(r) => r.next,
            Self::FunctionSlot(r) => return r.next.map(RecordKey),
            Self::FootprintDefinition(r) => r.next,
            Self::FootprintInstance(r) => r.next,
            Self::PinNumber(r) => r.next,
            Self::PinDefinition(r) => r.next,
            Self::Pad(r) => r.next,
            Self::PinName(r) => r.next,
            Self::PlacedPad(r) => r.next,
            Self::Via(r) => r.next,
            Self::NetAssignment(r) => r.next,
            Self::Track(r) => r.next,
            Self::Net(r) => r.next,
            Self::Ratline(r) => r.next,
            Self::Connection(r) => r.next,
            Self::TextWrapper(r) => r.next,
            Self::Film(r) => r.next,
            Self::FilmListNode(r) => r.next,
            Self::DesignRuleCheck(r) => r.next,
            Self::UnknownRecord0x20(r) => r.next,
            Self::Table(r) => r.next,
            Self::PointerArray(r) => r.next,
            Self::FunctionInstance(_)
            | Self::Pin(_)
            | Self::FillLink(_)
            | Self::FilmLayerList(_)
            | Self::CrossReference(_)
            | Self::UnknownRecord0x22(_)
            | Self::MatchGroup(_)
            | Self::UnknownRecord0x2f(_)
            | Self::FileReference(_)
            | Self::OrderedKeyList(_) => return None,
        };
        Some(RecordKey(next))
    }

    pub fn key(&self) -> RecordKey {
        match self {
            Self::Arc(record) => record.key,
            Self::FootprintRectangle(record) => record.key,
            Self::Graphic(record) => record.key,
            Self::Segment(record) => record.key,
            Self::Rectangle(record) => record.key,
            Self::Shape(record) => record.key,
            Self::Keepout(record) => record.key,
            Self::Component(record) => record.key,
            Self::ComponentInstance(record) => record.key,
            Self::FunctionSlot(record) => record.key,
            Self::FunctionInstance(record) => record.key,
            Self::FootprintDefinition(record) => record.key,
            Self::FootprintInstance(record) => record.key,
            Self::PinNumber(record) => record.key,
            Self::PinDefinition(record) => record.key,
            Self::Pad(record) => record.key,
            Self::PinName(record) => record.key,
            Self::Pin(record) => record.key,
            Self::PlacedPad(record) => record.key,
            Self::Via(record) => record.key,
            Self::NetAssignment(record) => record.key,
            Self::Track(record) => record.key,
            Self::FillLink(record) => record.key,
            Self::Net(record) => record.key,
            Self::Ratline(record) => record.key,
            Self::Connection(record) => record.key,
            Self::TextWrapper(record) => record.key,
            Self::Film(record) => record.key,
            Self::FilmLayerList(record) => record.key,
            Self::FilmListNode(record) => record.key,
            Self::DesignRuleCheck(record) => record.key,
            Self::CrossReference(record) => record.key,
            Self::UnknownRecord0x20(record) => record.key,
            Self::UnknownRecord0x22(record) => record.key,
            Self::MatchGroup(record) => record.key,
            Self::Table(record) => record.key,
            Self::UnknownRecord0x2f(record) => record.key,
            Self::FileReference(_) => RecordKey(0),
            Self::PointerArray(record) => record.key,
            Self::OrderedKeyList(record) => record.key,
        }
    }
}
pub(super) fn read(
    reader: &mut Reader<'_>,
    kind: u8,
    version: u16,
) -> Result<FixedRecord, ImportError> {
    Ok(match kind {
        0x01 => FixedRecord::Arc(read_arc(reader, version)?),
        0x0e => FixedRecord::FootprintRectangle(read_footprint_rectangle(reader, version)?),
        0x14 => FixedRecord::Graphic(read_graphic(reader, version)?),
        0x15..=0x17 => FixedRecord::Segment(read_segment(reader, version)?),
        0x24 => FixedRecord::Rectangle(read_rectangle(reader, version)?),
        0x28 => FixedRecord::Shape(read_shape(reader, version)?),
        0x34 => FixedRecord::Keepout(read_keepout(reader, version)?),
        0x06 => FixedRecord::Component(read_component(reader, version)?),
        0x07 => FixedRecord::ComponentInstance(read_component_instance(reader, version)?),
        0x0f => FixedRecord::FunctionSlot(read_function_slot(reader, version)?),
        0x10 => FixedRecord::FunctionInstance(read_function_instance(reader, version)?),
        0x2b => FixedRecord::FootprintDefinition(read_footprint_definition(reader, version)?),
        0x2d => {
            let mut record = read_footprint_instance(reader, version)?;
            if version < 172 {
                record.inst_ref = record.inst_ref16x;
            }
            FixedRecord::FootprintInstance(record)
        }
        0x08 => FixedRecord::PinNumber(read_pin_number(reader, version)?),
        0x0c => FixedRecord::PinDefinition(read_pin_definition(reader, version)?),
        0x0d => FixedRecord::Pad(read_pad(reader, version)?),
        0x11 => FixedRecord::PinName(read_pin_name(reader, version)?),
        0x29 => FixedRecord::Pin(read_pin(reader, version)?),
        0x32 => FixedRecord::PlacedPad(read_placed_pad(reader, version)?),
        0x33 => FixedRecord::Via(read_via(reader, version)?),
        0x04 => FixedRecord::NetAssignment(read_net_assignment(reader, version)?),
        0x05 => FixedRecord::Track(read_track(reader, version)?),
        0x09 => FixedRecord::FillLink(read_fill_link(reader, version)?),
        0x1b => FixedRecord::Net(read_net(reader, version)?),
        0x23 => FixedRecord::Ratline(read_ratline(reader, version)?),
        0x2e => FixedRecord::Connection(read_connection(reader, version)?),
        0x30 => FixedRecord::TextWrapper(read_text_wrapper(reader, version)?),
        0x38 => FixedRecord::Film(read_film(reader, version)?),
        0x39 => FixedRecord::FilmLayerList(read_film_layer_list(reader, version)?),
        0x3a => FixedRecord::FilmListNode(read_film_list_node(reader, version)?),
        0x0a => FixedRecord::DesignRuleCheck(read_design_rule_check(reader, version)?),
        0x12 => FixedRecord::CrossReference(read_cross_reference(reader, version)?),
        0x20 => FixedRecord::UnknownRecord0x20(read_unknown_record0x20(reader, version)?),
        0x22 => FixedRecord::UnknownRecord0x22(read_unknown_record0x22(reader, version)?),
        0x26 => FixedRecord::MatchGroup(read_match_group(reader, version)?),
        0x2c => FixedRecord::Table(read_table(reader, version)?),
        0x2f => FixedRecord::UnknownRecord0x2f(read_unknown_record0x2f(reader, version)?),
        0x35 => FixedRecord::FileReference(read_file_reference(reader, version)?),
        0x37 => FixedRecord::PointerArray(read_pointer_array(reader, version)?),
        0x3e => FixedRecord::OrderedKeyList(read_ordered_key_list(reader, version)?),
        _ => {
            return Err(ImportError::UnsupportedRecordLayout {
                record_type: kind,
                version,
                offset: reader.offset() - 1,
            });
        }
    })
}
pub fn supports(kind: u8) -> bool {
    matches!(
        kind,
        0x01 | 0x0e | 0x14 | 0x15
            ..=0x17
                | 0x24
                | 0x28
                | 0x34
                | 0x06
                | 0x07
                | 0x0f
                | 0x10
                | 0x2b
                | 0x2d
                | 0x08
                | 0x0c
                | 0x0d
                | 0x11
                | 0x29
                | 0x32
                | 0x33
                | 0x04
                | 0x05
                | 0x09
                | 0x1b
                | 0x23
                | 0x2e
                | 0x30
                | 0x38
                | 0x39
                | 0x3a
                | 0x0a
                | 0x12
                | 0x20
                | 0x22
                | 0x26
                | 0x2c
                | 0x2f
                | 0x35
                | 0x37
                | 0x3e
    )
}
