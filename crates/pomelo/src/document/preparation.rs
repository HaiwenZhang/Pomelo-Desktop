//! Background import, render preparation, search indexes and view restoration.
use super::{LoadError, PreparedDocument};
use crate::services::source_font::SourceFontConfig;
use pomelo_core::{
    i18n::Locale,
    memory::MemoryBudget,
    task::{CancellationToken, LatestImportProgress},
    view_state::ViewState,
};
use pomelo_import::{BoardImporter, ImportContext, ImportOptions, formats::FormatImporter};
use pomelo_render::{
    copper::{CopperLimits, PreparedCopper},
    tracks::{PreparedTracks, TraceLimits},
};
use std::path::PathBuf;

/// An owned snapshot so background work never borrows a live document session.
pub(crate) struct DocumentPreparation {
    pub path: PathBuf,
    pub options: ImportOptions,
    pub cancellation: CancellationToken,
    pub mailbox: LatestImportProgress,
    pub reload_view: Option<ViewState>,
    pub search_locale: Locale,
    pub source_font: Option<SourceFontConfig>,
    pub preparation_budget: MemoryBudget,
}

/// Runs on the background executor; publication and request validation belong to the workbench.
pub(crate) fn prepare_document(input: DocumentPreparation) -> Result<PreparedDocument, LoadError> {
    let DocumentPreparation {
        path,
        options,
        cancellation,
        mailbox,
        reload_view,
        search_locale,
        source_font,
        preparation_budget,
    } = input;
    let mut preparation_memory =
        crate::services::preparation_memory::PreparationMemory::new(preparation_budget);
    let board = FormatImporter.import(
        &path,
        &options,
        &ImportContext {
            cancellation: &cancellation,
            progress: &|progress| mailbox.publish(progress),
        },
    )?;
    use pomelo_core::task::{ImportProgress, ImportStage};
    let begin_stage = |stage| {
        if cancellation.is_cancelled() {
            return Err(pomelo_render::tracks::PrepareError::Cancelled);
        }
        mailbox.publish(ImportProgress {
            stage,
            completed: 0,
            total: None,
        });
        Ok(())
    };
    begin_stage(ImportStage::PreparingTracks)?;
    let tracks = preparation_memory.prepare(
        |bytes| {
            PreparedTracks::build_with_outline(
                &board.scene.segments,
                &board.scene.outline,
                TraceLimits {
                    max_instances: u32::MAX as usize,
                    max_bytes: bytes,
                },
                &cancellation,
            )
        },
        PreparedTracks::allocation_bytes,
    )?;
    let drawings = preparation_memory.prepare(
        |bytes| {
            PreparedTracks::build_drawings(
                &board.scene.drawings,
                TraceLimits {
                    max_instances: u32::MAX as usize,
                    max_bytes: bytes,
                },
                &cancellation,
            )
        },
        PreparedTracks::allocation_bytes,
    )?;
    let zone_outlines = preparation_memory.prepare(
        |bytes| {
            PreparedTracks::build_zone_outlines(
                &board.scene.zones,
                TraceLimits {
                    max_instances: u32::MAX as usize,
                    max_bytes: bytes,
                },
                &cancellation,
            )
        },
        PreparedTracks::allocation_bytes,
    )?;
    begin_stage(ImportStage::PreparingTexts)?;
    let mut text_diagnostics = Vec::new();
    let mut prepared_text_ids = std::collections::BTreeSet::new();
    let source_strokes = if let Some(config) = source_font {
        match config.prepare(&board.scene, &cancellation) {
            Ok(mut source) => {
                prepared_text_ids.extend(source.objects.iter().copied());
                text_diagnostics.append(&mut source.summary.diagnostics);
                Some(std::sync::Arc::new(source))
            }
            Err(mut diagnostic) => {
                if cancellation.is_cancelled() {
                    return Err(LoadError::Prepare(
                        pomelo_render::tracks::PrepareError::Cancelled,
                    ));
                }
                diagnostic.severity = pomelo_core::model::Severity::Warning;
                text_diagnostics.push(diagnostic);
                None
            }
        }
    } else {
        None
    };
    let font = match pomelo_render::text::msdf::MsdfFont::bundled(
        board
            .scene
            .texts
            .iter()
            .map(|text| text.text.as_str())
            .chain(board.scene.nets.values().map(String::as_str)),
        &cancellation,
    ) {
        Ok(font) => Some(std::sync::Arc::new(font)),
        Err(mut diagnostic) => {
            if cancellation.is_cancelled() {
                return Err(LoadError::Prepare(
                    pomelo_render::tracks::PrepareError::Cancelled,
                ));
            }
            diagnostic.severity = pomelo_core::model::Severity::Warning;
            text_diagnostics.push(diagnostic);
            None
        }
    };
    let label_index = if let Some(font) = &font {
        match pomelo_render::text::msdf::LabelIndex::build(
            std::sync::Arc::clone(&board.scene),
            std::sync::Arc::clone(font),
            &cancellation,
        ) {
            Ok(index) => Some(std::sync::Arc::new(index)),
            Err(mut diagnostic) => {
                if cancellation.is_cancelled() {
                    return Err(LoadError::Prepare(
                        pomelo_render::tracks::PrepareError::Cancelled,
                    ));
                }
                diagnostic.severity = pomelo_core::model::Severity::Warning;
                text_diagnostics.push(diagnostic);
                None
            }
        }
    } else {
        None
    };
    let texts = if let Some(font) = font {
        match pomelo_render::text::msdf::PreparedGlyphs::build(
            board
                .scene
                .texts
                .iter()
                .filter(|text| !prepared_text_ids.contains(&text.id)),
            font,
            512 * 1024 * 1024,
            &cancellation,
        ) {
            Ok(mut source) => {
                source.bind_drawing_owners(&board.scene);
                prepared_text_ids.extend(source.objects.iter().copied());
                text_diagnostics.append(&mut source.diagnostics);
                Some(std::sync::Arc::new(source))
            }
            Err(mut diagnostic) => {
                if cancellation.is_cancelled() {
                    return Err(LoadError::Prepare(
                        pomelo_render::tracks::PrepareError::Cancelled,
                    ));
                }
                diagnostic.severity = pomelo_core::model::Severity::Warning;
                text_diagnostics.push(diagnostic);
                None
            }
        }
    } else {
        None
    };
    begin_stage(ImportStage::PreparingCopper)?;
    let copper = preparation_memory.prepare(
        |bytes| {
            PreparedCopper::build_scene(
                std::sync::Arc::clone(&board.scene),
                CopperLimits {
                    max_vertices: u32::MAX as usize,
                    max_indices: u32::MAX as usize,
                    max_zones: u32::MAX as usize,
                    max_bytes: bytes,
                },
                &cancellation,
            )
        },
        PreparedCopper::allocation_bytes,
    )?;
    begin_stage(ImportStage::PreparingPads)?;
    let mut pads = preparation_memory.prepare(
        |bytes| {
            pomelo_render::pads::PreparedPads::build(
                &board.scene.pins,
                &board.scene.vias,
                pomelo_render::pads::PadLimits {
                    max_pads: u32::MAX as usize,
                    max_bytes: bytes,
                },
                &cancellation,
            )
        },
        pomelo_render::pads::PreparedPads::allocation_bytes,
    )?;
    pads.custom_mesh = Some(std::sync::Arc::new(preparation_memory.prepare(
        |bytes| {
            pads.build_custom_meshes(
                CopperLimits {
                    max_vertices: u32::MAX as usize,
                    max_indices: u32::MAX as usize,
                    max_zones: u32::MAX as usize,
                    max_bytes: bytes,
                },
                &pomelo_core::copper::MeshLimits::default(),
                &cancellation,
            )
        },
        PreparedCopper::allocation_bytes,
    )?));
    begin_stage(ImportStage::PreparingDrills)?;
    let drills = preparation_memory.prepare(
        |bytes| {
            pomelo_render::drills::PreparedDrills::build(
                &board.scene.pins,
                &board.scene.vias,
                pomelo_render::pads::PadLimits {
                    max_pads: u32::MAX as usize,
                    max_bytes: bytes,
                },
                &cancellation,
            )
        },
        |drills| drills.geometry.allocation_bytes(),
    )?;
    begin_stage(ImportStage::BuildingSearch)?;
    let search = pomelo_core::search::SearchIndex::build(&board.scene, &cancellation)
        .map_err(LoadError::Search)?
        .ok_or(pomelo_render::tracks::PrepareError::Cancelled)?
        .with_locale(search_locale)
        .map_err(LoadError::Search)?;
    begin_stage(ImportStage::BuildingPicking)?;
    let mut pick_quads = Vec::new();
    let quad_count = texts
        .as_ref()
        .map_or(0, |source| source.pick_quads.len())
        .saturating_add(
            source_strokes
                .as_ref()
                .map_or(0, |source| source.pick_quads.len()),
        );
    pick_quads
        .try_reserve_exact(quad_count)
        .map_err(|_| pomelo_render::tracks::PrepareError::Allocation)?;
    if let Some(source) = &texts {
        pick_quads.extend_from_slice(&source.pick_quads);
    }
    if let Some(source) = &source_strokes {
        pick_quads.extend_from_slice(&source.pick_quads);
    }
    let picking = pomelo_core::picking_index::BoardPickingIndex::build(
        board.scene.clone(),
        pomelo_render::tracks::TraceLimits::default().max_instances,
        &cancellation,
    )
    .and_then(|index| index.with_text_quads(&pick_quads, &cancellation))
    .map_err(LoadError::Picking)?;
    let restored_view_result = if let Some(state) = reload_view {
        state
            .matching(&board.identity)
            .map(|matched| matched.cloned())
    } else {
        crate::services::preferences::ViewStore::platform_default()
            .map(|store| store.load_matching(&path, &board.identity))
            .transpose()
            .map(Option::flatten)
    };
    let restored_view = match restored_view_result {
        Ok(state) => state,
        Err(mut diagnostic) => {
            diagnostic.severity = pomelo_core::model::Severity::Warning;
            text_diagnostics.push(diagnostic);
            None
        }
    };
    if cancellation.is_cancelled() {
        return Err(LoadError::Prepare(
            pomelo_render::tracks::PrepareError::Cancelled,
        ));
    }
    let preview = match crate::services::preview::rasterize(
        pomelo_render::scene::thumbnail::Source {
            bounds: board.scene.bounds,
            tracks: &tracks,
            drawings: &drawings,
            copper: &copper,
            pads: &pads,
            drills: &drills.geometry,
        },
        &cancellation,
    ) {
        Ok(preview) => preview,
        Err(diagnostic) => {
            text_diagnostics.push(diagnostic.with_path(&path));
            None
        }
    };
    if cancellation.is_cancelled() {
        return Err(LoadError::Prepare(
            pomelo_render::tracks::PrepareError::Cancelled,
        ));
    }
    Ok::<_, LoadError>(PreparedDocument {
        preview,
        restored_view: restored_view.map(Box::new),
        drawings: std::sync::Arc::new(drawings),
        render_diagnostics: {
            text_diagnostics.extend(pomelo_render::coverage::diagnostics_with_text_ids(
                &board.scene,
                &prepared_text_ids,
            ));
            text_diagnostics
        },
        texts,
        source_strokes,
        label_index,
        board,
        search: std::sync::Arc::new(search),
        picking: std::sync::Arc::new(picking),
        tracks: std::sync::Arc::new(tracks),
        zone_outlines: std::sync::Arc::new(zone_outlines),
        copper: std::sync::Arc::new(copper),
        pads: std::sync::Arc::new(pads),
        drills: std::sync::Arc::new(drills.geometry),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::{
        display::BoardDisplay,
        interaction::{Camera, SelectionMode},
        model::Point,
    };

    #[test]
    fn prepares_a_board_with_indexes_and_restores_its_matching_view() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("board.kicad_pcb");
        std::fs::write(
            &path,
            include_bytes!("../../../pomelo-import/tests/data/basic.kicad_pcb"),
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let options = ImportOptions::default();
        let board = FormatImporter
            .import(
                &path,
                &options,
                &ImportContext {
                    cancellation: &cancellation,
                    progress: &|_| {},
                },
            )
            .unwrap();
        let camera = Camera {
            center: Point::new(10.0, 20.0),
            pixels_per_mm: 12.0,
            ..Camera::default()
        };
        let view = ViewState {
            schema_version: 1,
            source: board.identity.clone(),
            camera,
            display: BoardDisplay::default(),
            selection_mode: SelectionMode::Object,
            pick_filter: Default::default(),
            selection: None,
            selection_anchor: None,
        };
        let budget = MemoryBudget::new(0);
        let prepared = prepare_document(DocumentPreparation {
            path,
            options,
            cancellation,
            mailbox: LatestImportProgress::default(),
            reload_view: Some(view),
            search_locale: Locale::English,
            source_font: None,
            preparation_budget: budget.clone(),
        })
        .unwrap_or_else(|error| panic!("{}", error.diagnostic().code));

        assert_eq!(prepared.board.identity, board.identity);
        assert_eq!(prepared.tracks.instances.len(), 1);
        assert_eq!(prepared.pads.analytic.len(), 2);
        assert_eq!(prepared.copper.batches.len(), 1);
        assert_eq!(prepared.picking.segment_count(), 1);
        assert!(
            prepared
                .search
                .entries()
                .iter()
                .any(|entry| entry.name == "GND")
        );
        let restored = prepared.restored_view.as_ref().unwrap();
        assert_eq!(restored.camera.center, camera.center);
        assert_eq!(restored.camera.pixels_per_mm, camera.pixels_per_mm);
        assert!(budget.used() > 0);
        drop(prepared);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn cancelled_preparation_does_not_read_the_source_or_reserve_geometry() {
        let directory = tempfile::tempdir().unwrap();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let budget = MemoryBudget::new(0);
        let result = prepare_document(DocumentPreparation {
            path: directory.path().join("missing.kicad_pcb"),
            options: ImportOptions::default(),
            cancellation,
            mailbox: LatestImportProgress::default(),
            reload_view: None,
            search_locale: Locale::English,
            source_font: None,
            preparation_budget: budget.clone(),
        });
        assert!(matches!(
            result,
            Err(LoadError::Import(pomelo_import::ImportError::Cancelled))
        ));
        assert_eq!(budget.used(), 0);
    }
}
