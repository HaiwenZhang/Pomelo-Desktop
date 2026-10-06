use super::PlacementDecoder;
use crate::{
    ImportContext, ImportError,
    formats::allegro::{
        database::{ChainRequest, ReferenceLocation},
        decoder::{DecodedRecord, fixed::FixedRecord},
        index::{FileOffset, RecordKey},
        semantics::{bond::resolve_wire, geometry::decode_edge},
    },
};
use pomelo_core::{
    i18n::{Message, MessageKey},
    model::{LayerId, NetId, ObjectId, Segment},
    task::{ImportProgress, ImportStage},
};

impl PlacementDecoder<'_> {
    /// Decode copper tracks and supported TOP bond wires with their source identities.
    pub fn track(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Vec<Segment>, ImportError> {
        let database = self.database;
        let offset = database
            .index()
            .record(key)
            .map_or(0, |span| span.offset.0 as usize);
        let record = database.require_record(
            key,
            &[5],
            ReferenceLocation {
                offset: FileOffset(offset as u32),
                field: "Track",
            },
            context,
        )?;
        let DecodedRecord::Fixed(FixedRecord::Track(track)) = record.fields else {
            return Err(ImportError::InvalidGeometry {
                key: key.0,
                offset,
                field: "TRACK_RECORD_TYPE",
            });
        };
        if track.layer & 255 != 6 {
            return Ok(Vec::new());
        }
        if track.layer == 0xfd06 {
            let Some(wire) = resolve_wire(
                database,
                &track,
                &mut self.stacks,
                &self.chain,
                self.budget.available(),
                context,
            )?
            else {
                self.report(
                    "BRD_BOND_WIRE_UNSUPPORTED",
                    Message::new(MessageKey::BondWireUnsupported).arg("track", key.0),
                    key,
                    offset,
                )?;
                return Ok(Vec::new());
            };
            let charge = 2 * std::mem::size_of::<Segment>()
                + wire.info.profile.len()
                + wire.info.material.as_ref().map_or(0, String::len)
                + wire.info.reference.len()
                + wire.info.pin_name.len();
            self.budget.check(charge, offset)?;
            self.check_track_allocation(1, offset)?;
            let mut segment = decode_edge(&wire.segment, self.pads.scale(), false)?;
            segment.track_id = ObjectId(key.0);
            segment.layer = LayerId::BOND_WIRE_TOP;
            segment.net = wire.net;
            segment.bond_wire = Some(wire.info);
            context.check_cancelled()?;
            self.budget.commit(charge);
            return Ok(vec![segment]);
        }
        let layer = u32::from(track.layer >> 8);
        if layer >= self.layers {
            self.report(
                "BRD_TRACK_LAYER_UNDEFINED",
                Message::new(MessageKey::TrackLayerUndefined)
                    .arg("track", key.0)
                    .arg("layer", layer),
                key,
                offset,
            )?;
            return Ok(Vec::new());
        }
        let net = self.networks.owner(key).unwrap_or(NetId(0));
        let mut segments = Vec::new();
        database.walk_chain(
            &ChainRequest {
                start: RecordKey(track.first_seg_ptr),
                terminator: key,
                expected_types: &[1, 0x15, 0x16, 0x17],
                origin: ReferenceLocation {
                    offset: record.span.offset,
                    field: "FirstSegPtr",
                },
                link_field: "Next",
                limits: self.chain.clone(),
            },
            context,
            |record| {
                record
                    .fields
                    .next_key()
                    .ok_or(ImportError::InvalidGeometry {
                        key: record.span.key.0,
                        offset: record.span.offset.0 as usize,
                        field: "TRACK_NEXT",
                    })
            },
            |record| {
                let offset = record.span.offset.0 as usize;
                self.check_track_allocation(segments.len().saturating_add(1), offset)?;
                let charge = 2 * std::mem::size_of::<Segment>();
                self.budget.check(charge, offset)?;
                let mut segment = decode_edge(&record, self.pads.scale(), false)?;
                segment.track_id = ObjectId(key.0);
                segment.layer = LayerId(layer);
                segment.net = net;
                segments.push(segment);
                self.budget.commit(charge);
                self.placed_count += 1;
                if self.placed_count.is_multiple_of(256) {
                    (context.progress)(ImportProgress {
                        stage: ImportStage::BuildingGeometry,
                        completed: self.placed_count as u64,
                        total: None,
                    });
                }
                Ok(())
            },
        )?;
        context.check_cancelled()?;
        Ok(segments)
    }

    fn check_track_allocation(&self, count: usize, offset: usize) -> Result<(), ImportError> {
        if count > self.geometry_limits.max_edges {
            return Err(ImportError::InvalidRecord {
                offset,
                field: "TRACK_EDGE_COUNT",
                value: count as u64,
            });
        }
        let actual = count.saturating_mul(2 * std::mem::size_of::<Segment>() + 64);
        if actual > self.geometry_limits.max_allocation_bytes {
            return Err(ImportError::GeometryLimit {
                offset,
                actual: actual as u64,
                limit: self.geometry_limits.max_allocation_bytes as u64,
            });
        }
        Ok(())
    }
}
