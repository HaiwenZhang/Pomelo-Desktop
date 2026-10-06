//! Stored dimension graphics, grouped by board graphic or placed footprint without extra transforms.

use super::{
    AnnotationChain, CacheBudget, CacheLimits, annotation_warning, geometry::decode_edge, units,
};
use crate::{
    ImportContext, ImportError,
    formats::allegro::{
        database::{BrdDatabase, ChainLimits, ReferenceLocation},
        decoder::{DecodedRecord, fixed::FixedRecord},
        index::{FileOffset, RecordKey},
    },
};
use pomelo_core::{
    i18n::{Message, MessageKey as Key},
    model::{BoardDrawing, BoardText, Diagnostic, LayerId, NetId, ObjectId, Segment},
};
use serde::Serialize;
use std::{collections::HashMap, mem::size_of};

#[derive(Debug, Clone)]
pub struct DrawingLimits {
    pub objects: CacheLimits,
    pub chain: ChainLimits,
}
impl Default for DrawingLimits {
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
pub struct StoredDrawings {
    pub drawings: Vec<BoardDrawing>,
    pub diagnostics: Vec<Diagnostic>,
}
pub struct DrawingBuilder<'a> {
    database: &'a BrdDatabase,
    limits: DrawingLimits,
    budget: CacheBudget,
    diagnostics: Vec<Diagnostic>,
    groups: Vec<BoardDrawing>,
    group_positions: HashMap<u32, usize>,
}
impl<'a> DrawingBuilder<'a> {
    pub fn new(database: &'a BrdDatabase, limits: DrawingLimits) -> Self {
        Self {
            database,
            budget: CacheBudget::new(limits.objects.clone()),
            limits,
            diagnostics: Vec::new(),
            groups: Vec::new(),
            group_positions: HashMap::new(),
        }
    }
    pub fn build(
        mut self,
        texts: &[BoardText],
        context: &ImportContext<'_>,
    ) -> Result<StoredDrawings, ImportError> {
        let database = self.database;
        let scale =
            units::millimetres_per_unit(database.header().units, database.header().divisor)?;
        let mut candidates = Vec::new();
        let mut selected = HashMap::new();
        let mut owners = Vec::new();
        let mut owner_positions = HashMap::new();
        for record in database.records_of_type(0x14, context) {
            let record = record?;
            if let DecodedRecord::Fixed(FixedRecord::Graphic(g)) = record.fields
                && g.layer == 0xf901
            {
                self.budget.check(512, record.span.offset.0 as usize)?;
                self.budget.commit(512);
                self.add_owner(g.parent, &mut owners, &mut owner_positions, context)?;
                selected.insert(g.key.0, (record.span.offset, false));
                candidates.push((g.key.0, g.parent));
            }
        }
        for text in texts {
            context.check_cancelled()?;
            if text.layer == LayerId::DIMENSION
                && let Some(owner) = text.owner_id
            {
                self.add_owner(owner.0, &mut owners, &mut owner_positions, context)?;
            }
        }
        let board = &database.header().graphic_list;
        if candidates.iter().any(|g| g.1 == board.tail) {
            self.walk(
                board.head,
                board.tail,
                None,
                FileOffset(if database.header().version >= 180 {
                    0x84
                } else {
                    0x60
                }),
                &mut selected,
                scale,
                context,
            )?;
        }
        for (id, head, offset) in owners {
            self.walk(
                head,
                id,
                Some(ObjectId(id)),
                offset,
                &mut selected,
                scale,
                context,
            )?;
        }
        for (key, parent) in candidates {
            context.check_cancelled()?;
            let (offset, accepted) = selected[&key];
            if !accepted
                && database
                    .get(RecordKey(parent), context)?
                    .is_none_or(|r| r.span.record_type != 0x2b)
            {
                self.warn(
                    "BRD_DRAWING_ORPHAN",
                    Message::new(Key::DrawingOrphan).arg("key", key),
                    key,
                    offset.0 as usize,
                )?;
            }
        }
        for text in texts {
            context.check_cancelled()?;
            if text.layer == LayerId::DIMENSION
                && let Some(owner) = text.owner_id
                && owner_positions.contains_key(&owner.0)
            {
                let index = self.group(owner.0, Some(owner), 0)?;
                let bytes = size_of::<ObjectId>() * 2;
                self.budget.check(bytes, 0)?;
                self.budget.commit(bytes);
                self.groups[index].text_ids.push(text.id);
            }
        }
        context.check_cancelled()?;
        Ok(StoredDrawings {
            drawings: self.groups,
            diagnostics: self.diagnostics,
        })
    }

    fn add_owner(
        &mut self,
        id: u32,
        owners: &mut Vec<(u32, u32, FileOffset)>,
        positions: &mut HashMap<u32, usize>,
        context: &ImportContext<'_>,
    ) -> Result<(), ImportError> {
        if positions.contains_key(&id) {
            return Ok(());
        }
        if let Some(record) = self.database.get(RecordKey(id), context)?
            && let DecodedRecord::Fixed(FixedRecord::FootprintInstance(fp)) = record.fields
        {
            self.budget.check(256, record.span.offset.0 as usize)?;
            self.budget.commit(256);
            positions.insert(id, owners.len());
            owners.push((id, fp.graphic_ptr, record.span.offset));
        }
        Ok(())
    }

