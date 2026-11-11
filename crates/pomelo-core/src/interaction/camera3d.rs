//! Orthographic orbit camera. Screen coordinates are viewport-local logical pixels.

use crate::geometry3d::Bounds3;

/// Camera-to-world quaternion is [x, y, z, w]; identity looks along world -Z.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera3d {
    pivot: [f64; 3],
    orientation: [f64; 4],
    view_height: f64,
}

/// Standard viewing directions. Side and oblique views keep board +Z up on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraView3d {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
    Isometric,
    Dimetric,
    Trimetric,
}

impl CameraView3d {
    pub const ALL: [Self; 9] = [
        Self::Top,
        Self::Bottom,
        Self::Front,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::Isometric,
        Self::Dimetric,
        Self::Trimetric,
    ];

    pub fn orientation(self) -> [f64; 4] {
        const Z_UP: [f64; 3] = [0.0, 0.0, 1.0];
        match self {
            Self::Top => [0.0, 0.0, 0.0, 1.0],
            Self::Bottom => [0.0, 1.0, 0.0, 0.0],
            Self::Front => frame([0.0, -1.0, 0.0], Z_UP),
            Self::Back => frame([0.0, 1.0, 0.0], Z_UP),
            Self::Left => frame([-1.0, 0.0, 0.0], Z_UP),
            Self::Right => frame([1.0, 0.0, 0.0], Z_UP),
            Self::Isometric => {
                let (sy, cy) = (std::f64::consts::FRAC_PI_4 * 0.5).sin_cos();
                let (sp, cp) = ((1.0_f64 / 3.0_f64.sqrt()).asin() * 0.5).sin_cos();
                product([0.0, sy, 0.0, cy], [sp, 0.0, 0.0, cp])
            }
            // Two equally foreshortened axes, viewed from the front right.
            Self::Dimetric => frame(from_azimuth(45.0, 20.705), Z_UP),
            // Three different foreshortenings.
            Self::Trimetric => frame(from_azimuth(30.0, 25.0), Z_UP),
        }
    }
}

/// Direction toward the camera for an azimuth measured from front (-Y) toward +X.
fn from_azimuth(azimuth_deg: f64, elevation_deg: f64) -> [f64; 3] {
    let (sa, ca) = azimuth_deg.to_radians().sin_cos();
    let (se, ce) = elevation_deg.to_radians().sin_cos();
    [ce * sa, -ce * ca, se]
}

