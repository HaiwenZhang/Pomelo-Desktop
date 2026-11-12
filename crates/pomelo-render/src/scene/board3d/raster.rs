//! CPU image of the 3D view for copying and export. It follows the GPU path:
//! the same instance transform and height map, flat lighting, opaque faces with
//! depth, weighted blended transparency and feature-edge lines.
use super::{PreparedBoard3d, ZMap};
use pomelo_core::{
    interaction::camera3d::Camera3d,
    task::{CancellationToken, Cancelled},
};

/// Largest image side the rasterizer accepts.
pub const MAX_SIDE: u32 = 8192;

pub struct RasterRequest<'a> {
    pub camera: Camera3d,
    pub z_map: ZMap,
    /// Face materials then edge materials, as given to the GPU.
    pub materials: &'a [[f32; 4]],
    pub size: [u32; 2],
    pub background: [f32; 4],
}

struct Target {
    width: usize,
    height: usize,
    color: Vec<[f32; 4]>,
    depth: Vec<f32>,
    accum: Vec<[f32; 4]>,
    reveal: Vec<f32>,
}

fn lit(color: [f32; 4], normal: [f32; 3]) -> [f32; 3] {
    let light = {
        let l = [0.3_f32, -0.5, 1.0];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    let diffuse = if length > f32::EPSILON {
        ((normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]) / length).abs()
    } else {
        1.0
    };
    let k = 0.35 + 0.65 * diffuse;
    [color[0] * k, color[1] * k, color[2] * k]
}

