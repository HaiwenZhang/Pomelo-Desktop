use pomelo_core::{
    interaction::{Camera, ViewportNavigation},
    model::{Bounds, Point},
};

#[test]
fn fit_preserves_fourteen_percent_margin_and_resize_does_not_refit() {
    let board = Bounds {
        min: Point::new(-50.0, -1.0),
        max: Point::new(50.0, 1.0),
    };
    let mut view = ViewportNavigation::default();
    assert!(view.resize(board, 800.0, 600.0));
    let camera = view.camera();
    let min = camera.board_to_view(board.min, 800.0, 600.0);
    let max = camera.board_to_view(board.max, 800.0, 600.0);
    assert!((min.x - 56.0).abs() < 1e-12 && (max.x - 744.0).abs() < 1e-12);
    assert!(view.resize(board, 400.0, 300.0));
    assert_eq!(view.camera().pixels_per_mm, camera.pixels_per_mm);
}

#[test]
fn zoom_percentage_is_relative_to_fit_and_locating_does_not_reset_it() {
    let board = Bounds {
        min: Point::default(),
        max: Point::new(100.0, 80.0),
    };
    let mut view = ViewportNavigation::default();
    assert!(view.resize(board, 800.0, 600.0));
    assert_eq!(view.zoom_percent(), 100.0);
    assert!(view.zoom_at(Point::new(400.0, 300.0), 2.0));
    assert_eq!(view.zoom_percent(), 200.0);
    assert!(view.locate(Bounds {
        min: Point::new(12.0, 18.0),
        max: Point::new(12.0, 18.0)
    }));
    assert!((view.zoom_percent() - 144.0 / 6.45 * 100.0).abs() < 1e-9);
    assert!(view.fit(board));
    assert_eq!(view.zoom_percent(), 100.0);
}

#[test]
fn point_focus_has_a_bounded_scale_in_a_large_canvas() {
    let board = Bounds {
        min: Point::default(),
        max: Point::new(100.0, 80.0),
    };
    let mut view = ViewportNavigation::default();
    assert!(view.resize(board, 3840.0, 2160.0));
    view.flip();
    assert!(view.locate(Bounds {
        min: Point::new(12.0, 18.0),
        max: Point::new(12.0, 18.0)
    }));
    assert_eq!(view.camera().pixels_per_mm, 288.0);
    assert!(view.camera().flipped);
}

#[test]
fn restored_camera_survives_layout_with_a_board_relative_zoom_baseline() {
    let board = Bounds {
        min: Point::default(),
        max: Point::new(100.0, 80.0),
    };
    let camera = Camera {
        center: Point::new(23.0, -7.0),
        pixels_per_mm: 12.9,
        flipped: true,
    };
    let mut view = ViewportNavigation::default();
    assert!(view.restore_camera(camera));
    assert!(view.resize(board, 800.0, 600.0));
    assert_eq!(view.camera().center, camera.center);
    assert_eq!(view.camera().pixels_per_mm, camera.pixels_per_mm);
    assert!((view.zoom_percent() - 200.0).abs() < 1e-12);
}

#[test]
fn minimum_zoom_preserves_pointer_anchor_on_either_side() {
    for flipped in [false, true] {
        let mut camera = Camera {
            flipped,
            ..Camera::default()
        };
        let anchor = Point::new(80.0, 520.0);
        let before = camera.view_to_board(anchor, 800.0, 600.0);
        assert!(camera.zoom_at(anchor, 800.0, 600.0, 1e-30));
        assert_eq!(camera.pixels_per_mm, 0.01);
        assert!(camera.view_to_board(anchor, 800.0, 600.0).distance(before) < 1e-9);
    }
}

#[test]
fn invalid_fit_or_focus_does_not_replace_a_valid_view() {
    let bounds = Bounds {
        min: Point::default(),
        max: Point::new(10.0, 10.0),
    };
    let mut view = ViewportNavigation::default();
    assert!(view.resize(bounds, 800.0, 600.0));
    let previous = view.camera();
    let invalid = Bounds {
        min: Point::new(f64::INFINITY, 0.0),
        ..bounds
    };
    assert!(!view.fit(invalid));
    assert!(!view.locate(invalid));
    assert_eq!(view.camera().center, previous.center);
    assert_eq!(view.camera().pixels_per_mm, previous.pixels_per_mm);
    assert_eq!(view.zoom_percent(), 100.0);
}
