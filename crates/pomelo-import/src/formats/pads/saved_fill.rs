//! Native PADS saved-fill construction, following the reference TS offset/tree rules.
//! Curve tessellation uses a 1 µm chord tolerance rather than the reference's 25 nm.

use crate::ImportError;
use clipper2_rust::{
    ClipType, EndType, FillRule, JoinType, PathD, PathsD, PointD, PolyTreeD, area,
    boolean_op_tree_d, inflate_paths_d, poly_tree_to_paths_d,
};
use pomelo_core::task::CancellationToken;

use std::collections::BTreeMap;

const TOLERANCE: f64 = 0.001;
const PRECISION: i32 = 6;

#[derive(Debug)]
pub struct Contour {
    pub width: f64,
    pub ring: Vec<[f64; 2]>,
}
#[derive(Debug)]
pub struct Thermal {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub width: f64,
}
#[derive(Debug)]
pub struct Fill {
    pub owner: u32,
    pub outer: Contour,
    pub holes: Vec<Contour>,
    pub thermals: Vec<Thermal>,
}
#[derive(Debug)]
pub struct Region {
    pub outer: Vec<[f64; 2]>,
    pub holes: Vec<Vec<[f64; 2]>>,
}

fn error(details: &str) -> ImportError {
    ImportError::Format {
        format: "PADS".into(),
        details: details.into(),
    }
}
fn check(cancel: &CancellationToken) -> Result<(), ImportError> {
    if cancel.is_cancelled() {
        Err(ImportError::Cancelled)
    } else {
        Ok(())
    }
}
fn path(points: &[[f64; 2]]) -> PathD {
    points.iter().map(|p| PointD::new(p[0], p[1])).collect()
}
fn points(path: &PathD) -> Vec<[f64; 2]> {
    path.iter().map(|p| [p.x, p.y]).collect()
}

fn orient(mut paths: PathsD) -> PathsD {
    let largest = paths
        .iter()
        .map(area)
        .max_by(|a, b| a.abs().total_cmp(&b.abs()));
    if largest.is_some_and(|a| a < 0.0) {
        for p in &mut paths {
            p.reverse();
        }
    }
    paths
}

fn offset(contour: &Contour, outer: bool) -> Result<PathsD, ImportError> {
    if !contour.width.is_finite()
        || contour.width < 0.0
        || contour
            .ring
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
    {
        return Err(error("Invalid copper contour dimensions"));
    }
    if contour.ring.len() < 3 {
        if !outer
            && contour.ring.len() == 2
            && pomelo_core::geometry::web_hypot(
                contour.ring[0][0] - contour.ring[1][0],
                contour.ring[0][1] - contour.ring[1][1],
            ) < contour.width / 2.0
        {
            return Ok(Vec::new());
        }
        return Err(error("Invalid copper contour vertex count"));
    }
    let input = vec![path(&contour.ring)];
    if contour.width == 0.0 {
        return Ok(orient(input));
    }
    let delta = if outer {
        contour.width / 2.0
    } else {
        -contour.width / 2.0
    };
    Ok(orient(inflate_paths_d(
        &input,
        delta,
        JoinType::Round,
        EndType::Polygon,
        2.0,
        PRECISION,
        TOLERANCE,
    )))
}

pub fn build(fill: &Fill, cancel: &CancellationToken) -> Result<Vec<Region>, ImportError> {
    check(cancel)?;
    let count = fill
        .outer
        .ring
        .len()
        .saturating_add(fill.holes.iter().map(|h| h.ring.len()).sum::<usize>())
        .saturating_add(fill.thermals.len().saturating_mul(2));
    if count > 8_000_000 {
        return Err(ImportError::ResourceLimit {
            actual: count as u64,
            limit: 8_000_000,
        });
    }
    let outer = offset(&fill.outer, true)?;
    let mut holes = Vec::new();
    for hole in &fill.holes {
        check(cancel)?;
        holes.extend(offset(hole, false)?);
    }
    check(cancel)?;
    let mut tree = PolyTreeD::new();
    boolean_op_tree_d(
        ClipType::Difference,
        FillRule::NonZero,
        &outer,
        &holes,
        &mut tree,
        PRECISION,
    );
    check(cancel)?;
    if !fill.thermals.is_empty() {
        // Positive finite widths have the same sort order as their bit representation.
        let mut by_width: BTreeMap<u64, PathsD> = BTreeMap::new();
        for stroke in &fill.thermals {
            check(cancel)?;
            if !stroke.width.is_finite()
                || stroke.width <= 0.0
                || stroke
                    .a
                    .iter()
                    .chain(&stroke.b)
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
            {
                return Err(error("Invalid thermal stroke dimensions"));
            }
            by_width
                .entry(stroke.width.to_bits())
                .or_default()
                .push(path(&[stroke.a, stroke.b]));
        }
        let mut thermals = Vec::new();
        for (width, paths) in by_width {
            check(cancel)?;
            let mut expanded = inflate_paths_d(
                &paths,
                f64::from_bits(width) / 2.0,
                JoinType::Round,
                EndType::Round,
                2.0,
                PRECISION,
                TOLERANCE,
            );
            for p in &mut expanded {
                if area(p) < 0.0 {
                    p.reverse();
                }
            }
            thermals.extend(expanded);
        }
        let subject = poly_tree_to_paths_d(&tree);
        let mut merged = PolyTreeD::new();
        boolean_op_tree_d(
            ClipType::Union,
            FillRule::NonZero,
            &subject,
            &thermals,
            &mut merged,
            PRECISION,
        );
        tree = merged;
    }
    check(cancel)?;
    let mut result = Vec::new();
    // Depth-first traversal preserves the reference's exterior/island ownership.
    let mut pending: Vec<_> = tree.root().children().iter().rev().copied().collect();
    while let Some(index) = pending.pop() {
        check(cancel)?;
        let node = &tree.nodes[index];
        if !tree.is_hole(index) {
            if node.polygon().len() < 3 {
                return Err(error("Invalid exterior after boolean operation"));
            }
            let mut region = Region {
                outer: points(node.polygon()),
                holes: Vec::new(),
            };
            for &child in node.children() {
                if !tree.is_hole(child) || tree.nodes[child].polygon().len() < 3 {
                    return Err(error("Invalid hole after boolean operation"));
                }
                region.holes.push(points(tree.nodes[child].polygon()));
            }
            result.push(region);
        }
        pending.extend(node.children().iter().rev().copied());
    }
    if result.is_empty() {
        return Err(error(&format!(
            "Copper owner {} has no drawable geometry",
            fill.owner
        )));
    }
    Ok(result)
}
