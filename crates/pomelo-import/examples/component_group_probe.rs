//! Development-only full reference grouping oracle; not linked into the application.
use anyhow::{Context, ensure};
use pomelo_core::{
    interaction::SelectionMode,
    picking::ObjectHit,
    search::{SearchIndex, SearchTarget},
    selection::{SelectedObject, SelectionTarget, resolve_canvas_candidates},
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

fn object(value: SelectedObject) -> serde_json::Value {
    match value {
        SelectedObject::Pin(id) => json!(["pin", id.0]),
        SelectedObject::Via(id) => json!(["via", id.0]),
        _ => json!(["unsupported"]),
    }
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 3, "COMPONENT_PROBE_USAGE");
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[2]);
    let cancel = CancellationToken::default();
    let scene = if args[1] == "synthetic" {
        std::sync::Arc::new(serde_json::from_slice::<pomelo_core::model::BoardScene>(
            &fs::read(&source)?,
        )?)
    } else {
        let options = ImportOptions {
            text_encoding: TextEncoding::from_tag(&args[1].to_string_lossy())
                .context("COMPONENT_PROBE_ENCODING")?,
            ..Default::default()
        };
        AllegroImporter
            .import(
                &source,
                &options,
                &ImportContext {
                    cancellation: &cancel,
                    progress: &|_| {},
                },
            )?
            .scene
    };
    let scene = &scene;
    let mut placements = std::collections::BTreeMap::<&str, Vec<u32>>::new();
    for component in &scene.components {
        placements
            .entry(&component.reference)
            .or_default()
            .push(component.id.0);
    }
    let duplicate_placements: Vec<_> = placements
        .into_iter()
        .filter(|(reference, ids)| !reference.is_empty() && ids.len() > 1)
        .map(|(reference, ids)| json!({"reference":reference,"ids":ids}))
        .collect();
    let index = SearchIndex::build(scene, &cancel)?.context("COMPONENT_PROBE_CANCELLED")?;
    let mut entries = vec![];
    let mut groups = vec![];
    for entry in index.entries() {
        match entry.target {
            SearchTarget::Net(id) => {
                entries.push(json!({"kind":"net","id":id.0,"name":entry.name,"count":entry.count}))
            }
            SearchTarget::ComponentGroup(anchor) => {
                entries.push(json!({"kind":"component","id":entry.name,"name":entry.name,"count":entry.count}));
                let target = SelectionTarget::ComponentGroup(anchor);
                let mut members = vec![];
                anchor.visit_members(scene, &cancel, |member| members.push(member))?;
                // Validate real UI pagination and full hover/pin overlays, not only grouping.
                let mut pages = vec![];
                for offset in (0..members.len()).step_by(256) {
                    let page = target.members_page(scene, offset, 256, &cancel)?;
                    ensure!(page.total == members.len(), "COMPONENT_PROBE_PAGE_TOTAL");
                    pages.extend(page.objects);
                }
                let summary = target.summarize(scene, &cancel)?;
                let resolved = members
                    .last()
                    .map(|&member| {
                        let candidates = resolve_canvas_candidates(
                            scene,
                            &[ObjectHit {
                                object: member,
                                distance_mm: 0.0,
                            }],
                            SelectionMode::Component,
                            1,
                            &cancel,
                        )?;
                        Ok::<_, pomelo_core::geometry::PathError>(
                            candidates.first().map(|candidate| candidate.target),
                        )
                    })
                    .transpose()?
                    .flatten();
                ensure!(resolved == Some(target), "COMPONENT_PROBE_CANONICAL_TARGET");
                groups.push(json!({"name":entry.name,"count":entry.count,
                    "members":members.iter().copied().map(object).collect::<Vec<_>>(),
                    "pages":pages.into_iter().map(object).collect::<Vec<_>>(),
                    "hover_members":target.component_objects(scene,&cancel)?.into_iter().map(object).collect::<Vec<_>>(),
                    "selected_pin_count":target.pin_ids(scene,&cancel)?.len(),
                    "summary":{"pins":summary.pins,"vias":summary.vias,"segments":summary.segments,"zones":summary.zones}}));
            }
            SearchTarget::Component(_) => anyhow::bail!("COMPONENT_PROBE_LEGACY_SEARCH_ENTRY"),
        }
    }
    fs::write(
        output,
        serde_json::to_vec(
            &json!({"sha256":format!("{:x}",Sha256::digest(fs::read(source)?)),"entries":entries,"groups":groups,
        "source_placements":scene.components.len(),"source_pins":scene.pins.len(),"source_fingers":scene.vias.iter().filter(|via|via.finger.is_some()).count(),"duplicate_placement_references":duplicate_placements}),
        )?,
    )?;
    Ok(())
}
