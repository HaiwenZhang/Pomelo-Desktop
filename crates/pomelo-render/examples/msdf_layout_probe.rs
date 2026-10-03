//! Read-only font/layout differential probe. Does not initialize a GPU backend.
use pomelo_core::{
    display::BoardDisplay,
    interaction::Camera,
    model::{BoardScene, BoardText},
    task::CancellationToken,
};
use pomelo_render::text::msdf::{
    GlyphInstance, LabelIndex, LabelOptions, MsdfFont, PreparedGlyphs,
};
use serde::Deserialize;
use std::{fs, sync::Arc};
#[derive(Deserialize)]
struct Request {
    texts: Vec<BoardText>,
    scene: Option<Arc<BoardScene>>,
    source: Option<std::path::PathBuf>,
    views: Vec<View>,
}
#[derive(Deserialize)]
struct View {
    camera: Camera,
    width: f64,
    height: f64,
    display: BoardDisplay,
}
fn packet(i: &GlyphInstance) -> [f64; 16] {
    [
        f64::from(i.xywh[0]) + f64::from(i.low[0]),
        f64::from(i.xywh[1]) + f64::from(i.low[1]),
        f64::from(i.xywh[2]),
        f64::from(i.xywh[3]),
        f64::from(i.uv[0]),
        f64::from(i.uv[1]),
        f64::from(i.uv[2]),
        f64::from(i.uv[3]),
        f64::from(i.color[0]),
        f64::from(i.color[1]),
        f64::from(i.color[2]),
        f64::from(i.color[3]),
        f64::from(i.rotation[0]),
        f64::from(i.rotation[1]),
        f64::from(i.rotation[2]),
        f64::from(i.rotation[3]),
    ]
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(args.len() == 2, "MSDF_LAYOUT_PROBE_USAGE");
    let request: Request = serde_json::from_slice(&fs::read(&args[0])?)?;
    let cancel = CancellationToken::default();
    let scene = if let Some(scene) = request.scene {
        scene
    } else {
        use pomelo_import::{
            BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter,
        };
        AllegroImporter
            .import(
                request
                    .source
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("MSDF_LAYOUT_SOURCE"))?,
                &ImportOptions::default(),
                &ImportContext {
                    cancellation: &cancel,
                    progress: &|_| {},
                },
            )?
            .scene
    };
    let font = Arc::new(
        MsdfFont::bundled(
            request
                .texts
                .iter()
                .map(|t| t.text.as_str())
                .chain(scene.texts.iter().map(|t| t.text.as_str()))
                .chain(scene.nets.values().map(String::as_str)),
            &cancel,
        )
        .map_err(|d| anyhow::anyhow!("{}", d.code))?,
    );
    let text = PreparedGlyphs::build(&request.texts, Arc::clone(&font), 64 * 1024 * 1024, &cancel)
        .map_err(|d| anyhow::anyhow!("{}", d.code))?;
    let index = LabelIndex::build(Arc::clone(&scene), Arc::clone(&font), &cancel)
        .map_err(|d| anyhow::anyhow!("{}", d.code))?;
    let labels = request
        .views
        .into_iter()
        .map(|v| {
            index
                .layout(
                    v.camera,
                    v.width,
                    v.height,
                    &v.display,
                    LabelOptions::default(),
                    &cancel,
                )
                .map(|s| {
                    s.instances
                        .iter()
                        .map(|i| serde_json::json!({"ids":i.ids,"packet":packet(i)}))
                        .collect::<Vec<_>>()
                })
                .map_err(|d| anyhow::anyhow!("{}", d.code))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    fs::write(
        &args[1],
        serde_json::to_vec(
            &serde_json::json!({"board":text.instances.iter().map(|i|serde_json::json!({"id":i.ids[0],"packet":packet(i)})).collect::<Vec<_>>(),"labels":labels,"pages":font.pages.iter().map(|p|p.page).collect::<Vec<_>>()}),
        )?,
    )?;
    Ok(())
}
