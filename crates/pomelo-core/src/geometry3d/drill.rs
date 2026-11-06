//! Physical, coaxial machining intervals. Heights and tool diameters must be resolved
//! from manufacturing data or an explicit preview before entering this module.

use std::{error::Error, fmt};

/// Bound per-hole sorting and scratch allocation; ordinary backdrilling uses two cuts.
pub const MAX_DRILL_CUTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrillCut3d {
    pub z: [f64; 2],
    /// Actual tool diameter, not a label or display-marker diameter.
    pub diameter_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrillSpanError {
    Invalid(&'static str),
    CutLimit,
    /// A smaller tool can leave part of the barrel; interval subtraction alone
    /// cannot represent that remaining metal cross-section.
    PartialWallCut,
    Allocation,
}

impl fmt::Display for DrillSpanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "DRILL_SPAN_{self:?}")
    }
}
impl Error for DrillSpanError {}

/// Subtract the union of full-barrel cuts, returning disjoint ascending Z intervals.
/// `outer_diameter_mm` is the barrel's maximum cross-section dimension including
/// plating. Coaxial tools at least this wide also cover the entirety of a slot.
pub fn retained_drill_spans(
    span: [f64; 2],
    outer_diameter_mm: f64,
    cuts: &[DrillCut3d],
) -> Result<Vec<[f64; 2]>, DrillSpanError> {
    if !valid_span(span) {
        return Err(DrillSpanError::Invalid("Z_INTERVAL"));
    }
    if !outer_diameter_mm.is_finite() || outer_diameter_mm <= 0.0 {
        return Err(DrillSpanError::Invalid("BARREL_DIAMETER"));
    }
    if cuts.len() > MAX_DRILL_CUTS {
        return Err(DrillSpanError::CutLimit);
    }
    let mut removed = Vec::new();
    removed
        .try_reserve_exact(cuts.len())
        .map_err(|_| DrillSpanError::Allocation)?;
    for cut in cuts {
        if !valid_span(cut.z) || !cut.diameter_mm.is_finite() || cut.diameter_mm <= 0.0 {
            return Err(DrillSpanError::Invalid("CUT"));
        }
        let z = [span[0].max(cut.z[0]), span[1].min(cut.z[1])];
        if z[0] >= z[1] {
            continue;
        }
        if cut.diameter_mm < outer_diameter_mm {
            return Err(DrillSpanError::PartialWallCut);
        }
        removed.push(z);
    }
    removed.sort_unstable_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let mut result = Vec::new();
    result
        .try_reserve_exact(removed.len() + 1)
        .map_err(|_| DrillSpanError::Allocation)?;
    let mut cursor = span[0];
    for z in removed {
        if z[0] > cursor {
            result.push([cursor, z[0]]);
        }
        cursor = cursor.max(z[1]);
        if cursor >= span[1] {
            return Ok(result);
        }
    }
    if cursor < span[1] {
        result.push([cursor, span[1]]);
    }
    Ok(result)
}

