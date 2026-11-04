//! Visible component placement regions, with analytic exterior/void coverage.
use super::*;
use crate::{
    geometry::{path_bounds, path_contains},
    model::{ComponentBody, Point},
};

pub(super) fn bounds(body: &ComponentBody) -> Option<Bounds> {
    body.regions
        .iter()
        .filter_map(|region| region.paths.first().and_then(|path| path_bounds(path)))
        .reduce(|mut a, b| {
            a.include(b.min);
            a.include(b.max);
            a
        })
}

pub(super) fn distance(
    body: &ComponentBody,
    point: Point,
    half_pixel: f64,
    cancel: &CancellationToken,
) -> Result<Option<f64>, PathError> {
    let mut nearest = f64::INFINITY;
    for region in &body.regions {
        cancel.check()?;
        let Some(outer) = region.paths.first().filter(|path| !path.is_empty()) else {
            continue;
        };
        let inside = path_contains(outer, point, cancel)?;
        let mut hole = false;
        if inside {
            for path in region.paths.iter().skip(1) {
                if path_contains(path, point, cancel)? {
                    hole = true;
                    break;
                }
            }
            if !hole {
                return Ok(Some(-half_pixel));
            }
        }
        // Voids stay unpickable; a different region may still cover this point.
        if hole {
            continue;
        }
        for edge in outer {
            cancel.check()?;
            nearest = nearest
                .min(crate::picking::segment_centerline_distance_mm(edge, point)? - half_pixel);
        }
    }
    Ok(nearest.is_finite().then_some(nearest))
}
