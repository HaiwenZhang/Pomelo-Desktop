//! Index-only circle LODs; picking retains the complete prepared surface.
use super::*;
impl Mesh3d {
    pub(super) fn add_round_lods(
        &mut self,
        budget: usize,
        cancel: &CancellationToken,
    ) -> Result<(), SolidPrepareError> {
        let fail = || Builder::error(SolidError::Allocation);
        let base_bytes =
            self.vertices.capacity() * size_of::<Vertex3d>() + self.indices.capacity() * 4;
        let scratch = self
            .vertices
            .len()
            .saturating_mul(64)
            .saturating_add(12 * 128 * 4 + 4 * size_of::<RoundLod3d>());
        if base_bytes.saturating_add(scratch) > budget {
            return Ok(());
        }
        let min_z = self
            .vertices
            .iter()
            .map(|v| v.position[2])
            .fold(f32::INFINITY, f32::min);
        let max_z = self
            .vertices
            .iter()
            .map(|v| v.position[2])
            .fold(f32::NEG_INFINITY, f32::max);
        if min_z >= max_z {
            return Ok(());
        }
        let mut top = Vec::new();
        let mut bottom = Vec::new();
        top.try_reserve_exact(self.vertices.len() / 2)
            .map_err(|_| fail())?;
        bottom
            .try_reserve_exact(self.vertices.len() / 2)
            .map_err(|_| fail())?;
        let mut radius = 0.0_f64;
        let mut inner = f64::INFINITY;
        for (i, v) in self.vertices.iter().enumerate() {
            if i.is_multiple_of(1024) {
                cancel.check().map_err(|e| Builder::error(e.into()))?;
            }
            let x = f64::from(v.position[0]);
            let y = f64::from(v.position[1]);
            let r = x.hypot(y);
            radius = radius.max(r);
            inner = inner.min(r);
            let entry = (y.atan2(x), i as u32);
            if v.position[2] == max_z {
                top.push(entry);
            } else if v.position[2] == min_z {
                bottom.push(entry);
            } else {
                return Ok(());
            }
        }
        if top.len() != bottom.len() || top.len() < 16 || radius - inner > radius * 1e-4 + 2e-6 {
            return Ok(());
        }
        top.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        bottom.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        if top.iter().zip(&bottom).any(|(a, b)| {
            self.vertices[a.1 as usize].position[..2] != self.vertices[b.1 as usize].position[..2]
        }) {
            return Ok(());
        }
        for segments in [8usize, 16, 32, 64] {
            if segments >= top.len() {
                break;
            }
            self.indices
                .try_reserve_exact(12 * segments - 12)
                .map_err(|_| fail())?;
            self.round_lods.try_reserve_exact(1).map_err(|_| fail())?;
            let start = self.indices.len() as u32;
            let mut gap = 0.0_f64;
            for i in 0..segments {
                let a = i * top.len() / segments;
                let b = (i + 1) * top.len() / segments;
                let next = b % top.len();
                let angle = if b == top.len() {
                    top[0].0 + std::f64::consts::TAU
                } else {
                    top[b].0
                };
                gap = gap.max(angle - top[a].0);
                let (ta, tb, ba, bb) = (top[a].1, top[next].1, bottom[a].1, bottom[next].1);
                self.indices.extend_from_slice(&[ba, bb, tb, ba, tb, ta]);
                if i > 0 && i + 1 < segments {
                    self.indices
                        .extend_from_slice(&[top[0].1, ta, tb, bottom[0].1, bb, ba]);
                }
            }
            let error = radius * (1.0 - (gap * 0.5).cos()) + 2.0 * (radius - inner) + 2e-6;
            self.round_lods.push(RoundLod3d {
                indices: start..self.indices.len() as u32,
                error_mm: (error * (1.0 + 1e-6)) as f32,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_lod_ranges_are_rejected_before_native_draw() {
        let mut mesh = Mesh3d {
            vertices: vec![],
            indices: vec![0; 12],
            round_lods: vec![RoundLod3d {
                indices: 6..12,
                error_mm: 0.01,
            }],
        };
        assert!(mesh.valid_lods());
        mesh.round_lods[0].indices.end = 15;
        assert!(!mesh.valid_lods());
        mesh.round_lods[0].indices = 6..12;
        mesh.round_lods[0].error_mm = f32::NAN;
        assert!(!mesh.valid_lods());
    }
}