impl Target {
    fn triangle(&mut self, p: [[f32; 3]; 3], color: [f32; 4], normal: [f32; 3]) {
        let area =
            (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if !area.is_finite() || area.abs() < 1e-12 {
            return;
        }
        let min_x = p
            .iter()
            .map(|v| v[0])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_x = p
            .iter()
            .map(|v| v[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.width as f32) as usize;
        let min_y = p
            .iter()
            .map(|v| v[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_y = p
            .iter()
            .map(|v| v[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.height as f32) as usize;
        if min_x >= max_x || min_y >= max_y {
            return;
        }
        let shade = lit(color, normal);
        let opaque = color[3] >= 0.999;
        for y in min_y..max_y {
            let py = y as f32 + 0.5;
            for x in min_x..max_x {
                let px = x as f32 + 0.5;
                let w0 = ((p[1][0] - px) * (p[2][1] - py) - (p[2][0] - px) * (p[1][1] - py)) / area;
                let w1 = ((p[2][0] - px) * (p[0][1] - py) - (p[0][0] - px) * (p[2][1] - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * p[0][2] + w1 * p[1][2] + w2 * p[2][2];
                if !(0.0..=1.0).contains(&z) {
                    continue;
                }
                let i = y * self.width + x;
                if z > self.depth[i] {
                    continue;
                }
                if opaque {
                    self.depth[i] = z;
                    self.color[i] = [shade[0], shade[1], shade[2], 1.0];
                } else {
                    let weight = 0.1 + 8.0 * (1.0 - z).powi(3);
                    let a = color[3];
                    let acc = &mut self.accum[i];
                    acc[0] += shade[0] * a * weight;
                    acc[1] += shade[1] * a * weight;
                    acc[2] += shade[2] * a * weight;
                    acc[3] += a * weight;
                    self.reveal[i] *= 1.0 - a;
                }
            }
        }
    }

    fn line(&mut self, a: [f32; 3], b: [f32; 3], color: [f32; 4]) {
        let steps = (b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil().max(1.0) as usize;
        if steps > 1 << 16 {
            return;
        }
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let x = a[0] + (b[0] - a[0]) * t;
            let y = a[1] + (b[1] - a[1]) * t;
            let z = a[2] + (b[2] - a[2]) * t - 0.0004;
            if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
                continue;
            }
            let i = y as usize * self.width + x as usize;
            if z <= self.depth[i] {
                self.color[i] = [color[0], color[1], color[2], 1.0];
            }
        }
    }
}

/// Straight-alpha RGBA8 pixels of the view, top row first.
pub fn rasterize(
    scene: &PreparedBoard3d,
    request: &RasterRequest<'_>,
    cancel: &CancellationToken,
) -> Result<Option<Vec<u8>>, Cancelled> {
    let [w, h] = request.size;
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Ok(None);
    }
    let count = scene.sources.len() + 1;
    if request.materials.len() < count {
        return Ok(None);
    }
    let viewport = [f64::from(w), f64::from(h)];
    let Some(matrix) = request.camera.clip_matrix(
        viewport,
        request.z_map.bounds(scene.bounds),
        [scene.origin.x, scene.origin.y, 0.0],
    ) else {
        return Ok(None);
    };
    let (width, height) = (w as usize, h as usize);
    let mut target = Target {
        width,
        height,
        color: vec![[0.0; 4]; width * height],
        depth: vec![1.0; width * height],
        accum: vec![[0.0; 4]; width * height],
        reveal: vec![1.0; width * height],
    };
    let pixels_per_mm = (f64::from(h) / request.camera.view_height()) as f32;
    let project = |p: [f32; 3]| -> [f32; 3] {
        let clip: [f32; 4] = std::array::from_fn(|row| {
            matrix[0][row] * p[0] + matrix[1][row] * p[1] + matrix[2][row] * p[2] + matrix[3][row]
        });
        let w = if clip[3].abs() > f32::EPSILON {
            clip[3]
        } else {
            1.0
        };
        [
            (clip[0] / w * 0.5 + 0.5) * width as f32,
            (0.5 - clip[1] / w * 0.5) * height as f32,
            clip[2] / w,
        ]
    };
    let edge_materials = request.materials.get(count..);
    let transform = |vertex: &super::Vertex3d, instance: &super::Instance3d| {
        let [tx, ty, c, s] = instance.transform;
        let mut x = vertex.position[0];
        if instance.owner[1] == 1 {
            x += x.signum() * f32::from_bits(instance.owner[2]);
        }
        let z = request.z_map.apply(f64::from(vertex.position[2])) as f32;
        let local = [
            c * x - s * vertex.position[1],
            s * x + c * vertex.position[1],
            z,
        ];
        (
            [local[0] + tx, local[1] + ty, local[2]],
            if instance.owner[0] != 0 {
                instance.owner[0]
            } else {
                vertex.owner[0]
            },
        )
    };
    for (page_index, page) in scene.pages.iter().enumerate() {
        if page_index % 16 == 0 {
            cancel.check()?;
        }
        let mesh = &scene.meshes[page.mesh];
        let range = mesh.draw_indices(pixels_per_mm);
        let triangles = &mesh.indices[range.start as usize..range.end as usize];
        for instance in &page.instances {
            for triangle in triangles.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (
                    mesh.vertices.get(triangle[0] as usize),
                    mesh.vertices.get(triangle[1] as usize),
                    mesh.vertices.get(triangle[2] as usize),
                ) else {
                    continue;
                };
                let (pa, owner) = transform(a, instance);
                let (pb, _) = transform(b, instance);
                let (pc, _) = transform(c, instance);
                let Some(&color) = request.materials.get(owner as usize) else {
                    continue;
                };
                if color[3] <= 0.0 {
                    continue;
                }
                let u = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
                let v = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
                let normal = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                // Opaque faces first, then transparency tests against their depth.
                if color[3] >= 0.999 {
                    target.triangle([project(pa), project(pb), project(pc)], color, normal);
                }
            }
        }
    }
    for page in &scene.pages {
        cancel.check()?;
        let mesh = &scene.meshes[page.mesh];
        let range = mesh.draw_indices(pixels_per_mm);
        let triangles = &mesh.indices[range.start as usize..range.end as usize];
        for instance in &page.instances {
            for triangle in triangles.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (
                    mesh.vertices.get(triangle[0] as usize),
                    mesh.vertices.get(triangle[1] as usize),
                    mesh.vertices.get(triangle[2] as usize),
                ) else {
                    continue;
                };
                let (pa, owner) = transform(a, instance);
                let Some(&color) = request.materials.get(owner as usize) else {
                    continue;
                };
                if color[3] <= 0.0 || color[3] >= 0.999 {
                    continue;
                }
                let (pb, _) = transform(b, instance);
                let (pc, _) = transform(c, instance);
                let u = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
                let v = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
                let normal = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                target.triangle([project(pa), project(pb), project(pc)], color, normal);
            }
            if let Some(edges) = edge_materials
                && !mesh.edges.is_empty()
            {
                let lines = &mesh.indices[mesh.edges.start as usize..mesh.edges.end as usize];
                for line in lines.as_chunks::<2>().0 {
                    let (Some(a), Some(b)) = (
                        mesh.vertices.get(line[0] as usize),
                        mesh.vertices.get(line[1] as usize),
                    ) else {
                        continue;
                    };
                    let (pa, owner) = transform(a, instance);
                    let Some(&color) = edges.get(owner as usize) else {
                        continue;
                    };
                    if color[3] <= 0.0 {
                        continue;
                    }
                    let (pb, _) = transform(b, instance);
                    target.line(project(pa), project(pb), color);
                }
            }
        }
    }
    let bg = request.background;
    let mut out = Vec::with_capacity(width * height * 4);
    for i in 0..width * height {
        let base = if target.depth[i] < 1.0 {
            target.color[i]
        } else {
            bg
        };
        let alpha = 1.0 - target.reveal[i].clamp(0.0, 1.0);
        let acc = target.accum[i];
        let mixed: [f32; 3] = if acc[3] > 1e-5 {
            std::array::from_fn(|c| acc[c] / acc[3] * alpha + base[c] * (1.0 - alpha))
        } else {
            [base[0], base[1], base[2]]
        };
        let a = (alpha + base[3] * (1.0 - alpha)).clamp(0.0, 1.0);
        out.extend(mixed.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        out.push((a * 255.0).round() as u8);
    }
    Ok(Some(out))
}
