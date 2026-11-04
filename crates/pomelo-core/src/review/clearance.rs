//! Shortest 2D distance between the copper geometry of two source objects.
//!
//! Strokes are capsules (round-cap centrelines with a radius); filled areas are regions
//! with their own exact containment test (even-odd rings, zone fill mesh or custom pad
//! contours). Curves are sampled with a bounded chord error that is reported with the
//! result. Overlap and contact return zero. This is geometry, not a design-rule check.
use crate::{
    display::{BoardDisplay, LayerPrimitive},
    geometry::{PathError, flatten_path},
    model::{BoardScene, Bounds, Pad, Point, Segment},
    pad::{PadOutline, PadPlacement},
    selection::SelectedObject,
    task::CancellationToken,
};

/// Chord error used for arcs, circles and rounded corners, in millimetres.
pub const DEFAULT_TOLERANCE_MM: f64 = 0.0005;
/// Element-pair evaluations allowed for one query before it reports a limit.
pub const DEFAULT_PAIR_BUDGET: u64 = 200_000_000;
/// Gaps below a picometre are floating-point contact, not clearance.
const CONTACT_EPSILON_MM: f64 = 1e-9;
const MAX_REGION_POINTS: usize = 4_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClearanceError {
    #[error("CLEARANCE_CANCELLED")]
    Cancelled,
    #[error("CLEARANCE_SAME_OBJECT")]
    SameObject,
    #[error("CLEARANCE_MISSING {0:?}")]
    Missing(SelectedObject),
    /// The object has no supported copper geometry (for example an unsupported pad family).
    #[error("CLEARANCE_UNSUPPORTED {0:?}")]
    Unsupported(SelectedObject),
    #[error("CLEARANCE_INVALID {0:?}")]
    Invalid(SelectedObject),
    #[error("CLEARANCE_LIMIT")]
    Limit,
}

impl From<crate::task::Cancelled> for ClearanceError {
    fn from(_: crate::task::Cancelled) -> Self {
        Self::Cancelled
    }
}

