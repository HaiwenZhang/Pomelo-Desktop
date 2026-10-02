//! Allegro layout detection and bounded binary readers.

pub mod database;
pub mod decoder;
pub mod header;
pub mod index;
pub mod reader;
mod record_scan;
pub mod semantics;

use std::{path::Path, sync::Arc};

use crate::{BoardImporter, FormatProbe, ImportContext, ImportError, ImportOptions, ImportedBoard};

pub struct AllegroImporter;

impl BoardImporter for AllegroImporter {
    fn probe(&self, bytes: &[u8], options: &ImportOptions) -> Result<FormatProbe, ImportError> {
        let header = header::BrdHeader::read(bytes, options.text_encoding)?;
        Ok(FormatProbe {
            format: "allegro".into(),
            layout_version: header.version,
            writer_version: header.writer_version,
            scene_supported: true,
        })
    }

    fn import(
        &self,
        path: &Path,
        options: &ImportOptions,
        context: &ImportContext<'_>,
    ) -> Result<ImportedBoard, ImportError> {
        context.check_cancelled()?;
        let database = database::BrdDatabase::read_path(
            path,
            options,
            &index::IndexLimits::default(),
            decoder::DecodeLimits::default(),
            context,
        )?;
        let identity = pomelo_core::view_state::SourceIdentity {
            sha256: crate::source::content_sha256(database.source_bytes(), context)?,
            format: "allegro".into(),
            encoding: options.text_encoding.tag().into(),
        };
        let mut limits = semantics::scene::SceneLimits::default();
        // One shared path allocation, including owned OS string capacity and Arc bookkeeping.
        limits.max_output_bytes = limits
            .max_output_bytes
            .saturating_sub(path.as_os_str().len().saturating_mul(2).saturating_add(128));
        let mut scene = semantics::scene::SceneBuilder::new(&database, limits).build(context)?;
        let source_path: Arc<Path> = Arc::from(path);
        for diagnostic in &mut scene.diagnostics {
            context.check_cancelled()?;
            diagnostic.path = Some(Arc::clone(&source_path));
        }
        context.check_cancelled()?;
        Ok(ImportedBoard {
            identity,
            scene: Arc::new(scene),
            source: FormatProbe {
                format: "allegro".into(),
                layout_version: database.header().version,
                writer_version: database.header().writer_version.clone(),
                scene_supported: true,
            },
        })
    }
}
