//! Immutable network runs in source order; built within existing upload chunks.
use super::trace::InstanceSource;
use pomelo_core::selection::SelectedObject;
use std::ops::Range;

pub(super) struct NetRuns {
    rows: Vec<[u32; 3]>, // net, global start, global end
    categories: u8,
}

impl NetRuns {
    pub(super) fn build<S: InstanceSource>(
        source: &S,
        range: Range<usize>,
    ) -> anyhow::Result<Self> {
        let mut rows: Vec<[u32; 3]> = Vec::new();
        rows.try_reserve_exact(range.len())?;
        let mut categories = 0;
        for index in range {
            let net = source.selection_ids(index)[3];
            categories |= category(source.selected_object(index));
            if let Some(last) = rows.last_mut().filter(|row| row[0] == net) {
                last[2] = index as u32 + 1;
            } else {
                rows.push([net, index as u32, index as u32 + 1]);
            }
        }
        rows.sort_unstable();
        if rows.len() != rows.capacity() {
            let mut compact = Vec::new();
            compact.try_reserve_exact(rows.len())?;
            compact.extend_from_slice(&rows);
            rows = compact;
        }
        Ok(Self { rows, categories })
    }

    // Category absence proves that the pointer cannot add geometry to this chunk.
    // Presence deliberately retains exact membership, even for a missing object ID.
    pub(super) fn may_contain(&self, object: SelectedObject) -> bool {
        self.categories & category(object) != 0
    }

    pub(super) fn visible(
        &self,
        net: u32,
        range: Range<usize>,
        mut emit: impl FnMut(Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let first = self.rows.partition_point(|row| row[0] < net);
        let last = self.rows.partition_point(|row| row[0] <= net);
        let rows = &self.rows[first..last];
        let start = rows.partition_point(|row| row[2] as usize <= range.start);
        for row in rows[start..]
            .iter()
            .take_while(|row| (row[1] as usize) < range.end)
        {
            let start = (row[1] as usize).max(range.start);
            let end = (row[2] as usize).min(range.end);
            if start < end {
                emit(start..end)?;
            }
        }
        Ok(())
    }
}

fn category(object: SelectedObject) -> u8 {
    match object {
        SelectedObject::Segment(_) => 1,
        SelectedObject::Pin(_) => 2,
        SelectedObject::Via(_) => 4,
        SelectedObject::Zone(_) => 8,
        SelectedObject::Drawing(_) => 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracks::{PreparedTracks, TraceInstance};
    use bytemuck::Zeroable;
    use pomelo_core::model::ObjectId;

    #[test]
    fn net_runs_match_source_order_for_partial_chunks_and_clipped_ranges() {
        let source = PreparedTracks {
            instances: (0..16_385)
                .map(|i| {
                    let mut line = TraceInstance::zeroed();
                    line.ids = [7, 0, 0, [0, 3, 3, u32::MAX][i % 4]];
                    line.flags[3] = 8;
                    line
                })
                .collect(),
            batches: Vec::new(),
            memory_reservation: None,
        };
        for chunk in [0..0, 0..65, 0..16_384, 16_384..16_385] {
            let runs = NetRuns::build(&source, chunk.clone()).unwrap();
            assert_eq!(
                runs.may_contain(SelectedObject::Zone(ObjectId(7))),
                !chunk.is_empty()
            );
            assert!(!runs.may_contain(SelectedObject::Via(ObjectId(7))));
            for net in [0, 3, 9, u32::MAX] {
                for range in [0..0, 1..64, 63..129, 0..16_385, 16_384..16_385] {
                    let mut actual = Vec::new();
                    let mut previous = None;
                    runs.visible(net, range.clone(), |span| {
                        assert!(previous.is_none_or(|end| end < span.start));
                        previous = Some(span.end);
                        actual.extend(span);
                        Ok(())
                    })
                    .unwrap();
                    let expected: Vec<_> = chunk
                        .clone()
                        .filter(|&i| range.contains(&i) && source.instances[i].ids[3] == net)
                        .collect();
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    #[ignore = "requires explicit local input; network run footprint diagnostic"]
    fn real_board_net_runs_profile() {
        use pomelo_import::{
            BoardImporter, ImportContext, ImportOptions, TextEncoding,
            formats::allegro::AllegroImporter,
        };
        let path = std::env::var_os("POMELO_FRAME_PROFILE_CASE").expect("explicit local case");
        let cancel = pomelo_core::task::CancellationToken::default();
        let board = AllegroImporter
            .import(
                std::path::Path::new(&path),
                &ImportOptions {
                    text_encoding: TextEncoding::Windows1252,
                    ..Default::default()
                },
                &ImportContext {
                    cancellation: &cancel,
                    progress: &|_| {},
                },
            )
            .unwrap();
        let limits = crate::tracks::TraceLimits {
            max_instances: usize::MAX,
            max_bytes: usize::MAX,
        };
        for (kind, source) in [
            (
                "tracks",
                PreparedTracks::build_with_outline(
                    &board.scene.segments,
                    &board.scene.outline,
                    limits,
                    &cancel,
                )
                .unwrap(),
            ),
            (
                "boundaries",
                PreparedTracks::build_zone_outlines(&board.scene.zones, limits, &cancel).unwrap(),
            ),
        ] {
            let started = std::time::Instant::now();
            let mut bytes = 0;
            let mut rows = 0;
            let mut max_chunk_us = 0;
            for start in (0..source.instances.len()).step_by(super::super::trace::CHUNK_INSTANCES) {
                let end =
                    (start + super::super::trace::CHUNK_INSTANCES).min(source.instances.len());
                let chunk_started = std::time::Instant::now();
                let runs = NetRuns::build(&source, start..end).unwrap();
                max_chunk_us = max_chunk_us.max(chunk_started.elapsed().as_micros());
                bytes += runs.rows.capacity() * size_of::<[u32; 3]>();
                rows += runs.rows.len();
            }
            println!(
                "NET_RUNS_PROFILE {}",
                serde_json::json!({"case":path.to_string_lossy(),"kind":kind,"instances":source.instances.len(),"rows":rows,"bytes":bytes,"build_us":started.elapsed().as_micros(),"max_chunk_us":max_chunk_us})
            );
        }
    }
}
