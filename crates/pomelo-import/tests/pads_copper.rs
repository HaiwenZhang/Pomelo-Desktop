use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    model::Point,
    task::CancellationToken,
};
use pomelo_import::{
    ImportError,
    formats::pads::saved_fill::{self, Contour, Fill, Thermal},
};

fn rectangle(a: f64, b: f64) -> Contour {
    Contour {
        width: 0.0,
        ring: vec![[a, a], [b, a], [b, b], [a, b]],
    }
}
fn mesh(fill: &Fill, cancel: &CancellationToken) -> CopperMesh {
    let regions = saved_fill::build(fill, cancel).unwrap();
    let rings = regions
        .into_iter()
        .flat_map(|r| std::iter::once(r.outer).chain(r.holes))
        .map(|r| r.into_iter().map(|p| Point::new(p[0], p[1])).collect())
        .collect::<Vec<_>>();
    CopperMesh::build(&rings, &[], &MeshLimits::default(), cancel).unwrap()
}
#[test]
fn saved_fill_retains_cutout_and_restores_only_thermal_spokes() {
    let cancel = CancellationToken::default();
    let mut fill = Fill {
        owner: 1,
        outer: rectangle(0.0, 10.0),
        holes: vec![rectangle(3.0, 7.0)],
        thermals: vec![],
    };
    let plain = mesh(&fill, &cancel);
    assert!(plain.covers_fill(Point::new(1.0, 1.0), &cancel).unwrap());
    assert!(!plain.covers_fill(Point::new(5.0, 5.0), &cancel).unwrap());
    fill.thermals.push(Thermal {
        a: [2.0, 5.0],
        b: [8.0, 5.0],
        width: 0.4,
    });
    let thermal = mesh(&fill, &cancel);
    assert!(thermal.covers_fill(Point::new(5.0, 5.0), &cancel).unwrap());
    assert!(!thermal.covers_fill(Point::new(5.0, 5.3), &cancel).unwrap());
    assert!(!thermal.covers_fill(Point::new(5.0, 4.7), &cancel).unwrap());
}
#[test]
fn boundary_width_expands_exterior_and_shrinks_saved_cutout() {
    let cancel = CancellationToken::default();
    let mut fill = Fill {
        owner: 1,
        outer: rectangle(0.0, 10.0),
        holes: vec![rectangle(3.0, 7.0)],
        thermals: vec![],
    };
    fill.outer.width = 1.0;
    fill.holes[0].width = 1.0;
    let copper = mesh(&fill, &cancel);
    assert!(copper.covers_fill(Point::new(-0.49, 5.0), &cancel).unwrap());
    assert!(!copper.covers_fill(Point::new(-0.51, 5.0), &cancel).unwrap());
    assert!(copper.covers_fill(Point::new(3.49, 5.0), &cancel).unwrap());
    assert!(!copper.covers_fill(Point::new(3.51, 5.0), &cancel).unwrap());
}
#[test]
fn invalid_geometry_and_cancelled_fill_fail_before_clipping() {
    let cancel = CancellationToken::default();
    let mut fill = Fill {
        owner: 1,
        outer: rectangle(0.0, 10.0),
        holes: vec![],
        thermals: vec![],
    };
    fill.outer.ring[0][0] = f64::NAN;
    assert!(matches!(
        saved_fill::build(&fill, &cancel),
        Err(ImportError::Format { .. })
    ));
    cancel.cancel();
    assert!(matches!(
        saved_fill::build(&fill, &cancel),
        Err(ImportError::Cancelled)
    ));
}
