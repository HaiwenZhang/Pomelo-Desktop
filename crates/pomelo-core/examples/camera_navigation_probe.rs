//! Development-only camera oracle adapter; no UI or GPU dependency.
use pomelo_core::{
    interaction::{Camera, ViewportNavigation},
    model::{Bounds, Point},
};
use serde::{Deserialize, Serialize};
use std::{error::Error, fs};

#[derive(Deserialize)]
struct Case {
    bounds: Bounds,
    actions: Vec<Action>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Action {
    Resize { width: f64, height: f64 },
    Fit,
    Zoom { anchor: Point, factor: f64 },
    Pan { delta: Point },
    Flip,
    Locate { bounds: Bounds },
    Restore { camera: Camera },
}
#[derive(Serialize)]
struct Snapshot {
    camera: Camera,
    zoom: f64,
    points: [Point; 3],
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("CAMERA_PROBE_USAGE".into());
    }
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(&args[0])?)?;
    let mut results = Vec::new();
    for case in cases {
        let mut navigation = ViewportNavigation::default();
        let mut snapshots = Vec::new();
        for action in case.actions {
            let valid = match action {
                Action::Resize { width, height } => navigation.resize(case.bounds, width, height),
                Action::Fit => navigation.fit(case.bounds),
                Action::Zoom { anchor, factor } => navigation.zoom_at(anchor, factor),
                Action::Pan { delta } => navigation.pan(delta),
                Action::Flip => {
                    navigation.flip();
                    true
                }
                Action::Locate { bounds } => navigation.locate(bounds),
                Action::Restore { camera } => navigation.restore_camera(camera),
            };
            if !valid {
                return Err("CAMERA_PROBE_INVALID_ACTION".into());
            }
            let camera = navigation.camera();
            let size = navigation.size();
            snapshots.push(Snapshot {
                camera,
                zoom: navigation.zoom_percent(),
                points: [
                    Point::default(),
                    Point::new(size.x / 2.0, size.y / 2.0),
                    size,
                ]
                .map(|p| camera.view_to_board(p, size.x, size.y)),
            });
        }
        results.push(snapshots);
    }
    fs::write(&args[1], serde_json::to_vec(&results)?)?;
    Ok(())
}
