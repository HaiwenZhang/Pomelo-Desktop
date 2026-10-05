//! Immutable per-upload lookup data; source draw order stays in the original buffer.
use crate::pads::PadInstance;
use pomelo_core::selection::SelectedObject;

pub(super) struct PadIndex {
    nets: Vec<[u32; 3]>,        // net, instance count, source-order run count
    owners: [Vec<[u32; 2]>; 2], // separate pin/via buckets: object, net
}

impl PadIndex {
    pub(super) fn build(source: &[PadInstance]) -> anyhow::Result<Self> {
        let mut nets = Vec::new();
        nets.try_reserve_exact(source.len())?;
        let mut previous = None;
        for pad in source {
            let net = pad.ids[2];
            nets.push([net, 1, u32::from(previous != Some(net))]);
            previous = Some(net);
        }
        nets.sort_unstable_by_key(|row| row[0]);
        let mut written = 0;
        for read in 0..nets.len() {
            let row = nets[read];
            if written > 0 && nets[written - 1][0] == row[0] {
                nets[written - 1][1] += row[1];
                nets[written - 1][2] += row[2];
            } else {
                nets[written] = row;
                written += 1;
            }
        }
        nets.truncate(written);
        let nets = compact(nets)?;
        let mut owners = [Vec::new(), Vec::new()];
        let mut counts = [0, 0];
        for pad in source {
            counts[usize::from(pad.source[0] != 0)] += 1;
        }
        for (rows, count) in owners.iter_mut().zip(counts) {
            rows.try_reserve_exact(count)?;
        }
        for pad in source {
            owners[usize::from(pad.source[0] != 0)].push([pad.ids[0], pad.ids[2]]);
        }
        for rows in &mut owners {
            rows.sort_unstable();
            rows.dedup();
        }
        let [pins, vias] = owners;
        Ok(Self {
            nets,
            owners: [compact(pins)?, compact(vias)?],
        })
    }

    pub(super) fn summary(&self, net: u32) -> (usize, usize) {
        self.nets
            .binary_search_by_key(&net, |row| row[0])
            .map_or((0, 0), |index| {
                (self.nets[index][1] as usize, self.nets[index][2] as usize)
            })
    }

    pub(super) fn contains_category(&self, pin: bool) -> bool {
        !self.owners[usize::from(!pin)].is_empty()
    }

    pub(super) fn extra_hover(&self, net: u32, hovered: Option<SelectedObject>) -> bool {
        let (category, owner) = match hovered {
            Some(SelectedObject::Pin(id)) => (0, id.0),
            Some(SelectedObject::Via(id)) => (1, id.0),
            _ => return false,
        };
        let rows = &self.owners[category];
        let start = rows.partition_point(|row| row[0] < owner);
        rows[start..]
            .iter()
            .take_while(|row| row[0] == owner)
            .any(|row| row[1] != net)
    }
}

// Sorting needs one bounded upload-chunk scratch vector. Retain only unique rows,
// rather than every chunk's full scratch capacity for the lifetime of the board.
fn compact<T: Copy>(rows: Vec<T>) -> anyhow::Result<Vec<T>> {
    if rows.len() == rows.capacity() {
        return Ok(rows);
    }
    let mut result = Vec::new();
    result.try_reserve_exact(rows.len())?;
    result.extend_from_slice(&rows);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;
    use pomelo_core::model::ObjectId;

    #[test]
    #[ignore = "requires explicit local board input; pad index memory and construction diagnostic"]
    fn real_board_pad_index_profile() {
        use crate::{drills::PreparedDrills, pads::PreparedPads};
        use pomelo_import::{
            BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
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
        let limits = crate::pads::PadLimits {
            max_pads: usize::MAX,
            max_bytes: usize::MAX,
        };
        let pads =
            PreparedPads::build(&board.scene.pins, &board.scene.vias, limits, &cancel).unwrap();
        let drills =
            PreparedDrills::build(&board.scene.pins, &board.scene.vias, limits, &cancel).unwrap();
        for (kind, source) in [("pads", &pads), ("drills", &drills.geometry)] {
            let started = std::time::Instant::now();
            let mut bytes = 0;
            let mut max_chunk_us = 0;
            for chunk in source.analytic.chunks(super::super::trace::CHUNK_INSTANCES) {
                let chunk_started = std::time::Instant::now();
                let index = PadIndex::build(chunk).unwrap();
                max_chunk_us = max_chunk_us.max(chunk_started.elapsed().as_micros());
                bytes += index.nets.capacity() * size_of::<[u32; 3]>()
                    + index
                        .owners
                        .iter()
                        .map(|rows| rows.capacity() * size_of::<[u32; 2]>())
                        .sum::<usize>();
            }
            println!(
                "PAD_INDEX_PROFILE {}",
                serde_json::json!({"case":path,"kind":kind,"instances":source.analytic.len(),"retained_bytes":bytes,"build_us":started.elapsed().as_micros(),"max_chunk_us":max_chunk_us})
            );
        }
    }

    #[test]
    fn indexed_counts_runs_and_typed_owners_match_source_scan() {
        let source: Vec<_> = (0..16_385)
            .map(|i| {
                let mut pad = PadInstance::zeroed();
                pad.ids[0] = [0, 7, u32::MAX][i % 3];
                pad.ids[2] = [0, 3, 3, u32::MAX][i % 4];
                pad.source[0] = (i % 2) as u32;
                pad
            })
            .collect();
        for source in [
            &source[..0],
            &source[..63],
            &source[..16_384],
            &source[16_384..],
        ] {
            let index = PadIndex::build(source).unwrap();
            for pin in [false, true] {
                assert_eq!(
                    index.contains_category(pin),
                    source.iter().any(|pad| (pad.source[0] == 0) == pin)
                );
            }
            for net in [0, 3, 9, u32::MAX] {
                let count = source.iter().filter(|pad| pad.ids[2] == net).count();
                let runs = source
                    .iter()
                    .enumerate()
                    .filter(|&(i, pad)| {
                        pad.ids[2] == net && (i == 0 || source[i - 1].ids[2] != net)
                    })
                    .count();
                assert_eq!(index.summary(net), (count, runs));
                for id in [0, 7, 9, u32::MAX] {
                    for hovered in [
                        None,
                        Some(SelectedObject::Pin(ObjectId(id))),
                        Some(SelectedObject::Via(ObjectId(id))),
                        Some(SelectedObject::Drawing(ObjectId(id))),
                    ] {
                        let expected = source.iter().any(|pad| {
                            let object = if pad.source[0] == 0 {
                                SelectedObject::Pin(ObjectId(pad.ids[0]))
                            } else {
                                SelectedObject::Via(ObjectId(pad.ids[0]))
                            };
                            Some(object) == hovered && pad.ids[2] != net
                        });
                        assert_eq!(index.extra_hover(net, hovered), expected);
                    }
                }
            }
            assert!(index.nets.len() <= 3);
            assert!(index.owners.iter().map(Vec::len).sum::<usize>() <= 18);
        }
    }
}
