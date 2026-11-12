//! Experimental hook for the solidcore comparison (docs/pcb-3d-solidcore-experiment-plan.md).
//!
//! Every extrusion ends in one normalized region: an outer ring and its holes after the
//! Clipper2 boolean. By default the region is triangulated here (CDT caps plus walls). A
//! process-wide [`RegionExtruder`] may take over that last step so another geometry engine can
//! build the same region from identical input. Nothing installs one unless asked to.

use super::{SolidError, SolidMesh};
use crate::model::Point;
use std::sync::{Arc, RwLock};

/// One normalized region in coordinates local to `origin`.
pub struct RegionInput<'a> {
    /// `rings[0]` is the outer ring, counter-clockwise; the rest are holes, clockwise. Each ring
    /// is open (the last point is not repeated), has at least three points and no exact
    /// collinear runs.
    pub rings: &'a [Vec<[f64; 2]>],
    /// Added to every ring coordinate to reach source millimetres.
    pub origin: Point,
    /// Bottom and top of the extrusion, `z[0] < z[1]`.
    pub z: [f64; 2],
    /// Cap area the rings enclose (outer minus holes), for the backend's own checks.
    pub area: f64,
}

/// Builds one region's closed display mesh and appends it to `mesh`, keeping `mesh.bounds`
/// and `mesh.surfaces` consistent with the default implementation.
pub trait RegionExtruder: Send + Sync {
    fn extrude(&self, input: &RegionInput<'_>, mesh: &mut SolidMesh) -> Result<(), SolidError>;
}

static EXTRUDER: RwLock<Option<Arc<dyn RegionExtruder>>> = RwLock::new(None);

/// Installs (or with `None`, removes) the process-wide region extruder.
pub fn set_region_extruder(extruder: Option<Arc<dyn RegionExtruder>>) {
    *EXTRUDER.write().unwrap_or_else(|e| e.into_inner()) = extruder;
}

pub(super) fn region_extruder() -> Option<Arc<dyn RegionExtruder>> {
    EXTRUDER.read().unwrap_or_else(|e| e.into_inner()).clone()
}
