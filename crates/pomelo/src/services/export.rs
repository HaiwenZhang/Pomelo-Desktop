//! Local review output: PNG rasterization of view snapshots and atomic file writes.
//! A failed export leaves any existing file untouched; nothing is written to the source.
use std::{
    io::{self, Write},
    path::Path,
};

/// Write through a temporary file in the destination directory, then replace the
/// target in one step. Errors before the final rename leave the target unchanged.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    // tempfile replaces existing files on Windows too, unlike std::fs::rename.
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Rasterize a snapshot SVG at its declared pixel size and encode it as PNG.
pub fn rasterize_png(svg: &str) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| "SNAPSHOT_PIXMAP_SIZE".to_owned())?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied alpha; PNG expects straight alpha.
    let mut rgba = Vec::with_capacity(pixmap.pixels().len() * 4);
    for pixel in pixmap.pixels() {
        let color = pixel.demultiply();
        rgba.extend_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    let image = image::RgbaImage::from_raw(size.width(), size.height(), rgba)
        .ok_or_else(|| "SNAPSHOT_IMAGE_SIZE".to_owned())?;
    let mut png = Vec::new();
    image
        .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_writes_replace_existing_files_and_keep_them_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("summary.csv");
        std::fs::write(&path, b"old").unwrap();
        write_atomically(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let missing = directory.path().join("missing").join("summary.csv");
        assert!(write_atomically(&missing, b"x").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn png_has_the_snapshot_size_and_background() {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"3\" \
                   viewBox=\"0 0 4 3\"><rect width=\"4\" height=\"3\" fill=\"#102030\"/></svg>";
        let png = rasterize_png(svg).unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (4, 3));
        assert_eq!(image.get_pixel(1, 1).0, [0x10, 0x20, 0x30, 255]);
    }

    #[test]
    fn material_snapshot_masks_follow_screen_pixels_through_zoom_and_flip() {
        use pomelo_core::{
            appearance::{ColorTarget, pattern::FillPattern},
            copper::CopperMesh,
            display::BoardDisplay,
            model::{BoardScene, Bounds, LayerId, NetId, ObjectId, Point, Segment, Zone, ZoneKind},
            task::CancellationToken,
        };
        use pomelo_render::scene::snapshot::{self, Marks, SnapshotRequest};
        use std::collections::BTreeMap;
        let path = |min: f64, max: f64| {
            let corners = [(min, min), (max, min), (max, max), (min, max)];
            (0..4)
                .map(|i| Segment {
                    id: ObjectId(i as u32),
                    track_id: ObjectId(i as u32),
                    layer: LayerId(0),
                    net: NetId(0),
                    a: Point::new(corners[i].0, corners[i].1),
                    b: Point::new(corners[(i + 1) % 4].0, corners[(i + 1) % 4].1),
                    width: 0.0,
                    arc: None,
                    bond_wire: None,
                })
                .collect()
        };
        let board = BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: BTreeMap::new(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![Zone {
                id: ObjectId(9),
                layer: LayerId(0),
                net: NetId(0),
                kind: ZoneKind::Dynamic,
                paths: vec![path(0.0, 10.0), path(4.0, 6.0)],
                mesh: CopperMesh::default(),
            }],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            bounds: Bounds {
                min: Point::default(),
                max: Point::new(10.0, 10.0),
            },
            diagnostics: vec![],
        };
        let colors = BTreeMap::from([(LayerId(0), [1.0, 0.0, 0.0, 1.0])]);
        let mut display = BoardDisplay {
            copper_opacity: 1.0,
            ..Default::default()
        };
        for pattern in FillPattern::ALL {
            display
                .appearance
                .set_pattern(ColorTarget::Etch(LayerId(0)), Some(pattern));
            for half in [5.0, 3.0] {
                for flipped in [false, true] {
                    let svg = snapshot::svg(
                        &SnapshotRequest {
                            scene: &board,
                            display: &display,
                            area: Bounds {
                                min: Point::new(5.0 - half, 5.0 - half),
                                max: Point::new(5.0 + half, 5.0 + half),
                            },
                            width: 128,
                            height: 128,
                            pixel_ratio: 1.0,
                            annotations: snapshot::AnnotationSources::default(),
                            flipped,
                            layer_order: &[LayerId(0)],
                            colors: &colors,
                            fallback_color: [1.0, 0.0, 0.0, 1.0],
                            drill_color: [0.0; 4],
                            outline_color: [1.0; 4],
                            background: [0.0, 0.0, 0.0, 1.0],
                            mark_color: [1.0; 4],
                            object_colors: None,
                            marks: Marks::default(),
                            scale_unit: None,
                            max_elements: snapshot::MAX_ELEMENTS,
                        },
                        &CancellationToken::default(),
                    )
                    .unwrap();
                    let png = rasterize_png(&svg).unwrap();
                    let image = image::load_from_memory(&png).unwrap().to_rgba8();
                    for y in 10..40 {
                        for x in 10..40 {
                            let expected = if pattern.ink(x, y) {
                                [255, 0, 0, 255]
                            } else {
                                [0, 0, 0, 255]
                            };
                            assert_eq!(
                                image.get_pixel(x, y).0,
                                expected,
                                "{pattern:?}, flip={flipped}, half={half}, {x},{y}"
                            );
                        }
                    }
                    assert_eq!(
                        image.get_pixel(64, 64).0,
                        [0, 0, 0, 255],
                        "copper hole must survive the material mask"
                    );
                }
            }
        }
    }
}
