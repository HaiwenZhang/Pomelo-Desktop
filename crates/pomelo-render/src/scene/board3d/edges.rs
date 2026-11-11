//! Feature edges for outline and wireframe drawing. An edge is kept where the
//! surface folds sharply or ends; flat triangulation seams and smooth round
//! walls stay invisible.
use super::Vertex3d;
use std::collections::HashMap;

/// Faces meeting at more than this angle draw their shared edge.
const FEATURE_COS: f32 = 0.85;

fn normal(vertices: &[Vertex3d], triangle: &[u32; 3]) -> Option<[f32; 3]> {
    let p = |i: usize| vertices.get(triangle[i] as usize).map(|v| v.position);
    let (a, b, c) = (p(0)?, p(1)?, p(2)?);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (length > f32::EPSILON && length.is_finite()).then(|| n.map(|v| v / length))
}

/// Line-list indices of the feature edges of a triangle list.
pub(super) fn feature_edges(vertices: &[Vertex3d], triangles: &[u32]) -> Vec<u32> {
    // Each undirected edge with the normals of the faces sharing it.
    let mut faces: HashMap<(u32, u32), Vec<[f32; 3]>> = HashMap::new();
    for triangle in triangles.as_chunks::<3>().0 {
        let Some(n) = normal(vertices, triangle) else {
            continue;
        };
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            if a != b {
                faces.entry((a.min(b), a.max(b))).or_default().push(n);
            }
        }
    }
    let mut edges: Vec<_> = faces
        .into_iter()
        .filter(|(_, normals)| match normals.as_slice() {
            [a, b] => a[0] * b[0] + a[1] * b[1] + a[2] * b[2] < FEATURE_COS,
            _ => true,
        })
        .map(|(edge, _)| edge)
        .collect();
    // Stable output keeps uploads and tests deterministic.
    edges.sort_unstable();
    edges.into_iter().flat_map(|(a, b)| [a, b]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: f32, y: f32, z: f32) -> Vertex3d {
        Vertex3d {
            position: [x, y, z, 1.0],
            normal: [0.0; 4],
            owner: [1, 0, 0, 0],
        }
    }

    #[test]
    fn flat_quad_keeps_its_border_but_not_its_diagonal() {
        let vertices = [
            vertex(0.0, 0.0, 0.0),
            vertex(1.0, 0.0, 0.0),
            vertex(1.0, 1.0, 0.0),
            vertex(0.0, 1.0, 0.0),
        ];
        let edges = feature_edges(&vertices, &[0, 1, 2, 0, 2, 3]);
        assert_eq!(edges.len(), 8);
        assert!(!edges.chunks(2).any(|e| e == [0, 2]));
    }

    #[test]
    fn folded_faces_keep_the_fold() {
        let vertices = [
            vertex(0.0, 0.0, 0.0),
            vertex(1.0, 0.0, 0.0),
            vertex(0.0, 1.0, 0.0),
            vertex(0.0, 0.0, 1.0),
        ];
        let edges = feature_edges(&vertices, &[0, 1, 2, 0, 3, 1]);
        assert!(edges.chunks(2).any(|e| e == [0, 1]));
    }
}