    fn group(
        &mut self,
        id: u32,
        owner: Option<ObjectId>,
        offset: usize,
    ) -> Result<usize, ImportError> {
        if let Some(&index) = self.group_positions.get(&id) {
            return Ok(index);
        }
        let bytes = size_of::<BoardDrawing>() * 2 + 128;
        self.budget.check(bytes, offset)?;
        self.budget.commit(bytes);
        let index = self.groups.len();
        self.group_positions.insert(id, index);
        self.groups.push(BoardDrawing {
            id: ObjectId(id),
            owner_id: owner,
            layer: LayerId::DIMENSION,
            net: NetId(0),
            graphic_ids: Vec::new(),
            segments: Vec::new(),
            text_ids: Vec::new(),
        });
        Ok(index)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Source chains carry explicit membership, ownership, origin and scale"
    )]
    fn walk(
        &mut self,
        head: u32,
        tail: u32,
        owner: Option<ObjectId>,
        offset: FileOffset,
        selected: &mut HashMap<u32, (FileOffset, bool)>,
        scale: f64,
        context: &ImportContext<'_>,
    ) -> Result<(), ImportError> {
        let database = self.database;
        let limits = self.limits.chain.clone();
        let mut chain = AnnotationChain::new(&limits);
        let mut key = RecordKey(head);
        let mut origin = ReferenceLocation {
            offset,
            field: if owner.is_some() {
                "GraphicPtr"
            } else {
                "Header.GraphicList.Head"
            },
        };
        while key.0 != 0 && key.0 != tail && !database.header().sentinel_keys.contains(&key.0) {
            chain.visit(key, origin, context)?;
            let record = database.get(key, context)?;
            let Some(record) = record else {
                self.warn(
                    "BRD_DRAWING_OWNER_LINK_MISSING",
                    Message::new(Key::DrawingOwnerLinkMissing).arg("key", key.0),
                    key.0,
                    origin.offset.0 as usize,
                )?;
                break;
            };
            let DecodedRecord::Fixed(FixedRecord::Graphic(g)) = record.fields else {
                self.warn(
                    "BRD_DRAWING_OWNER_LINK_MISSING",
                    Message::new(Key::DrawingOwnerLinkMissing).arg("key", key.0),
                    key.0,
                    record.span.offset.0 as usize,
                )?;
                break;
            };
            if let Some((_, accepted)) = selected.get_mut(&key.0) {
                if g.parent != owner.map_or(tail, |id| id.0) {
                    self.warn(
                        "BRD_DRAWING_OWNER_MISMATCH",
                        Message::new(Key::DrawingOwnerMismatch).arg("key", key.0),
                        key.0,
                        record.span.offset.0 as usize,
                    )?;
                } else if !*accepted {
                    *accepted = true;
                    self.append(
                        g.key.0,
                        g.segment_ptr,
                        owner,
                        record.span.offset,
                        scale,
                        context,
                    )?;
                }
            }
            origin = ReferenceLocation {
                offset: record.span.offset,
                field: "Next",
            };
            key = RecordKey(g.next);
        }
        Ok(())
    }

    fn append(
        &mut self,
        graphic: u32,
        first: u32,
        owner: Option<ObjectId>,
        offset: FileOffset,
        scale: f64,
        context: &ImportContext<'_>,
    ) -> Result<(), ImportError> {
        let index = self.group(owner.map_or(graphic, |id| id.0), owner, offset.0 as usize)?;
        let bytes = size_of::<ObjectId>() * 2;
        self.budget.check(bytes, offset.0 as usize)?;
        self.budget.commit(bytes);
        self.groups[index].graphic_ids.push(ObjectId(graphic));
        let database = self.database;
        let limits = self.limits.chain.clone();
        let mut chain = AnnotationChain::new(&limits);
        let mut key = RecordKey(first);
        let mut origin = ReferenceLocation {
            offset,
            field: "SegmentPtr",
        };
        while key.0 != 0 && key.0 != graphic {
            if key.0 == first && chain.contains(key) {
                break;
            }
            chain.visit(key, origin, context)?;
            let Some(record) = database.get(key, context)? else {
                self.path_warning(
                    Key::DrawingPathMissing,
                    graphic,
                    key.0,
                    origin.offset.0 as usize,
                )?;
                break;
            };
            if ![1, 0x15, 0x16, 0x17].contains(&record.span.record_type) {
                self.path_warning(
                    Key::DrawingPathMissing,
                    graphic,
                    key.0,
                    record.span.offset.0 as usize,
                )?;
                break;
            }
            let mut edge = match decode_edge(&record, scale, false) {
                Ok(edge) if edge.width >= 0.0 => edge,
                Ok(_) | Err(ImportError::InvalidGeometry { .. }) => {
                    self.path_warning(
                        Key::DrawingGeometryInvalid,
                        graphic,
                        key.0,
                        record.span.offset.0 as usize,
                    )?;
                    break;
                }
                Err(error) => return Err(error),
            };
            edge.layer = LayerId::DIMENSION;
            let bytes = size_of::<Segment>() * 2;
            self.budget.check(bytes, record.span.offset.0 as usize)?;
            self.budget.commit(bytes);
            self.groups[index].segments.push(edge);
            origin = ReferenceLocation {
                offset: record.span.offset,
                field: "Next",
            };
            key = record.fields.next_key().unwrap_or(RecordKey(0));
        }
        Ok(())
    }

    fn path_warning(
        &mut self,
        message: Key,
        graphic: u32,
        key: u32,
        offset: usize,
    ) -> Result<(), ImportError> {
        let code = if message == Key::DrawingPathMissing {
            "BRD_DRAWING_PATH_MISSING"
        } else {
            "BRD_DRAWING_GEOMETRY_INVALID"
        };
        self.warn(
            code,
            Message::new(message)
                .arg("graphic", graphic)
                .arg("key", key),
            graphic,
            offset,
        )
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