fn valid_span(z: [f64; 2]) -> bool {
    z.into_iter().all(f64::is_finite) && z[0] < z[1] && (z[1] - z[0]).is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(a: f64, b: f64) -> DrillCut3d {
        DrillCut3d {
            z: [a, b],
            diameter_mm: 0.8,
        }
    }

    #[test]
    fn no_cuts_and_top_bottom_cuts_preserve_only_the_retained_span() {
        assert_eq!(
            retained_drill_spans([-0.8, 0.8], 0.6, &[]).unwrap(),
            vec![[-0.8, 0.8]]
        );
        assert_eq!(
            retained_drill_spans([-0.8, 0.8], 0.6, &[cut(0.4, 1.0)]).unwrap(),
            vec![[-0.8, 0.4]]
        );
        assert_eq!(
            retained_drill_spans([-0.8, 0.8], 0.6, &[cut(0.4, 1.0), cut(-1.0, -0.3)]).unwrap(),
            vec![[-0.3, 0.4]]
        );
    }

    #[test]
    fn unsorted_overlapping_duplicate_and_touching_cuts_form_a_union() {
        let cuts = [
            cut(0.1, 0.3),
            cut(-0.5, -0.2),
            cut(-0.3, 0.1),
            cut(0.1, 0.3),
        ];
        assert_eq!(
            retained_drill_spans([-0.8, 0.8], 0.6, &cuts).unwrap(),
            vec![[-0.8, -0.5], [0.3, 0.8]]
        );
    }

    #[test]
    fn separated_cuts_keep_every_intermediate_segment() {
        assert_eq!(
            retained_drill_spans([-1.0, 1.0], 0.6, &[cut(-0.7, -0.5), cut(0.1, 0.4)]).unwrap(),
            vec![[-1.0, -0.7], [-0.5, 0.1], [0.4, 1.0]]
        );
    }

    #[test]
    fn full_removal_and_boundary_contact_do_not_create_zero_length_spans() {
        assert!(
            retained_drill_spans([-0.8, 0.8], 0.6, &[cut(-1.0, 1.0)])
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            retained_drill_spans([-0.8, 0.8], 0.6, &[cut(-1.0, -0.8), cut(0.8, 1.0)]).unwrap(),
            vec![[-0.8, 0.8]]
        );
    }

    #[test]
    fn partial_radial_cuts_are_rejected_instead_of_erasing_remaining_copper() {
        let partial = DrillCut3d {
            z: [0.0, 0.8],
            diameter_mm: 0.5,
        };
        assert_eq!(
            retained_drill_spans([-0.8, 0.8], 0.6, &[partial]),
            Err(DrillSpanError::PartialWallCut)
        );
        let outside = DrillCut3d {
            z: [1.0, 2.0],
            ..partial
        };
        assert_eq!(
            retained_drill_spans([-0.8, 0.8], 0.6, &[outside]).unwrap(),
            vec![[-0.8, 0.8]]
        );
    }

    #[test]
    fn exact_tool_size_is_supported_and_small_positive_segments_are_kept() {
        let cut = DrillCut3d {
            z: [1e-12, 1.0],
            diameter_mm: 0.6,
        };
        assert_eq!(
            retained_drill_spans([0.0, 1.0], 0.6, &[cut]).unwrap(),
            vec![[0.0, 1e-12]]
        );
    }

    #[test]
    fn invalid_ranges_dimensions_and_excessive_cut_counts_are_errors() {
        for z in [
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, f64::NAN],
            [-f64::MAX, f64::MAX],
        ] {
            assert_eq!(
                retained_drill_spans(z, 0.6, &[]),
                Err(DrillSpanError::Invalid("Z_INTERVAL"))
            );
        }
        assert_eq!(
            retained_drill_spans([0.0, 1.0], f64::NAN, &[]),
            Err(DrillSpanError::Invalid("BARREL_DIAMETER"))
        );
        assert_eq!(
            retained_drill_spans([0.0, 1.0], 0.6, &[cut(f64::NAN, 1.0)]),
            Err(DrillSpanError::Invalid("CUT"))
        );
        assert_eq!(
            retained_drill_spans([0.0, 1.0], 0.6, &vec![cut(0.1, 0.2); MAX_DRILL_CUTS + 1]),
            Err(DrillSpanError::CutLimit)
        );
    }

    #[test]
    fn every_returned_interval_matches_independent_cut_membership() {
        let cuts = [
            cut(-0.73, -0.55),
            cut(-0.2, 0.17),
            cut(0.08, 0.31),
            cut(0.67, 0.9),
        ];
        let result = retained_drill_spans([-1.0, 1.0], 0.6, &cuts).unwrap();
        for pair in result.windows(2) {
            assert!(pair[0][1] < pair[1][0]);
        }
        for i in 0..2000 {
            let z = -1.0 + (i as f64 + 0.5) / 1000.0;
            let retained = result.iter().any(|r| r[0] <= z && z < r[1]);
            let removed = cuts.iter().any(|cut| cut.z[0] <= z && z < cut.z[1]);
            assert_eq!(retained, !removed, "z={z}");
        }
    }
}