/// Camera-to-world rotation whose back axis is `back` and whose up axis is `up`
/// made orthogonal to it.
fn frame(back: [f64; 3], up: [f64; 3]) -> [f64; 4] {
    let length = |v: [f64; 3]| dot(v, v).sqrt();
    let b = back.map(|v| v / length(back));
    let projected = std::array::from_fn::<_, 3, _>(|i| up[i] - b[i] * dot(up, b));
    let u = projected.map(|v| v / length(projected));
    let r = cross(u, b);
    // Columns of the rotation matrix are right, up and back.
    let m = |row: usize, column: usize| [r, u, b][column][row];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m(2, 1) - m(1, 2)) / s,
            (m(0, 2) - m(2, 0)) / s,
            (m(1, 0) - m(0, 1)) / s,
            0.25 * s,
        ]
    } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
        let s = (1.0 + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.0;
        [
            0.25 * s,
            (m(0, 1) + m(1, 0)) / s,
            (m(0, 2) + m(2, 0)) / s,
            (m(2, 1) - m(1, 2)) / s,
        ]
    } else if m(1, 1) > m(2, 2) {
        let s = (1.0 + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.0;
        [
            (m(0, 1) + m(1, 0)) / s,
            0.25 * s,
            (m(1, 2) + m(2, 1)) / s,
            (m(0, 2) - m(2, 0)) / s,
        ]
    } else {
        let s = (1.0 + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.0;
        [
            (m(0, 2) + m(2, 0)) / s,
            (m(1, 2) + m(2, 1)) / s,
            0.25 * s,
            (m(1, 0) - m(0, 1)) / s,
        ]
    };
    normalized(q).unwrap_or([0.0, 0.0, 0.0, 1.0])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Shortest-arc interpolation between two orientations, `t` in 0..=1.
pub fn slerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let mut cosine: f64 = a.iter().zip(b).map(|(a, b)| a * b).sum();
    let b = if cosine < 0.0 {
        cosine = -cosine;
        b.map(|v| -v)
    } else {
        b
    };
    let t = t.clamp(0.0, 1.0);
    let (wa, wb) = if cosine > 1.0 - 1e-9 {
        (1.0 - t, t)
    } else {
        let angle = cosine.clamp(-1.0, 1.0).acos();
        let sine = angle.sin();
        (((1.0 - t) * angle).sin() / sine, (t * angle).sin() / sine)
    };
    let q = std::array::from_fn(|i| a[i] * wa + b[i] * wb);
    normalized(q).unwrap_or(b)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickRay3d {
    pub origin: [f64; 3],
    pub direction: [f64; 3],
    /// Valid ray interval is 0..length, matching the rendered clipping range.
    pub length: f64,
}

impl Default for Camera3d {
    fn default() -> Self {
        Self {
            pivot: [0.0; 3],
            orientation: [0.0, 0.0, 0.0, 1.0],
            view_height: 100.0,
        }
    }
}

impl Camera3d {
    pub fn new(pivot: [f64; 3], orientation: [f64; 4], view_height: f64) -> Option<Self> {
        if !pivot.into_iter().all(f64::is_finite) || !view_height.is_finite() || view_height <= 0.0
        {
            return None;
        }
        Some(Self {
            pivot,
            orientation: normalized(orientation)?,
            view_height,
        })
    }

    pub fn pivot(self) -> [f64; 3] {
        self.pivot
    }
    pub fn orientation(self) -> [f64; 4] {
        self.orientation
    }
    pub fn view_height(self) -> f64 {
        self.view_height
    }

    /// Conservative orthographic XY frustum test. The renderer's depth span encloses the scene.
    pub fn intersects(self, bounds: Bounds3, viewport: [f64; 2]) -> bool {
        if !bounds.is_valid() || !valid_viewport(viewport) {
            return false;
        }
        let center = subtract(bounds.center(), self.pivot);
        let half = std::array::from_fn::<_, 3, _>(|i| (bounds.max[i] - bounds.min[i]) * 0.5);
        let [right, up, _] = self.basis();
        let extent = [
            self.view_height * viewport[0] / viewport[1] * 0.5,
            self.view_height * 0.5,
        ];
        for (axis, extent) in [right, up].into_iter().zip(extent) {
            let radius = (0..3).map(|i| axis[i].abs() * half[i]).sum::<f64>();
            let distance = dot(axis, center).abs();
            // Include float32 instance/vertex rounding at page edges.
            let epsilon = 1e-6 * (radius + distance + extent).max(1.0);
            if distance > extent + radius + epsilon {
                return false;
            }
        }
        true
    }

    /// Change viewing direction without discarding the current focus or zoom.
    pub fn set_view(&mut self, view: CameraView3d) {
        self.orientation = view.orientation();
    }

    pub fn set_orientation(&mut self, orientation: [f64; 4]) -> bool {
        let Some(orientation) = normalized(orientation) else {
            return false;
        };
        self.orientation = orientation;
        true
    }

    /// Re-center on a board point and show `view_height` millimetres vertically.
    pub fn look_at(&mut self, pivot: [f64; 3], view_height: f64) -> bool {
        if !pivot.into_iter().all(f64::is_finite) || !view_height.is_finite() || view_height <= 0.0
        {
            return false;
        }
        self.pivot = pivot;
        self.view_height = view_height;
        true
    }

    /// Screen right, screen up and toward-viewer directions in world space.
    pub fn axes(self) -> [[f64; 3]; 3] {
        self.basis()
    }

    pub fn is_view(self, view: CameraView3d) -> bool {
        self.orientation
            .into_iter()
            .zip(view.orientation())
            .map(|(a, b)| a * b)
            .sum::<f64>()
            .abs()
            > 1.0 - 1e-12
    }

    /// Rotate about current screen up/right axes, without Euler-angle poles.
    pub fn orbit(&mut self, yaw: f64, pitch: f64) -> bool {
        if !yaw.is_finite() || !pitch.is_finite() {
            return false;
        }
        let (sy, cy) = (yaw * 0.5).sin_cos();
        let (sp, cp) = (pitch * 0.5).sin_cos();
        let Some(orientation) = normalized(product(
            product(self.orientation, [0.0, sy, 0.0, cy]),
            [sp, 0.0, 0.0, cp],
        )) else {
            return false;
        };
        self.orientation = orientation;
        true
    }

    /// Move the displayed board by a logical pixel delta. DPI belongs to the host.
    pub fn pan(&mut self, delta: [f64; 2], viewport: [f64; 2]) -> bool {
        if !valid_viewport(viewport) || !delta.into_iter().all(f64::is_finite) {
            return false;
        }
        let [right, up, _] = self.basis();
        let scale = self.view_height / viewport[1];
        if !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        let pivot = std::array::from_fn(|i| {
            self.pivot[i] - right[i] * delta[0] * scale + up[i] * delta[1] * scale
        });
        if !pivot.into_iter().all(f64::is_finite) {
            return false;
        }
        self.pivot = pivot;
        true
    }

    /// A factor above one zooms in, preserving the point on the pivot plane at anchor.
    pub fn zoom_at(&mut self, anchor: [f64; 2], viewport: [f64; 2], factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let Some(before) = self.point_on_pivot_plane(anchor, viewport) else {
            return false;
        };
        let mut next = *self;
        next.view_height /= factor;
        if !next.view_height.is_finite() || next.view_height <= 0.0 {
            return false;
        }
        let Some(after) = next.point_on_pivot_plane(anchor, viewport) else {
            return false;
        };
        next.pivot = std::array::from_fn(|i| next.pivot[i] + (before[i] - after[i]));
        if !next.pivot.into_iter().all(f64::is_finite) {
            return false;
        }
        *self = next;
        true
    }

    /// Fit all eight projected corners; flat boards and point bounds remain navigable.
    pub fn fit(&mut self, bounds: Bounds3, viewport: [f64; 2], padding: f64) -> bool {
        if !bounds.is_valid()
            || !valid_viewport(viewport)
            || !padding.is_finite()
            || padding < 0.0
            || viewport.into_iter().any(|v| v <= 2.0 * padding)
        {
            return false;
        }
        let center = bounds.center();
        let [right, up, _] = self.basis();
        let mut extent = [0.0_f64; 2];
        for corner in bounds.corners() {
            let relative = subtract(corner, center);
            let projected = [dot(right, relative), dot(up, relative)];
            if !projected.into_iter().all(f64::is_finite) {
                return false;
            }
            extent[0] = extent[0].max(projected[0].abs());
            extent[1] = extent[1].max(projected[1].abs());
        }
        let view_height = (2.0 * extent[1] * viewport[1] / (viewport[1] - 2.0 * padding))
            .max(2.0 * extent[0] * viewport[1] / (viewport[0] - 2.0 * padding))
            .max(1e-6);
        if !view_height.is_finite() {
            return false;
        }
        self.pivot = center;
        self.view_height = view_height;
        true
    }

    pub fn point_on_pivot_plane(self, pixel: [f64; 2], viewport: [f64; 2]) -> Option<[f64; 3]> {
        if !valid_viewport(viewport) || !pixel.into_iter().all(f64::is_finite) {
            return None;
        }
        let [right, up, _] = self.basis();
        let scale = self.view_height / viewport[1];
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let point = std::array::from_fn(|i| {
            self.pivot[i] + right[i] * (pixel[0] - viewport[0] * 0.5) * scale
                - up[i] * (pixel[1] - viewport[1] * 0.5) * scale
        });
        point.into_iter().all(f64::is_finite).then_some(point)
    }

    pub fn pick_ray(
        self,
        pixel: [f64; 2],
        viewport: [f64; 2],
        bounds: Bounds3,
    ) -> Option<PickRay3d> {
        let point = self.point_on_pivot_plane(pixel, viewport)?;
        let length = self.depth_span(bounds)?;
        let back = self.basis()[2];
        let origin = std::array::from_fn(|i| point[i] + back[i] * length * 0.5);
        origin.into_iter().all(f64::is_finite).then_some(PickRay3d {
            origin,
            direction: back.map(|v| -v),
            length,
        })
    }

    /// Column-major transform for positions relative to origin, with GPU depth in 0..1.
    /// A local origin avoids subtracting large board coordinates in float32 shaders.
    pub fn clip_matrix(
        self,
        viewport: [f64; 2],
        bounds: Bounds3,
        origin: [f64; 3],
    ) -> Option<[[f32; 4]; 4]> {
        if !valid_viewport(viewport) || !origin.into_iter().all(f64::is_finite) {
            return None;
        }
        let [right, up, back] = self.basis();
        let depth = self.depth_span(bounds)?;
        let sx = 2.0 / self.view_height * viewport[1] / viewport[0];
        let sy = 2.0 / self.view_height;
        if [sx, sy, 1.0 / depth]
            .into_iter()
            .any(|v| !(v as f32).is_finite() || v as f32 <= 0.0)
        {
            return None;
        }
        let offset = subtract(origin, self.pivot);
        let matrix = [
            [right[0] * sx, up[0] * sy, -back[0] / depth, 0.0],
            [right[1] * sx, up[1] * sy, -back[1] / depth, 0.0],
            [right[2] * sx, up[2] * sy, -back[2] / depth, 0.0],
            [
                dot(right, offset) * sx,
                dot(up, offset) * sy,
                0.5 - dot(back, offset) / depth,
                1.0,
            ],
        ]
        .map(|column| column.map(|value| value as f32));
        matrix
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(matrix)
    }

    fn depth_span(self, bounds: Bounds3) -> Option<f64> {
        if !bounds.is_valid() {
            return None;
        }
        let back = self.basis()[2];
        let mut extent = 0.0_f64;
        for corner in bounds.corners() {
            let projected = dot(back, subtract(corner, self.pivot));
            if !projected.is_finite() {
                return None;
            }
            extent = extent.max(projected.abs());
        }
        let span = (extent * 2.02).max(1e-3);
        span.is_finite().then_some(span)
    }

    fn basis(self) -> [[f64; 3]; 3] {
        let [x, y, z, w] = self.orientation;
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y + z * w),
                2.0 * (x * z - y * w),
            ],
            [
                2.0 * (x * y - z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z + x * w),
            ],
            [
                2.0 * (x * z + y * w),
                2.0 * (y * z - x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }
}

fn valid_viewport(viewport: [f64; 2]) -> bool {
    viewport.into_iter().all(|v| v.is_finite() && v > 0.0)
}
fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn normalized(q: [f64; 4]) -> Option<[f64; 4]> {
    if !q.into_iter().all(f64::is_finite) {
        return None;
    }
    let scale = q.into_iter().map(f64::abs).fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return None;
    }
    let q = q.map(|v| v / scale);
    let length = q.into_iter().map(|v| v * v).sum::<f64>().sqrt();
    Some(q.map(|v| v / length))
}
fn product(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [x, y, z, w] = a;
    let [u, v, s, t] = b;
    [
        w * u + x * t + y * s - z * v,
        w * v - x * s + y * t + z * u,
        w * s + x * v - y * u + z * t,
        w * t - x * u - y * v - z * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: [f64; 2] = [1200.0, 800.0];
    const BOARD: Bounds3 = Bounds3 {
        min: [-50.0, -30.0, -0.8],
        max: [50.0, 30.0, 0.8],
    };

    fn close(a: [f64; 3], b: [f64; 3]) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-9, "{a:?} != {b:?}");
        }
    }
    fn clip(camera: Camera3d, p: [f64; 3], bounds: Bounds3) -> [f64; 3] {
        let origin = camera.pivot();
        let matrix = camera.clip_matrix(VIEW, bounds, origin).unwrap();
        let p = subtract(p, origin);
        std::array::from_fn(|row| {
            (0..3)
                .map(|col| f64::from(matrix[col][row]) * p[col])
                .sum::<f64>()
                + f64::from(matrix[3][row])
        })
    }

    #[test]
    fn frustum_rejects_distant_pages_but_keeps_crossing_and_rotated_bounds() {
        let camera = Camera3d::new([0.0; 3], [0.0, 0.0, 0.0, 1.0], 10.0).unwrap();
        assert!(!camera.intersects(
            Bounds3 {
                min: [20.0, 20.0, -1.0],
                max: [30.0, 30.0, 1.0]
            },
            [100.0; 2]
        ));
        assert!(camera.intersects(
            Bounds3 {
                min: [-100.0, -0.1, -1.0],
                max: [100.0, 0.1, 1.0]
            },
            [100.0; 2]
        ));
        assert!(camera.intersects(
            Bounds3 {
                min: [5.0, 0.0, 0.0],
                max: [6.0, 1.0, 1.0]
            },
            [100.0; 2]
        ));
        for view in [
            CameraView3d::Top,
            CameraView3d::Bottom,
            CameraView3d::Isometric,
        ] {
            let mut camera = camera;
            camera.set_view(view);
            assert!(camera.fit(BOARD, VIEW, 20.0));
            assert!(camera.intersects(BOARD, VIEW));
            for corner in BOARD.corners() {
                assert!(camera.intersects(
                    Bounds3 {
                        min: corner,
                        max: corner
                    },
                    VIEW
                ));
            }
        }
        assert!(!camera.intersects(BOARD, [0.0; 2]));
    }

    #[test]
    fn side_and_oblique_views_keep_board_up_and_look_toward_the_board() {
        let up = |view: CameraView3d| {
            let mut camera = Camera3d::default();
            camera.set_view(view);
            camera.axes()
        };
        for (view, back) in [
            (CameraView3d::Front, [0.0, -1.0, 0.0]),
            (CameraView3d::Back, [0.0, 1.0, 0.0]),
            (CameraView3d::Left, [-1.0, 0.0, 0.0]),
            (CameraView3d::Right, [1.0, 0.0, 0.0]),
        ] {
            let [right, screen_up, toward] = up(view);
            close(toward, back);
            close(screen_up, [0.0, 0.0, 1.0]);
            close(cross(screen_up, toward), right);
        }
        for view in [CameraView3d::Dimetric, CameraView3d::Trimetric] {
            let [_, screen_up, toward] = up(view);
            assert!(toward[2] > 0.0 && screen_up[2] > 0.0);
            assert!(dot(screen_up, toward).abs() < 1e-9);
        }
        let mut distinct = 0;
        for (i, a) in CameraView3d::ALL.iter().enumerate() {
            let mut camera = Camera3d::default();
            camera.set_view(*a);
            assert!(camera.is_view(*a));
            distinct += CameraView3d::ALL[i + 1..]
                .iter()
                .filter(|b| !camera.is_view(**b))
                .count();
        }
        assert_eq!(distinct, 9 * 8 / 2);
    }

    #[test]
    fn slerp_reaches_both_ends_and_stays_normalized() {
        let a = CameraView3d::Top.orientation();
        let b = CameraView3d::Front.orientation();
        let close4 = |x: [f64; 4], y: [f64; 4]| {
            let d: f64 = x.iter().zip(y).map(|(x, y)| x * y).sum();
            assert!(d.abs() > 1.0 - 1e-9);
        };
        close4(slerp(a, b, 0.0), a);
        close4(slerp(a, b, 1.0), b);
        let mid = slerp(a, b, 0.5);
        assert!((mid.iter().map(|v| v * v).sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn view_presets_preserve_focus_and_match_physical_directions() {
        let mut camera = Camera3d::new([3.0, 4.0, 5.0], [0.0, 0.0, 0.0, 1.0], 20.0).unwrap();
        let d = 1.0 / 3.0_f64.sqrt();
        for (view, direction) in [
            (CameraView3d::Top, [0.0, 0.0, -1.0]),
            (CameraView3d::Bottom, [0.0, 0.0, 1.0]),
            (CameraView3d::Isometric, [-d, d, -d]),
        ] {
            camera.set_view(view);
            assert!(camera.is_view(view));
            assert_eq!(camera.pivot(), [3.0, 4.0, 5.0]);
            assert_eq!(camera.view_height(), 20.0);
            close(
                camera
                    .pick_ray([600.0, 400.0], VIEW, BOARD)
                    .unwrap()
                    .direction,
                direction,
            );
            let mut fitted = camera;
            assert!(fitted.fit(BOARD, VIEW, 40.0));
            for corner in BOARD.corners() {
                let projected = clip(fitted, corner, BOARD);
                assert!(projected[0].abs() < 1.0 && projected[1].abs() < 1.0);
            }
            camera.orbit(0.1, 0.0);
            assert!(!camera.is_view(view));
        }
    }

    #[test]
    fn fit_projects_every_corner_inside_padding_at_top_bottom_and_oblique_angles() {
        for (yaw, pitch) in [(0.0, 0.0), (0.0, std::f64::consts::PI), (0.7, 1.1)] {
            let mut camera = Camera3d::default();
            assert!(camera.orbit(yaw, pitch));
            assert!(camera.fit(BOARD, VIEW, 40.0));
            for p in BOARD.corners() {
                let p = clip(camera, p, BOARD);
                assert!(p[0].abs() <= 1.0 - 80.0 / VIEW[0] + 1e-6);
                assert!(p[1].abs() <= 1.0 - 80.0 / VIEW[1] + 1e-6);
                assert!(p[2] > 0.0 && p[2] < 1.0);
            }
        }
    }

    #[test]
    fn zoom_preserves_cursor_anchor_after_orbit_and_pan() {
        let mut camera = Camera3d::default();
        camera.orbit(0.7, 1.1);
        assert!(camera.pan([35.0, -40.0], VIEW));
        let pixel = [913.0, 222.0];
        let before = camera.point_on_pivot_plane(pixel, VIEW).unwrap();
        assert!(camera.zoom_at(pixel, VIEW, 3.5));
        assert!((camera.view_height() - 100.0 / 3.5).abs() < 1e-12);
        close(before, camera.point_on_pivot_plane(pixel, VIEW).unwrap());
    }

    #[test]
    fn pan_moves_board_by_requested_pixels() {
        let mut camera = Camera3d::default();
        camera.orbit(0.7, 1.1);
        let p = camera.pivot();
        assert!(camera.pan([90.0, -60.0], VIEW));
        let p = clip(camera, p, BOARD);
        assert!((p[0] * VIEW[0] / 2.0 - 90.0).abs() < 1e-4);
        assert!((-p[1] * VIEW[1] / 2.0 + 60.0).abs() < 1e-4);
    }

    #[test]
    fn picking_ray_and_rendered_depth_agree_from_both_sides() {
        for pitch in [0.0, std::f64::consts::PI, 0.85] {
            let mut camera = Camera3d::default();
            camera.orbit(0.4, pitch);
            camera.fit(BOARD, VIEW, 40.0);
            let pixel = [700.0, 300.0];
            let ray = camera.pick_ray(pixel, VIEW, BOARD).unwrap();
            for t in [0.0, 0.5, 1.0] {
                let p = std::array::from_fn(|i| ray.origin[i] + ray.direction[i] * ray.length * t);
                let p = clip(camera, p, BOARD);
                assert!((p[0] - (pixel[0] / VIEW[0] * 2.0 - 1.0)).abs() < 1e-6);
                assert!((p[1] - (1.0 - pixel[1] / VIEW[1] * 2.0)).abs() < 1e-6);
                assert!((p[2] - t).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn top_and_bottom_look_toward_opposite_board_surfaces() {
        let mut camera = Camera3d::default();
        let ray = camera.pick_ray([600.0, 400.0], VIEW, BOARD).unwrap();
        close(ray.direction, [0.0, 0.0, -1.0]);
        assert!(ray.origin[2] > BOARD.max[2]);
        assert!(camera.orbit(0.0, std::f64::consts::PI));
        let ray = camera.pick_ray([600.0, 400.0], VIEW, BOARD).unwrap();
        close(ray.direction, [0.0, 0.0, 1.0]);
        assert!(ray.origin[2] < BOARD.min[2]);
    }

    #[test]
    fn projection_rejects_float32_underflow_and_overflow() {
        for height in [1e-300, 1e300] {
            let camera = Camera3d::new([0.0; 3], [0.0, 0.0, 0.0, 1.0], height).unwrap();
            assert!(camera.clip_matrix(VIEW, BOARD, [0.0; 3]).is_none());
        }
    }

    #[test]
    fn repeated_orbit_keeps_unit_quaternion_and_large_origin_stays_precise() {
        let mut camera = Camera3d::new([1e9, -1e9, 0.0], [0.0, 0.0, 0.0, 1e300], 1.0).unwrap();
        for _ in 0..10_000 {
            assert!(camera.orbit(0.01, 0.02));
        }
        let norm = camera.orientation().into_iter().map(|v| v * v).sum::<f64>();
        assert!((norm - 1.0).abs() < 1e-12);
        let matrix = camera
            .clip_matrix(
                VIEW,
                Bounds3 {
                    min: [1e9 - 1.0, -1e9 - 1.0, -1.0],
                    max: [1e9 + 1.0, -1e9 + 1.0, 1.0],
                },
                camera.pivot(),
            )
            .unwrap();
        assert_eq!(matrix[3], [0.0, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn invalid_input_does_not_change_camera() {
        let mut camera = Camera3d::default();
        let before = camera;
        assert!(!camera.orbit(f64::NAN, 0.0));
        assert!(!camera.pan([f64::INFINITY, 0.0], VIEW));
        assert!(!camera.zoom_at([0.0; 2], VIEW, 0.0));
        assert!(!camera.fit(BOARD, [0.0, 800.0], 0.0));
        assert!(!camera.fit(BOARD, VIEW, 401.0));
        assert!(camera.pick_ray([0.0; 2], [0.0; 2], BOARD).is_none());
        assert!(camera.clip_matrix(VIEW, BOARD, [f64::NAN; 3]).is_none());
        assert!(Camera3d::new([0.0; 3], [0.0; 4], 100.0).is_none());
        assert_eq!(camera, before);
        assert!(camera.fit(
            Bounds3 {
                min: [0.0; 3],
                max: [0.0; 3]
            },
            VIEW,
            0.0
        ));
    }
}
