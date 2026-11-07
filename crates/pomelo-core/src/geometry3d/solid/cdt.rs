//! Constrained Delaunay caps; boundary parity excludes holes and concave exterior faces.
use super::SolidError;
use crate::task::CancellationToken;
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};

#[derive(Clone, Copy)]
struct Vertex {
    position: Point2<f64>,
    source: u32,
}
impl HasPosition for Vertex {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.position
    }
}

pub(super) fn triangulate(
    points: &[[f64; 2]],
    holes: &[u32],
    triangles: &mut Vec<u32>,
    cancel: &CancellationToken,
) -> Result<(), SolidError> {
    cancel.check()?;
    if points.len() > u32::MAX as usize {
        return Err(SolidError::Invalid("CDT_POINT_COUNT"));
    }
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(points.len())
        .map_err(|_| SolidError::Allocation)?;
    vertices.extend(points.iter().enumerate().map(|(i, p)| Vertex {
        position: Point2::new(p[0], p[1]),
        source: i as u32,
    }));
    let mut edges = Vec::new();
    edges
        .try_reserve_exact(points.len())
        .map_err(|_| SolidError::Allocation)?;
    let mut start = 0;
    for end in holes
        .iter()
        .map(|&i| i as usize)
        .chain(std::iter::once(points.len()))
    {
        if end > points.len() || end.saturating_sub(start) < 3 {
            return Err(SolidError::Invalid("CDT_RING"));
        }
        for i in start..end {
            edges.push([i, if i + 1 == end { start } else { i + 1 }]);
        }
        start = end;
    }
    let mut conflicts = 0usize;
    let cdt =
        ConstrainedDelaunayTriangulation::<Vertex>::try_bulk_load_cdt(vertices, edges, |_| {
            conflicts += 1
        })
        .map_err(|_| SolidError::Invalid("CDT_INSERTION"))?;
    if conflicts != 0 {
        return Err(SolidError::Invalid("CDT_CONSTRAINT_CONFLICT"));
    }
    cancel.check()?;
    // Cross every constrained edge with a parity flip. The outer face is outside.
    // Clipper has already produced nonintersecting exterior/hole boundaries.
    let mut parity = Vec::new();
    parity
        .try_reserve_exact(cdt.num_all_faces())
        .map_err(|_| SolidError::Allocation)?;
    parity.resize(cdt.num_all_faces(), u8::MAX);
    parity[cdt.outer_face().fix().index()] = 0;
    let mut queue = Vec::new();
    queue
        .try_reserve_exact(cdt.num_inner_faces())
        .map_err(|_| SolidError::Allocation)?;
    for edge in cdt.convex_hull() {
        let face = edge
            .face()
            .as_inner()
            .or_else(|| edge.rev().face().as_inner());
        if let Some(face) = face {
            let value = u8::from(edge.is_constraint_edge());
            let index = face.fix().index();
            if parity[index] == u8::MAX {
                parity[index] = value;
                queue.push(face.fix());
            } else if parity[index] != value {
                return Err(SolidError::Invalid("CDT_HULL_PARITY"));
            }
        }
    }
    let mut cursor: usize = 0;
    while cursor < queue.len() {
        if cursor.is_multiple_of(1024) {
            cancel.check()?;
        }
        let handle = queue[cursor];
        cursor += 1;
        let value = parity[handle.index()];
        for edge in cdt.face(handle).adjacent_edges() {
            let other = edge.rev().face();
            let expected = value ^ u8::from(edge.is_constraint_edge());
            let index = other.fix().index();
            if parity[index] == u8::MAX {
                parity[index] = expected;
                if let Some(inner) = other.as_inner() {
                    queue.push(inner.fix());
                }
            } else if parity[index] != expected {
                return Err(SolidError::Invalid("CDT_REGION_PARITY"));
            }
        }
    }
    triangles.clear();
    triangles
        .try_reserve(cdt.num_inner_faces().saturating_mul(3))
        .map_err(|_| SolidError::Allocation)?;
    for (i, face) in cdt.inner_faces().enumerate() {
        if i.is_multiple_of(1024) {
            cancel.check()?;
        }
        match parity[face.fix().index()] {
            1 => triangles.extend(face.vertices().map(|v| v.data().source)),
            0 => {}
            _ => return Err(SolidError::Invalid("CDT_UNCLASSIFIED_FACE")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_ring_offsets_and_crossing_constraints() {
        let points = [[0., 0.], [2., 2.], [0., 2.], [2., 0.]];
        for holes in [&[2u32][..], &[5u32][..], &[3u32, 1u32][..]] {
            assert!(
                triangulate(
                    &points,
                    holes,
                    &mut Vec::new(),
                    &CancellationToken::default()
                )
                .is_err()
            );
        }
        assert!(triangulate(&points, &[], &mut Vec::new(), &CancellationToken::default()).is_err());
    }
    #[test]
    fn cancelled_cdt_never_starts_bulk_insertion() {
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(matches!(
            triangulate(
                &[[0., 0.], [1., 0.], [0., 1.]],
                &[],
                &mut Vec::new(),
                &cancel
            ),
            Err(SolidError::Cancelled)
        ));
    }
}