impl From<PathError> for ClearanceError {
    fn from(error: PathError) -> Self {
        match error {
            PathError::Cancelled => Self::Cancelled,
            PathError::PointLimit { .. } => Self::Limit,
            PathError::Invalid(id) => Self::Invalid(SelectedObject::Segment(id)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearanceResult {
    /// Shortest edge-to-edge distance; zero for touching or overlapping geometry.
    pub distance_mm: f64,
    /// Nearest point on the first object's boundary.
    pub a: Point,
    /// Nearest point on the second object's boundary.
    pub b: Point,
    /// Upper bound of the curve sampling error included in `distance_mm`.
    pub tolerance_mm: f64,
}

impl ClearanceResult {
    pub fn overlapping(&self) -> bool {
        self.distance_mm <= 0.0
    }
}

#[derive(Debug, Clone, Copy)]
struct Capsule {
    a: Point,
    b: Point,
    radius: f64,
}

enum Containment<'a> {
    EvenOdd,
    Zone(&'a crate::copper::CopperMesh),
    CustomPad(&'a Pad, PadPlacement),
}

struct Region<'a> {
    rings: Vec<Vec<Point>>,
    containment: Containment<'a>,
}

impl Region<'_> {
    fn contains(&self, point: Point, cancel: &CancellationToken) -> Result<bool, ClearanceError> {
        let mesh_error = |error| match error {
            crate::copper::MeshError::Cancelled => ClearanceError::Cancelled,
            _ => ClearanceError::Limit,
        };
        match self.containment {
            Containment::EvenOdd => Ok(even_odd_contains(&self.rings, point)),
            Containment::Zone(mesh) => mesh.covers_fill(point, cancel).map_err(mesh_error),
            Containment::CustomPad(pad, owner) => pad
                .custom_covers_fill(point, owner, cancel)
                .map(|covered| covered == Some(true))
                .map_err(mesh_error),
        }
    }
}

/// Clearance geometry of one source object, borrowing exact containment from the scene.
pub struct ClearanceShape<'a> {
    object: SelectedObject,
    capsules: Vec<Capsule>,
    regions: Vec<Region<'a>>,
}

/// One boundary element: a capsule, or a region edge with zero radius.
#[derive(Debug, Clone, Copy)]
struct Element {
    a: Point,
    b: Point,
    radius: f64,
    bounds: Bounds,
}

impl Element {
    fn new(a: Point, b: Point, radius: f64) -> Self {
        let bounds = Bounds {
            min: Point::new(a.x.min(b.x) - radius, a.y.min(b.y) - radius),
            max: Point::new(a.x.max(b.x) + radius, a.y.max(b.y) + radius),
        };
        Self {
            a,
            b,
            radius,
            bounds,
        }
    }
}

impl<'a> ClearanceShape<'a> {
    /// Build the shape on visible layers. Pads and vias use their visible copper
    /// layers, falling back to every supported layer when all of them are hidden.
    pub fn from_object(
        scene: &'a BoardScene,
        object: SelectedObject,
        display: &BoardDisplay,
        tolerance_mm: f64,
        cancel: &CancellationToken,
    ) -> Result<Self, ClearanceError> {
        let mut shape = Self {
            object,
            capsules: Vec::new(),
            regions: Vec::new(),
        };
        match object {
            SelectedObject::Placement(_) => return Err(ClearanceError::Unsupported(object)),
            SelectedObject::Segment(id) => {
                let segment = scene
                    .segments
                    .iter()
                    .find(|segment| segment.id == id)
                    .ok_or(ClearanceError::Missing(object))?;
                shape.add_stroke(segment, tolerance_mm)?;
            }
            SelectedObject::Drawing(id) => {
                let drawing = scene
                    .drawings
                    .iter()
                    .find(|drawing| drawing.id == id)
                    .ok_or(ClearanceError::Missing(object))?;
                for segment in &drawing.segments {
                    cancel.check()?;
                    shape.add_stroke(segment, tolerance_mm)?;
                }
            }
            SelectedObject::Pin(id) => {
                let pin = scene
                    .pins
                    .iter()
                    .find(|pin| pin.id == id)
                    .ok_or(ClearanceError::Missing(object))?;
                let owner = PadPlacement {
                    at: pin.at,
                    angle: pin.angle,
                    mirrored: pin.mirrored,
                };
                shape.add_pads(
                    &pin.pads,
                    owner,
                    LayerPrimitive::Pads,
                    display,
                    tolerance_mm,
                )?;
            }
            SelectedObject::Via(id) => {
                let via = scene
                    .vias
                    .iter()
                    .find(|via| via.id == id)
                    .ok_or(ClearanceError::Missing(object))?;
                let owner = PadPlacement {
                    at: via.at,
                    angle: via.angle,
                    mirrored: via.mirrored,
                };
                shape.add_pads(
                    &via.pads,
                    owner,
                    LayerPrimitive::Vias,
                    display,
                    tolerance_mm,
                )?;
                if shape.is_empty()
                    && let Some(drill) = via.drill_shape.pad()
                {
                    shape.add_drill(&drill, owner, tolerance_mm)?;
                }
            }
            SelectedObject::Zone(id) => {
                let zone = scene
                    .zones
                    .iter()
                    .find(|zone| zone.id == id)
                    .ok_or(ClearanceError::Missing(object))?;
                let mut rings = Vec::with_capacity(zone.paths.len());
                let mut points = 0usize;
                for path in &zone.paths {
                    let ring = flatten_path(
                        path,
                        tolerance_mm,
                        MAX_REGION_POINTS.saturating_sub(points),
                        cancel,
                    )?;
                    points += ring.len();
                    if ring.len() >= 2 {
                        rings.push(ring);
                    }
                }
                if !rings.is_empty() {
                    shape.regions.push(Region {
                        rings,
                        containment: Containment::Zone(&zone.mesh),
                    });
                }
            }
        }
        if shape.is_empty() {
            return Err(ClearanceError::Unsupported(object));
        }
        Ok(shape)
    }

    fn is_empty(&self) -> bool {
        self.capsules.is_empty() && self.regions.is_empty()
    }

    fn add_stroke(&mut self, segment: &Segment, tolerance_mm: f64) -> Result<(), ClearanceError> {
        let radius = segment.width.max(0.0) * 0.5;
        if !radius.is_finite() || !segment.a.is_finite() || !segment.b.is_finite() {
            return Err(ClearanceError::Invalid(self.object));
        }
        let Some(arc) = segment.arc else {
            self.capsules.push(Capsule {
                a: segment.a,
                b: segment.b,
                radius,
            });
            return Ok(());
        };
        if ![arc.center.x, arc.center.y, arc.radius, arc.start, arc.sweep]
            .into_iter()
            .all(f64::is_finite)
            || arc.radius < 0.0
        {
            return Err(ClearanceError::Invalid(self.object));
        }
        let count = crate::pad::curve_samples(arc.radius, arc.sweep.abs(), tolerance_mm);
        let mut previous = segment.a;
        for index in 1..=count {
            let next = if index == count {
                segment.b
            } else {
                let angle = arc.start + arc.sweep * index as f64 / count as f64;
                Point::new(
                    arc.center.x + arc.radius * angle.cos(),
                    arc.center.y + arc.radius * angle.sin(),
                )
            };
            self.capsules.push(Capsule {
                a: previous,
                b: next,
                radius,
            });
            previous = next;
        }
        Ok(())
    }

    fn add_pads(
        &mut self,
        pads: &'a [Pad],
        owner: PadPlacement,
        primitive: LayerPrimitive,
        display: &BoardDisplay,
        tolerance_mm: f64,
    ) -> Result<(), ClearanceError> {
        let copper: Vec<_> = pads
            .iter()
            .filter(|pad| pad.supported() && !pad.backdrill)
            .collect();
        let visible: Vec<_> = copper
            .iter()
            .copied()
            .filter(|pad| display.primitive_visible(pad.layer, primitive))
            .collect();
        for pad in if visible.is_empty() { copper } else { visible } {
            self.add_pad(pad, owner, tolerance_mm)?;
        }
        Ok(())
    }

    fn add_pad(
        &mut self,
        pad: &'a Pad,
        owner: PadPlacement,
        tolerance_mm: f64,
    ) -> Result<(), ClearanceError> {
        let custom = pad.custom.is_some();
        match pad.outline(owner, tolerance_mm) {
            Err(()) => return Err(ClearanceError::Invalid(self.object)),
            Ok(None) => {}
            Ok(Some(PadOutline::Circle { center, radius })) => self.capsules.push(Capsule {
                a: center,
                b: center,
                radius,
            }),
            Ok(Some(PadOutline::Capsule { a, b, radius })) => {
                self.capsules.push(Capsule { a, b, radius })
            }
            Ok(Some(PadOutline::Rings(rings))) => self.regions.push(Region {
                rings,
                containment: if custom {
                    Containment::CustomPad(pad, owner)
                } else {
                    Containment::EvenOdd
                },
            }),
        }
        Ok(())
    }

    /// Drill shapes are built on the fly and never custom, so nothing is borrowed.
    fn add_drill(
        &mut self,
        drill: &Pad,
        owner: PadPlacement,
        tolerance_mm: f64,
    ) -> Result<(), ClearanceError> {
        match drill.outline(owner, tolerance_mm) {
            Ok(Some(PadOutline::Circle { center, radius })) => self.capsules.push(Capsule {
                a: center,
                b: center,
                radius,
            }),
            Ok(Some(PadOutline::Capsule { a, b, radius })) => {
                self.capsules.push(Capsule { a, b, radius })
            }
            Ok(Some(PadOutline::Rings(rings))) => self.regions.push(Region {
                rings,
                containment: Containment::EvenOdd,
            }),
            Ok(None) => {}
            Err(()) => return Err(ClearanceError::Invalid(self.object)),
        }
        Ok(())
    }

    fn elements(&self) -> Vec<Element> {
        let mut elements: Vec<_> = self
            .capsules
            .iter()
            .map(|capsule| Element::new(capsule.a, capsule.b, capsule.radius))
            .collect();
        for region in &self.regions {
            for ring in &region.rings {
                for (index, &point) in ring.iter().enumerate() {
                    let next = ring[(index + 1) % ring.len()];
                    elements.push(Element::new(point, next, 0.0));
                }
            }
        }
        elements
    }

    /// A point that lies on this shape, used to detect full containment.
    fn interior_samples(&self) -> impl Iterator<Item = Point> + '_ {
        self.capsules.iter().map(|capsule| capsule.a).chain(
            self.regions
                .iter()
                .flat_map(|region| region.rings.iter().filter_map(|ring| ring.first().copied())),
        )
    }

    fn covers(&self, point: Point, cancel: &CancellationToken) -> Result<bool, ClearanceError> {
        for capsule in &self.capsules {
            if point_segment_distance(point, capsule.a, capsule.b) <= capsule.radius {
                return Ok(true);
            }
        }
        for region in &self.regions {
            cancel.check()?;
            if region.contains(point, cancel)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Shortest distance between two shapes. Containment of one shape inside the other is
/// detected first; otherwise the minimum boundary-element distance is exact up to the
/// sampling tolerance used to build the shapes.
pub fn clearance(
    first: &ClearanceShape<'_>,
    second: &ClearanceShape<'_>,
    tolerance_mm: f64,
    pair_budget: u64,
    cancel: &CancellationToken,
) -> Result<ClearanceResult, ClearanceError> {
    if first.object == second.object {
        return Err(ClearanceError::SameObject);
    }
    for (inner, outer) in [(first, second), (second, first)] {
        for point in inner.interior_samples() {
            cancel.check()?;
            if outer.covers(point, cancel)? {
                return Ok(ClearanceResult {
                    distance_mm: 0.0,
                    a: point,
                    b: point,
                    tolerance_mm,
                });
            }
        }
    }
    let a_elements = first.elements();
    let mut b_elements = second.elements();
    b_elements.sort_by(|a, b| a.bounds.min.x.total_cmp(&b.bounds.min.x));
    let mut best: Option<(f64, Point, Point)> = None;
    let mut work = 0u64;
    for a in &a_elements {
        let limit = best.map_or(f64::INFINITY, |(distance, _, _)| distance);
        let reach = a.bounds.max.x + limit;
        for b in &b_elements {
            if b.bounds.min.x > reach {
                break;
            }
            work += 1;
            if work > pair_budget {
                return Err(ClearanceError::Limit);
            }
            if work.is_multiple_of(65_536) {
                cancel.check()?;
            }
            let current = best.map_or(f64::INFINITY, |(distance, _, _)| distance);
            if bounds_gap(a.bounds, b.bounds) >= current {
                continue;
            }
            let (p, q) = closest_segment_points(a.a, a.b, b.a, b.b);
            let axis = p.distance(q);
            let gap = axis - a.radius - b.radius;
            // Touching strokes differ from zero only by rounding of their radii.
            let distance = if gap <= CONTACT_EPSILON_MM { 0.0 } else { gap };
            if distance < current {
                let (pa, pb) = if distance == 0.0 {
                    let contact = if axis > 0.0 {
                        // Overlapping strokes: report the point between the two axes.
                        let t = (a.radius / (a.radius + b.radius)).clamp(0.0, 1.0);
                        Point::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t)
                    } else {
                        p
                    };
                    (contact, contact)
                } else {
                    let ux = (q.x - p.x) / axis;
                    let uy = (q.y - p.y) / axis;
                    (
                        Point::new(p.x + ux * a.radius, p.y + uy * a.radius),
                        Point::new(q.x - ux * b.radius, q.y - uy * b.radius),
                    )
                };
                best = Some((distance, pa, pb));
                if distance == 0.0 {
                    return Ok(ClearanceResult {
                        distance_mm: 0.0,
                        a: pa,
                        b: pb,
                        tolerance_mm,
                    });
                }
            }
        }
    }
    cancel.check()?;
    let (distance_mm, a, b) = best.ok_or(ClearanceError::Unsupported(first.object))?;
    Ok(ClearanceResult {
        distance_mm,
        a,
        b,
        tolerance_mm,
    })
}

/// Build both shapes and measure; the common entry point for hosts.
pub fn object_clearance(
    scene: &BoardScene,
    first: SelectedObject,
    second: SelectedObject,
    display: &BoardDisplay,
    cancel: &CancellationToken,
) -> Result<ClearanceResult, ClearanceError> {
    if first == second {
        return Err(ClearanceError::SameObject);
    }
    let a = ClearanceShape::from_object(scene, first, display, DEFAULT_TOLERANCE_MM, cancel)?;
    let b = ClearanceShape::from_object(scene, second, display, DEFAULT_TOLERANCE_MM, cancel)?;
    clearance(&a, &b, DEFAULT_TOLERANCE_MM, DEFAULT_PAIR_BUDGET, cancel)
}

fn bounds_gap(a: Bounds, b: Bounds) -> f64 {
    let dx = (b.min.x - a.max.x).max(a.min.x - b.max.x).max(0.0);
    let dy = (b.min.y - a.max.y).max(a.min.y - b.max.y).max(0.0);
    crate::geometry::web_hypot(dx, dy)
}

fn even_odd_contains(rings: &[Vec<Point>], point: Point) -> bool {
    let mut inside = false;
    for ring in rings {
        for (index, &a) in ring.iter().enumerate() {
            let b = ring[(index + 1) % ring.len()];
            if (a.y > point.y) != (b.y > point.y) {
                let x = a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y);
                if point.x < x {
                    inside = !inside;
                }
            }
        }
    }
    inside
}

fn point_segment_distance(point: Point, a: Point, b: Point) -> f64 {
    point.distance(project(point, a, b))
}

fn project(point: Point, a: Point, b: Point) -> Point {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length = dx * dx + dy * dy;
    if length == 0.0 {
        return a;
    }
    let t = (((point.x - a.x) * dx + (point.y - a.y) * dy) / length).clamp(0.0, 1.0);
    Point::new(a.x + dx * t, a.y + dy * t)
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Closest points between two segments; crossing segments return their intersection.
fn closest_segment_points(p1: Point, q1: Point, p2: Point, q2: Point) -> (Point, Point) {
    let d1 = cross(p2, q2, p1);
    let d2 = cross(p2, q2, q1);
    let d3 = cross(p1, q1, p2);
    let d4 = cross(p1, q1, q2);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        let t = d1 / (d1 - d2);
        let point = Point::new(p1.x + (q1.x - p1.x) * t, p1.y + (q1.y - p1.y) * t);
        return (point, point);
    }
    // Non-crossing segments attain their minimum at an endpoint of one of them.
    [
        (p1, project(p1, p2, q2)),
        (q1, project(q1, p2, q2)),
        (project(p2, p1, q1), p2),
        (project(q2, p1, q1), q2),
    ]
    .into_iter()
    .min_by(|a, b| a.0.distance(a.1).total_cmp(&b.0.distance(b.1)))
    .unwrap_or((p1, p2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerId, NetId, ObjectId, PadKind};

    fn segment(id: u32, a: (f64, f64), b: (f64, f64), width: f64) -> Segment {
        Segment {
            id: ObjectId(id),
            track_id: ObjectId(id),
            layer: LayerId(0),
            net: NetId(1),
            a: Point::new(a.0, a.1),
            b: Point::new(b.0, b.1),
            width,
            arc: None,
            bond_wire: None,
        }
    }

    fn scene(segments: Vec<Segment>) -> BoardScene {
        BoardScene {
            segments,
            ..crate::review::tests::empty_scene()
        }
    }

    fn measure(scene: &BoardScene, a: SelectedObject, b: SelectedObject) -> ClearanceResult {
        object_clearance(
            scene,
            a,
            b,
            &BoardDisplay::default(),
            &CancellationToken::default(),
        )
        .unwrap()
    }

    #[test]
    fn parallel_tracks_subtract_both_half_widths() {
        let scene = scene(vec![
            segment(1, (0.0, 0.0), (10.0, 0.0), 0.2),
            segment(2, (0.0, 1.0), (10.0, 1.0), 0.4),
        ]);
        let result = measure(
            &scene,
            SelectedObject::Segment(ObjectId(1)),
            SelectedObject::Segment(ObjectId(2)),
        );
        assert!((result.distance_mm - 0.7).abs() < 1e-12);
        assert!((result.a.y - 0.1).abs() < 1e-12);
        assert!((result.b.y - 0.8).abs() < 1e-12);
    }

    #[test]
    fn crossing_and_touching_tracks_return_zero() {
        let scene = scene(vec![
            segment(1, (0.0, 0.0), (10.0, 0.0), 0.2),
            segment(2, (5.0, -5.0), (5.0, 5.0), 0.2),
            segment(3, (0.0, 0.4), (10.0, 0.4), 0.6),
        ]);
        let crossing = measure(
            &scene,
            SelectedObject::Segment(ObjectId(1)),
            SelectedObject::Segment(ObjectId(2)),
        );
        assert_eq!(crossing.distance_mm, 0.0);
        assert!(crossing.overlapping());
        let touching = measure(
            &scene,
            SelectedObject::Segment(ObjectId(1)),
            SelectedObject::Segment(ObjectId(3)),
        );
        assert_eq!(touching.distance_mm, 0.0);
    }

    #[test]
    fn end_caps_are_round() {
        let scene = scene(vec![
            segment(1, (0.0, 0.0), (1.0, 0.0), 0.2),
            segment(2, (4.0, 4.0), (4.0, 9.0), 0.2),
        ]);
        let result = measure(
            &scene,
            SelectedObject::Segment(ObjectId(1)),
            SelectedObject::Segment(ObjectId(2)),
        );
        assert!((result.distance_mm - (5.0 - 0.2)).abs() < 1e-12);
    }

    #[test]
    fn rotated_rectangular_pad_uses_its_placement() {
        let mut pad = Pad::circle(LayerId(0), 2.0);
        pad.kind = PadKind(5);
        pad.height = 1.0;
        let pin = crate::model::Pin {
            id: ObjectId(10),
            owner_id: ObjectId(0),
            net: NetId(1),
            name: "1".into(),
            reference: "U1".into(),
            at: Point::new(0.0, 0.0),
            angle: std::f64::consts::FRAC_PI_2,
            mirrored: false,
            drill: 0.0,
            drill_shape: crate::model::DrillShape {
                width: 0.0,
                height: 0.0,
                plated: false,
            },
            pads: vec![pad],
            stackup_region: None,
            die: None,
        };
        let mut board = scene(vec![segment(1, (-5.0, 3.0), (5.0, 3.0), 0.0)]);
        board.pins.push(pin);
        // Rotated by 90 degrees the 2 mm side is vertical: its top edge is at y = 1.
        let result = measure(
            &board,
            SelectedObject::Pin(ObjectId(10)),
            SelectedObject::Segment(ObjectId(1)),
        );
        assert!((result.distance_mm - 2.0).abs() < 1e-9);
    }

    #[test]
    fn donut_hole_is_not_copper() {
        let mut pad = Pad::circle(LayerId(0), 4.0);
        pad.kind = PadKind::DONUT;
        pad.inner_diameter = Some(2.0);
        let via = crate::model::Via {
            id: ObjectId(20),
            net: NetId(1),
            at: Point::new(0.0, 0.0),
            drill: 0.0,
            drill_shape: crate::model::DrillShape {
                width: 0.0,
                height: 0.0,
                plated: false,
            },
            padstack: ObjectId(0),
            padstack_name: String::new(),
            start_layer: None,
            end_layer: None,
            pads: std::sync::Arc::from(vec![pad]),
            backdrill: None,
            stackup_region: None,
            angle: 0.0,
            mirrored: false,
            finger: None,
        };
        // A dot in the hole is 1 mm from the inner edge of the 2 mm hole... minus its radius.
        let mut board = scene(vec![segment(1, (0.0, 0.0), (0.0, 0.0), 0.2)]);
        board.vias.push(via);
        let result = measure(
            &board,
            SelectedObject::Segment(ObjectId(1)),
            SelectedObject::Via(ObjectId(20)),
        );
        assert!((result.distance_mm - 0.9).abs() < 2.0 * DEFAULT_TOLERANCE_MM);
        assert!(result.tolerance_mm > 0.0);
    }

    #[test]
    fn arc_tracks_are_sampled_within_tolerance() {
        let mut arc = segment(1, (1.0, 0.0), (-1.0, 0.0), 0.0);
        arc.arc = Some(crate::model::Arc {
            center: Point::new(0.0, 0.0),
            radius: 1.0,
            start: 0.0,
            sweep: std::f64::consts::PI,
        });
        let board = scene(vec![arc, segment(2, (-3.0, 3.0), (3.0, 3.0), 0.0)]);
        let result = measure(
            &board,
            SelectedObject::Segment(ObjectId(1)),
            SelectedObject::Segment(ObjectId(2)),
        );
        assert!((result.distance_mm - 2.0).abs() <= DEFAULT_TOLERANCE_MM);
    }

    #[test]
    fn same_missing_and_cancelled_queries_are_explicit() {
        let board = scene(vec![segment(1, (0.0, 0.0), (1.0, 0.0), 0.1)]);
        let display = BoardDisplay::default();
        let cancel = CancellationToken::default();
        let one = SelectedObject::Segment(ObjectId(1));
        assert_eq!(
            object_clearance(&board, one, one, &display, &cancel),
            Err(ClearanceError::SameObject)
        );
        let missing = SelectedObject::Pin(ObjectId(99));
        assert_eq!(
            object_clearance(&board, one, missing, &display, &cancel),
            Err(ClearanceError::Missing(missing))
        );
        cancel.cancel();
        assert_eq!(
            object_clearance(
                &scene(vec![
                    segment(1, (0.0, 0.0), (1.0, 0.0), 0.1),
                    segment(2, (0.0, 5.0), (1.0, 5.0), 0.1),
                ]),
                one,
                SelectedObject::Segment(ObjectId(2)),
                &display,
                &cancel
            ),
            Err(ClearanceError::Cancelled)
        );
    }

    #[test]
    fn pair_budget_is_enforced() {
        let board = scene(vec![
            segment(1, (0.0, 0.0), (1.0, 0.0), 0.1),
            segment(2, (0.0, 5.0), (1.0, 5.0), 0.1),
        ]);
        let display = BoardDisplay::default();
        let cancel = CancellationToken::default();
        let a = ClearanceShape::from_object(
            &board,
            SelectedObject::Segment(ObjectId(1)),
            &display,
            DEFAULT_TOLERANCE_MM,
            &cancel,
        )
        .unwrap();
        let b = ClearanceShape::from_object(
            &board,
            SelectedObject::Segment(ObjectId(2)),
            &display,
            DEFAULT_TOLERANCE_MM,
            &cancel,
        )
        .unwrap();
        assert_eq!(
            clearance(&a, &b, DEFAULT_TOLERANCE_MM, 0, &cancel),
            Err(ClearanceError::Limit)
        );
    }

    #[test]
    fn chamfered_rectangle_cuts_its_corners() {
        let mut pad = Pad::circle(LayerId(0), 4.0);
        pad.height = 2.0;
        pad.kind = PadKind(28);
        pad.corner = 0.5;
        let Ok(Some(PadOutline::Rings(rings))) =
            pad.outline(PadPlacement::default(), DEFAULT_TOLERANCE_MM)
        else {
            panic!("chamfered pads are polygons");
        };
        assert_eq!(rings[0].len(), 8);
        assert!(rings[0].contains(&Point::new(1.5, 1.0)));
        assert!(rings[0].contains(&Point::new(2.0, 0.5)));
        assert!(!even_odd_contains(&rings, Point::new(1.95, 0.95)));
    }
}
