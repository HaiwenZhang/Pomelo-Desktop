//! Physical layer metadata follows the 0x2a flags, never inferred from names.

use super::super::{
    database::{BrdDatabase, ReferenceLocation},
    decoder::{
        DecodedRecord,
        variable::{LayerEntry, VariableRecord},
    },
    index::{FileOffset, RecordKey},
};
use crate::{ImportContext, ImportError};
use pomelo_core::model::{Layer, LayerFunction, LayerId};
use pomelo_core::{
    i18n::{Message, MessageKey as Key},
    model::DrawingLayer,
};

/// Reserved subclasses keep their localized standard labels, even when the file supplies a name.
pub fn drawing_layer(raw: u16, custom_name: String) -> DrawingLayer {
    let class_id = raw as u8;
    let subclass = (raw >> 8) as u8;
    let class_label = match class_id {
        1 => Key::DrawingClassBoard,
        2 => Key::DrawingClassValue,
        3 => Key::DrawingClassType,
        4 => Key::DrawingClassFrame,
        7 => Key::DrawingClassManufacturing,
        9 => Key::DrawingClassPackage,
        13 => Key::DrawingClassReference,
        16 => Key::DrawingClassTolerance,
        17 => Key::DrawingClassPartNumber,
        _ => Key::DrawingClassUnknown,
    };
    let (label, top, bottom) = match class_id {
        2 | 3 | 13 | 16 | 17 => (
            match subclass {
                248 => Key::DrawingBottomDisplay,
                249 => Key::DrawingTopDisplay,
                250 => Key::DrawingBottomSilk,
                251 => Key::DrawingTopSilk,
                252 => Key::DrawingBottomAssembly,
                253 => Key::DrawingTopAssembly,
                _ => Key::DrawingSubclassUnknown,
            },
            subclass == 251,
            subclass == 250,
        ),
        1 => (
            match subclass {
                240 => Key::DrawingBottomSilk,
                241 => Key::DrawingTopSilk,
                237 => Key::DrawingBottomMask,
                238 => Key::DrawingTopMask,
                249 => Key::DrawingDimension,
                251 => Key::DrawingAssemblyNotes,
                252 => Key::DrawingPlatingBar,
                _ => Key::DrawingSubclassUnknown,
            },
            subclass == 241,
            subclass == 240,
        ),
        9 => (
            match subclass {
                241 => Key::DrawingBottomDisplay,
                242 => Key::DrawingTopDisplay,
                243 => Key::DrawingBottomMask,
                244 => Key::DrawingTopMask,
                245 => Key::DrawingComponentCenter,
                246 => Key::DrawingBottomSilk,
                247 => Key::DrawingTopSilk,
                248 => Key::DrawingPadstackName,
                249 => Key::DrawingPinNumber,
                250 => Key::DrawingBottomPlacement,
                251 => Key::DrawingTopPlacement,
                252 => Key::DrawingBottomAssembly,
                253 => Key::DrawingTopAssembly,
                _ => Key::DrawingSubclassUnknown,
            },
            subclass == 247,
            subclass == 246,
        ),
        _ => (Key::DrawingSubclassUnknown, false, false),
    };
    let mut class_label = Message::new(class_label);
    if class_label.key == Key::DrawingClassUnknown {
        class_label = class_label.arg("class", class_id as u32);
    }
    let mut subclass_label = Message::new(label);
    if label == Key::DrawingSubclassUnknown {
        subclass_label = subclass_label.arg("subclass", subclass as u32);
    }
    DrawingLayer {
        id: LayerId(0x10000 + u32::from(raw)),
        class_id,
        subclass,
        source_name: if subclass < 0xe0 {
            custom_name
        } else {
            String::new()
        },
        color: (if top {
            "#dae6d9"
        } else if bottom {
            "#c69adc"
        } else {
            "#a7a9bd"
        })
        .to_owned(),
        default_visible: top,
        class_label,
        subclass_label,
    }
}

pub(super) fn drawing_layer_from_source(
    database: &BrdDatabase,
    raw: u16,
    context: &ImportContext<'_>,
) -> Result<DrawingLayer, ImportError> {
    let key = database
        .header()
        .layer_map
        .get((raw & 255) as usize)
        .map_or(0, |entry| entry.record_id);
    let mut name = String::new();
    if key != 0
        && let Some(record) = database.get(RecordKey(key), context)?
        && let DecodedRecord::Variable(VariableRecord::LayerList(list)) = record.fields
        && let Some(entry) = list.entries.into_iter().nth((raw >> 8) as usize)
    {
        name = match entry {
            LayerEntry::Inline { name } => name,
            LayerEntry::Reference { name_id, .. } => {
                database.string(name_id).unwrap_or_default().to_owned()
            }
        };
    }
    Ok(drawing_layer(raw, name))
}

const COLORS: [&str; 8] = [
    "#58b5ed", "#83ce94", "#edb963", "#ba8bec", "#eb819d", "#54c7bd", "#a5b8df", "#e18d61",
];

pub fn function_from_flags(properties: Option<u32>) -> LayerFunction {
    match properties.map(|flags| flags & 0xc100) {
        Some(0x8000) => LayerFunction::Conductor,
        Some(0x100) => LayerFunction::Plane,
        Some(0x4000) => LayerFunction::Dielectric,
        _ => LayerFunction::Unknown,
    }
}

/// Original layer names are source data. Missing names are localized by Layer::display_name.
pub fn read_layers(
    database: &BrdDatabase,
    context: &ImportContext<'_>,
) -> Result<Vec<Layer>, ImportError> {
    let key = database
        .header()
        .layer_map
        .get(6)
        .map_or(0, |entry| entry.record_id);
    let map_offset = if database.header().version < 160 {
        0x470
    } else {
        0x428
    };
    let record = database.require_record(
        RecordKey(key),
        &[0x2a],
        ReferenceLocation {
            offset: FileOffset(map_offset + 6 * 8 + 4),
            field: "LayerMap[6].RecordId",
        },
        context,
    )?;
    let DecodedRecord::Variable(VariableRecord::LayerList(list)) = record.fields else {
        return Err(ImportError::InvalidRecord {
            offset: record.span.offset.0 as usize,
            field: "LAYER_LIST",
            value: u64::from(key),
        });
    };
    let mut result = Vec::with_capacity(list.entries.len());
    for (index, entry) in list.entries.into_iter().enumerate() {
        context.check_cancelled()?;
        let (name, source_flags) = match entry {
            LayerEntry::Inline { name } => (name, None),
            LayerEntry::Reference {
                name_id,
                properties,
                ..
            } => (
                database.string(name_id).unwrap_or_default().to_owned(),
                Some(properties),
            ),
        };
        result.push(Layer {
            id: LayerId(index as u32),
            name,
            function: function_from_flags(source_flags),
            color: COLORS[index % COLORS.len()].to_owned(),
            source_flags,
        });
    }
    Ok(result)
}
