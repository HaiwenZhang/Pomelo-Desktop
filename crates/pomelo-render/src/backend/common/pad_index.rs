//! Immutable per-upload lookup data; source draw order stays in the original buffer.
use crate::pads::PadInstance;
use pomelo_core::selection::SelectedObject;

pub(super) struct PadIndex {
    nets: Vec<[u32; 3]>,   // net, instance count, source-order run count
    owners: Vec<[u32; 3]>, // pin/via category, object, net
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
        let mut owners = Vec::new();
        owners.try_reserve_exact(source.len())?;
        owners.extend(
            source
                .iter()
                .map(|pad| [u32::from(pad.source[0] != 0), pad.ids[0], pad.ids[2]]),
        );
        owners.sort_unstable();
        owners.dedup();
        Ok(Self {
            nets,
            owners: compact(owners)?,
        })
    }

    pub(super) fn summary(&self, net: u32) -> (usize, usize) {
        self.nets
            .binary_search_by_key(&net, |row| row[0])
            .map_or((0, 0), |index| {
                (self.nets[index][1] as usize, self.nets[index][2] as usize)
            })
    }

    pub(super) fn extra_hover(&self, net: u32, hovered: Option<SelectedObject>) -> bool {
        let owner = match hovered {
            Some(SelectedObject::Pin(id)) => [0, id.0],
            Some(SelectedObject::Via(id)) => [1, id.0],
            _ => return false,
        };
        let start = self.owners.partition_point(|row| row[..2] < owner);
        self.owners[start..]
            .iter()
            .take_while(|row| row[..2] == owner)
            .any(|row| row[2] != net)
    }
}

// Sorting needs one bounded upload-chunk scratch vector. Retain only unique rows,
// rather than every chunk's full scratch capacity for the lifetime of the board.
fn compact(rows: Vec<[u32; 3]>) -> anyhow::Result<Vec<[u32; 3]>> {
    if rows.len() == rows.capacity() {
        return Ok(rows);
    }
    let mut result = Vec::new();
    result.try_reserve_exact(rows.len())?;
    result.extend_from_slice(&rows);
    Ok(result)
}
