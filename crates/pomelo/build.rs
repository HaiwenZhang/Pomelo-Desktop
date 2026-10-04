use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=assets/pomelo.svg");
    println!("cargo:rerun-if-env-changed=POMELO_PACKAGE_ICON");
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    // All Windows icon sizes share the application's SVG source.
    let svg = fs::read_to_string("assets/pomelo.svg")?;
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())?;
    let sizes = [16_u32, 24, 32, 48, 64, 128, 256];
    let mut images = Vec::new();
    for size in sizes {
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(size, size).ok_or("could not allocate icon pixels")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(
                size as f32 / tree.size().width(),
                size as f32 / tree.size().height(),
            ),
            &mut pixmap.as_mut(),
        );
        images.push(pixmap.encode_png()?);
    }
    let mut icon = vec![0, 0, 1, 0];
    icon.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + sizes.len() as u32 * 16;
    for (size, png) in sizes.iter().zip(&images) {
        let dimension = if *size == 256 { 0 } else { *size as u8 };
        icon.extend_from_slice(&[dimension, dimension, 0, 0]);
        icon.extend_from_slice(&1_u16.to_le_bytes());
        icon.extend_from_slice(&32_u16.to_le_bytes());
        icon.extend_from_slice(&(png.len() as u32).to_le_bytes());
        icon.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in images {
        icon.extend_from_slice(&png);
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    let icon_path = output.join("pomelo.ico");
    fs::write(&icon_path, icon)?;
    if let Some(package_icon) = env::var_os("POMELO_PACKAGE_ICON") {
        fs::copy(&icon_path, package_icon)?;
    }
    let resource = output.join("pomelo.rc");
    fs::write(
        &resource,
        format!(
            "1 ICON \"{}\"\n",
            icon_path.display().to_string().replace('\\', "/")
        ),
    )?;
    embed_resource::compile(&resource, embed_resource::NONE).manifest_required()?;
    Ok(())
}
