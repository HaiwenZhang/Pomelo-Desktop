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

/// Smooth curved walls of an instanced prototype keep this many vertical lines,
/// so a via barrel reads as a cylinder rather than two rings.
const GENERATORS: f32 = 8.0;

/// A smooth wall edge to keep as a generator: vertical, on a near-vertical wall,
/// and close to one of the evenly spaced angles around the prototype's origin.
fn generator(vertices: &[Vertex3d], edge: (u32, u32), normals: &[[f32; 3]]) -> bool {
    let (Some(a), Some(b)) = (vertices.get(edge.0 as usize), vertices.get(edge.1 as usize)) else {
        return false;
    };
    let (a, b) = (a.position, b.position);
    let vertical =
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5 && (a[2] - b[2]).abs() > 1e-6;
    let wall = normals.iter().all(|n| n[2].abs() < 0.2);
    if !vertical || !wall || a[0].hypot(a[1]) < 1e-6 {
        return false;
    }
    let step = std::f32::consts::TAU / GENERATORS;
    let angle = a[1].atan2(a[0]).rem_euclid(step);
    // The turn between the two facets is the wall's angular resolution; keeping
    // edges within half of it selects one edge per generator at any resolution.
    let [n0, n1] = [normals[0], normals[1]];
    let turn = (n0[0] * n1[0] + n0[1] * n1[1] + n0[2] * n1[2])
        .clamp(-1.0, 1.0)
        .acos();
    angle.min(step - angle) <= turn * 0.5 + 1e-4
}

/// Line-list indices of the feature edges of a triangle list. `generators` adds
/// vertical lines on smooth round walls of prototypes centred on their origin.
pub(super) fn feature_edges(
    vertices: &[Vertex3d],
    triangles: &[u32],
    generators: bool,
) -> Vec<u32> {
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
        .filter(|(edge, normals)| match normals.as_slice() {
            [a, b] => {
                a[0] * b[0] + a[1] * b[1] + a[2] * b[2] < FEATURE_COS
                    || (generators && generator(vertices, *edge, normals))
            }
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
        let edges = feature_edges(&vertices, &[0, 1, 2, 0, 2, 3], false);
        assert_eq!(edges.len(), 8);
        assert!(!edges.chunks(2).any(|e| e == [0, 2]));
    }

    #[test]
    fn smooth_cylinder_walls_keep_evenly_spaced_generators() {
        for segments in [36, 48, 64] {
            let mut vertices = Vec::new();
            for i in 0..segments {
                let t = i as f32 / segments as f32 * std::f32::consts::TAU;
                vertices.push(vertex(t.cos(), t.sin(), 0.0));
                vertices.push(vertex(t.cos(), t.sin(), 1.0));
            }
            let mut triangles = Vec::new();
            for i in 0..segments as u32 {
                let (a, b) = (2 * i, 2 * ((i + 1) % segments as u32));
                triangles.extend([a, b, a + 1, b, b + 1, a + 1]);
            }
            let plain = feature_edges(&vertices, &triangles, false);
            let with = feature_edges(&vertices, &triangles, true);
            let vertical = |edges: &[u32]| {
                edges
                    .chunks(2)
                    .filter(|e| {
                        vertices[e[0] as usize].position[2] != vertices[e[1] as usize].position[2]
                    })
                    .count()
            };
            assert_eq!(vertical(&plain), 0);
            // A vertex exactly halfway may serve two neighbouring generators' tolerance.
            let count = vertical(&with);
            assert!(
                (GENERATORS as usize..=2 * GENERATORS as usize).contains(&count),
                "{segments} segments: {count} generators"
            );
        }
    }

    #[test]
    fn folded_faces_keep_the_fold() {
        let vertices = [
            vertex(0.0, 0.0, 0.0),
            vertex(1.0, 0.0, 0.0),
            vertex(0.0, 1.0, 0.0),
            vertex(0.0, 0.0, 1.0),
        ];
        let edges = feature_edges(&vertices, &[0, 1, 2, 0, 3, 1], false);
        assert!(edges.chunks(2).any(|e| e == [0, 1]));
    }
}
