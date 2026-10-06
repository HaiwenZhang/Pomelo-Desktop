//! Placed board text. Library text and DRC are excluded; source coordinates are already absolute.

use std::{collections::HashMap, mem::size_of};

use super::{AnnotationChain, CacheBudget, CacheLimits, annotation_warning, layers, units};
use crate::{
    ImportContext, ImportError,
    formats::allegro::{
        database::{BrdDatabase, ChainLimits, ReferenceLocation},
        decoder::{
            DecodedRecord,
            fixed::FixedRecord,
            variable::{FontDefinition, VariableRecord},
        },
        index::{FileOffset, RecordKey},
    },
};
use pomelo_core::{
    i18n::{Message, MessageKey as Key},
    model::{BoardText, Diagnostic, DrawingLayer, LayerId, ObjectId, Point, TextAlignment},
};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct TextLimits {
    pub objects: CacheLimits,
    pub chain: ChainLimits,
}
impl Default for TextLimits {
    fn default() -> Self {
        Self {
            objects: CacheLimits {
                max_bytes: 512 * 1024 * 1024,
                max_entries: 4_000_000,
            },
            chain: ChainLimits::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlacedTexts {
    pub texts: Vec<BoardText>,
    pub drawing_layers: Vec<DrawingLayer>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct TextBuilder<'a> {
    database: &'a BrdDatabase,
    limits: TextLimits,
    budget: CacheBudget,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> TextBuilder<'a> {
    pub fn new(database: &'a BrdDatabase, limits: TextLimits) -> Self {
        Self {
            database,
            budget: CacheBudget::new(limits.objects.clone()),
            limits,
            diagnostics: Vec::new(),
        }
    }

    /// Traverse only header/placed-footprint chains, preserving first-seen order and last owner.
    pub fn build(mut self, context: &ImportContext<'_>) -> Result<PlacedTexts, ImportError> {
        let database = self.database;
        let scale =
            units::millimetres_per_unit(database.header().units, database.header().divisor)?;
        let mut fonts = Vec::new();
        let mut tables = 0;
        for record in database.records_of_type(0x36, context) {
            let record = record?;
            if let DecodedRecord::Variable(VariableRecord::DefinitionTable(table)) = record.fields
                && table.code == 8
            {
                tables += 1;
                if tables == 1 {
                    if let Some(source_fonts) = table.fonts {
                        let bytes = source_fonts
                            .capacity()
                            .saturating_mul(size_of::<FontDefinition>());
                        self.budget.check(bytes, record.span.offset.0 as usize)?;
                        self.budget.commit(bytes);
                        fonts = source_fonts;
                    } else {
                        self.warn(
                            "BRD_TEXT_FONT_TABLE_INVALID",
                            Message::new(Key::TextFontTableInvalid),
                            record.span.key.0,
                            record.span.offset.0 as usize,
                        )?;
                    }
                }
            }
        }
        if tables > 1 {
            self.warn(
                "BRD_TEXT_FONT_TABLES_MULTIPLE",
                Message::new(Key::TextFontTablesMultiple),
                0,
                0,
            )?;
        }
        // Retain keys and ownership only, not a full decoded record graph.
        let mut wrappers = Vec::new();
        let mut positions = HashMap::new();
        let board = &database.header().text_list;
        self.walk(
            board.head,
            board.tail,
            None,
            FileOffset(if database.header().version >= 180 {
                0xb4
            } else {
                0x90
            }),
            &mut wrappers,
            &mut positions,
            context,
        )?;
        for record in database.records_of_type(0x2d, context) {
            let record = record?;
            if let DecodedRecord::Fixed(FixedRecord::FootprintInstance(fp)) = record.fields {
                self.walk(
                    fp.text_ptr,
                    fp.key.0,
                    Some(ObjectId(fp.key.0)),
                    record.span.offset,
                    &mut wrappers,
                    &mut positions,
                    context,
                )?;
            }
        }
        let mut texts = Vec::new();
        let mut drawing_layers = Vec::new();
        for (key, owner_id) in wrappers {
            context.check_cancelled()?;
            let record = database.require_record(
                RecordKey(key),
                &[0x30],
                ReferenceLocation {
                    offset: FileOffset(0),
                    field: "TextWrapper",
                },
                context,
            )?;
            let offset = record.span.offset.0 as usize;
            let DecodedRecord::Fixed(FixedRecord::TextWrapper(wrapper)) = record.fields else {
                continue;
            };
            let graphic = database.get(RecordKey(wrapper.str_graphic_ptr), context)?;
            let Some(DecodedRecord::Variable(VariableRecord::TextGraphic(graphic))) =
                graphic.map(|r| r.fields)
            else {
                self.warn(
                    "BRD_TEXT_CONTENT_MISSING",
                    Message::new(Key::TextContentMissing).arg("key", key),
                    key,
                    offset,
                )?;
                continue;
            };
            let props = wrapper.font.or(wrapper.font16x).unwrap_or(0);
            let font_index = props as u8;
            let font = font_index
                .checked_sub(1)
                .and_then(|i| fonts.get(i as usize));
            let Some(font) = font.filter(|f| {
                [
                    f.height,
                    f.width,
                    f.character_space,
                    f.line_space,
                    f.stroke_width,
                ]
                .iter()
                .all(|&v| v == 0)
                    || (f.height > 0 && f.width > 0)
            }) else {
                self.warn(
                    "BRD_TEXT_FONT_INVALID",
                    Message::new(Key::TextFontInvalid)
                        .arg("key", key)
                        .arg("font", font_index as u32),
                    key,
                    offset,
                )?;
                continue;
            };
            let class_id = wrapper.layer as u8;
            if class_id == 5 {
                continue;
            }
            let subclass = (wrapper.layer >> 8) as u8;
            let layer = LayerId(if class_id == 6 {
                subclass as u32
            } else {
                0x10000 + u32::from(wrapper.layer)
            });
            let bytes = size_of::<BoardText>()
                .saturating_mul(2)
                .saturating_add(graphic.value.capacity());
            self.budget.check(bytes, offset)?;
            self.budget.commit(bytes);
            texts.push(BoardText {
                id: ObjectId(key),
                owner_id,
                layer,
                class_id,
                subclass,
                text: graphic.value,
                at: Point::new(
                    f64::from(wrapper.coords_x as i32) * scale,
                    f64::from(wrapper.coords_y as i32) * scale,
                ),
                angle: f64::from(wrapper.rotation) * std::f64::consts::PI / 180000.0,
                mirrored: matches!(props >> 24, 1 | 3),
                align: match (props >> 16) & 255 {
                    2 => TextAlignment::Right,
                    3 => TextAlignment::Center,
                    _ => TextAlignment::Left,
                },
                font_index,
                width: f64::from(font.width) * scale,
                height: f64::from(font.height) * scale,
                spacing: f64::from(font.character_space) * scale,
                line_spacing: f64::from(font.line_space) * scale,
                stroke_width: f64::from(font.stroke_width) * scale,
            });
            if class_id != 6 && !drawing_layers.iter().any(|l: &DrawingLayer| l.id == layer) {
                let layer = layers::drawing_layer_from_source(database, wrapper.layer, context)?;
                let bytes = 2048usize.saturating_add(layer.source_name.capacity());
                self.budget.check(bytes, offset)?;
                self.budget.commit(bytes);
                drawing_layers.push(layer);
            }
        }
        drawing_layers.sort_by_key(|l| (!l.default_visible, l.id));
        context.check_cancelled()?;
        Ok(PlacedTexts {
            texts,
            drawing_layers,
            diagnostics: self.diagnostics,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Each source chain specifies its own ownership and reference origin"
    )]
    fn walk(
        &mut self,
        head: u32,
        tail: u32,
        owner: Option<ObjectId>,
        offset: FileOffset,
        wrappers: &mut Vec<(u32, Option<ObjectId>)>,
        positions: &mut HashMap<u32, usize>,
        context: &ImportContext<'_>,
    ) -> Result<(), ImportError> {
        let database = self.database;
        let limits = self.limits.chain.clone();
        let mut chain = AnnotationChain::new(&limits);
        let mut key = RecordKey(head);
        let mut origin = ReferenceLocation {
            offset,
            field: if owner.is_some() {
                "TextPtr"
            } else {
                "Header.TextList.Head"
            },
        };
        while key.0 != 0 && key.0 != tail && !database.header().sentinel_keys.contains(&key.0) {
            chain.visit(key, origin, context)?;
            let Some(record) = database.get(key, context)? else {
                self.warn(
                    "BRD_TEXT_LINK_MISSING",
                    Message::new(Key::TextLinkMissing).arg("key", key.0),
                    key.0,
                    origin.offset.0 as usize,
                )?;
                break;
            };
            let next = record.fields.next_key().unwrap_or(RecordKey(0));
            match record.fields {
                DecodedRecord::Fixed(FixedRecord::TextWrapper(_)) => {
                    if let Some(&index) = positions.get(&key.0) {
                        if owner.is_some() {
                            wrappers[index].1 = owner;
                        }
                    } else {
                        self.budget.check(256, record.span.offset.0 as usize)?;
                        self.budget.commit(256);
                        positions.insert(key.0, wrappers.len());
                        wrappers.push((key.0, owner));
                    }
                }
                _ if record.span.record_type == 3 => {}
                _ => {
                    self.warn(
                        "BRD_TEXT_LINK_TYPE",
                        Message::new(Key::TextLinkType)
                            .arg("key", key.0)
                            .arg("record_type", record.span.record_type as u32),
                        key.0,
                        record.span.offset.0 as usize,
                    )?;
                    break;
                }
            }
            origin = ReferenceLocation {
                offset: record.span.offset,
                field: "Next",
            };
            key = next;
        }
        Ok(())
    }

    fn warn(
        &mut self,
        code: &'static str,
        message: Message,
        key: u32,
        offset: usize,
    ) -> Result<(), ImportError> {
        annotation_warning(
            &mut self.budget,
            &mut self.diagnostics,
            code,
            message,
            key,
            offset,
        )
    }
}
