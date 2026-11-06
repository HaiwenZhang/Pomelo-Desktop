use super::*;
use pomelo_core::interaction::camera3d::Camera3d;

#[derive(Default)]
pub(super) struct SpatialIndex3d {
    ids: Vec<usize>,
    nodes: Vec<Node>,
}
struct Node {
    bounds: Bounds3,
    range: Range<usize>,
    children: Option<[usize; 2]>,
}

impl SpatialIndex3d {
    pub fn estimate_bytes(count: usize) -> usize {
        count.saturating_mul(size_of::<usize>()).saturating_add(
            count
                .div_ceil(8)
                .saturating_mul(2)
                .saturating_mul(size_of::<Node>()),
        )
    }
    pub fn allocation_bytes(&self) -> usize {
        self.ids.capacity() * size_of::<usize>() + self.nodes.capacity() * size_of::<Node>()
    }

    pub fn build(
        count: usize,
        bounds: impl Fn(usize) -> Bounds3,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        let mut tree = Self::default();
        tree.ids
            .try_reserve_exact(count)
            .map_err(|_| Builder::error(SolidError::Allocation))?;
        tree.ids.extend(0..count);
        tree.nodes
            .try_reserve_exact(count.div_ceil(8).saturating_mul(2))
            .map_err(|_| Builder::error(SolidError::Allocation))?;
        if count > 0 {
            tree.partition(0..count, &bounds, cancel)?;
        }
        Ok(tree)
    }
    fn partition(
        &mut self,
        range: Range<usize>,
        bounds: &impl Fn(usize) -> Bounds3,
        cancel: &CancellationToken,
    ) -> Result<usize, SolidPrepareError> {
        cancel.check().map_err(|e| Builder::error(e.into()))?;
        let mut b = Bounds3 {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        };
        for (i, &id) in self.ids[range.clone()].iter().enumerate() {
            if i.is_multiple_of(1024) {
                cancel.check().map_err(|e| Builder::error(e.into()))?;
            }
            let item = bounds(id);
            for axis in 0..3 {
                b.min[axis] = b.min[axis].min(item.min[axis]);
                b.max[axis] = b.max[axis].max(item.max[axis]);
            }
        }
        if !b.is_valid() {
            return Err(Builder::error(SolidError::Invalid("SPATIAL_BOUNDS")));
        }
        let node = self.nodes.len();
        self.nodes
            .try_reserve(1)
            .map_err(|_| Builder::error(SolidError::Allocation))?;
        self.nodes.push(Node {
            bounds: b,
            range: range.clone(),
            children: None,
        });
        if range.len() > 16 {
            let axis = (0..3)
                .max_by(|&a, &c| (b.max[a] - b.min[a]).total_cmp(&(b.max[c] - b.min[c])))
                .unwrap_or(0);
            let mid = range.len() / 2;
            self.ids[range.clone()].select_nth_unstable_by(mid, |&a, &b| {
                bounds(a).center()[axis].total_cmp(&bounds(b).center()[axis])
            });
            let split = range.start + mid;
            let left = self.partition(range.start..split, bounds, cancel)?;
            let right = self.partition(split..range.end, bounds, cancel)?;
            self.nodes[node].children = Some([left, right]);
        }
        Ok(node)
    }
    pub fn visit_xy(
        &self,
        bounds: pomelo_core::model::Bounds,
        mut visit: impl FnMut(usize) -> Result<(), SolidError>,
    ) -> Result<(), SolidError> {
        if !self.nodes.is_empty() {
            self.xy_node(0, bounds, &mut visit)?;
        }
        Ok(())
    }
    fn xy_node(
        &self,
        node: usize,
        bounds: pomelo_core::model::Bounds,
        visit: &mut impl FnMut(usize) -> Result<(), SolidError>,
    ) -> Result<(), SolidError> {
        let node = &self.nodes[node];
        if node.bounds.min[0] > bounds.max.x
            || node.bounds.max[0] < bounds.min.x
            || node.bounds.min[1] > bounds.max.y
            || node.bounds.max[1] < bounds.min.y
        {
            return Ok(());
        }
        if let Some([left, right]) = node.children {
            self.xy_node(left, bounds, visit)?;
            self.xy_node(right, bounds, visit)?;
        } else {
            for &id in &self.ids[node.range.clone()] {
                visit(id)?;
            }
        }
        Ok(())
    }
    pub fn visit_ray(
        &self,
        ray: PickRay3d,
        nearest: &mut f64,
        mut visit: impl FnMut(usize, &mut f64),
    ) {
        if !self.nodes.is_empty() {
            self.ray_node(0, ray, nearest, &mut visit);
        }
    }
    fn ray_node(
        &self,
        node: usize,
        ray: PickRay3d,
        nearest: &mut f64,
        visit: &mut impl FnMut(usize, &mut f64),
    ) {
        let node = &self.nodes[node];
        if !ray_box(ray, node.bounds, *nearest) {
            return;
        }
        if let Some([left, right]) = node.children {
            self.ray_node(left, ray, nearest, visit);
            self.ray_node(right, ray, nearest, visit);
        } else {
            for &id in &self.ids[node.range.clone()] {
                visit(id, nearest);
            }
        }
    }
    pub fn visit_frustum(
        &self,
        camera: Camera3d,
        viewport: [f64; 2],
        scale: f64,
        mut visit: impl FnMut(usize),
    ) {
        if !self.nodes.is_empty() {
            self.frustum_node(0, camera, viewport, scale, &mut visit);
        }
    }
    fn frustum_node(
        &self,
        node: usize,
        camera: Camera3d,
        viewport: [f64; 2],
        scale: f64,
        visit: &mut impl FnMut(usize),
    ) {
        let node = &self.nodes[node];
        let mut bounds = node.bounds;
        bounds.min[2] *= scale;
        bounds.max[2] *= scale;
        if !camera.intersects(bounds, viewport) {
            return;
        }
        if let Some([left, right]) = node.children {
            self.frustum_node(left, camera, viewport, scale, visit);
            self.frustum_node(right, camera, viewport, scale, visit);
        } else {
            for &id in &self.ids[node.range.clone()] {
                visit(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_candidates_cover_bruteforce_hits_and_exclude_distant_regions() {
        let bounds: Vec<_> = (0..4096)
            .map(|i| {
                let x = (i % 64) as f64 * 10.0;
                let y = (i / 64) as f64 * 10.0;
                Bounds3 {
                    min: [x, y, -1.0],
                    max: [x + 1.0, y + 1.0, 1.0],
                }
            })
            .collect();
        let index =
            SpatialIndex3d::build(bounds.len(), |i| bounds[i], &CancellationToken::default())
                .unwrap();
        let camera = Camera3d::new([20.0, 20.0, 0.0], [0.0, 0.0, 0.0, 1.0], 15.0).unwrap();
        let mut candidates = std::collections::BTreeSet::new();
        index.visit_frustum(camera, [100.0; 2], 5.0, |id| {
            candidates.insert(id);
        });
        assert!(candidates.len() < 64);
        for (i, &b) in bounds.iter().enumerate() {
            if camera.intersects(b, [100.0; 2]) {
                assert!(candidates.contains(&i));
            }
        }
        for x in [0.5, 20.5, 400.5, 800.0] {
            let ray = PickRay3d {
                origin: [x, 20.5, 10.0],
                direction: [0.0, 0.0, -1.0],
                length: 20.0,
            };
            let mut candidates = std::collections::BTreeSet::new();
            index.visit_ray(ray, &mut 20.0, |id, _| {
                candidates.insert(id);
            });
            for (i, &b) in bounds.iter().enumerate() {
                if ray_box(ray, b, 20.0) {
                    assert!(candidates.contains(&i));
                }
            }
        }
    }
}
