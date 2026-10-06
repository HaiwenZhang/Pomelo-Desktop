//! Native Rust readers for Allegro, Altium, ODB++, PADS, KiCad and HFSS 3D Layout.
pub mod allegro;
mod altium;
mod geometry;
mod hfss;
mod kicad;
mod native;
mod odb;
mod pads;
pub mod pads_copper;
mod sexpr;

use std::{io::Read, path::Path, sync::Arc};

use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    i18n::{Message, MessageKey},
    model::{
        BoardScene, Diagnostic, DrawingLayer, LayerId, NetId, ObjectId, Point, Segment, Severity,
        SpecialLayer, SpecialLayerKind, Zone, ZoneKind,
    },
    task::{ImportProgress, ImportStage},
    view_state::SourceIdentity,
};

use crate::{BoardImporter, FormatProbe, ImportContext, ImportError, ImportOptions, ImportedBoard};
use allegro::AllegroImporter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardFormat {
    Allegro,
    Altium,
    Odb,
    Pads,
    KiCad,
    Hfss,
}

impl BoardFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?.to_ascii_lowercase();
        if name.ends_with(".tar.gz") {
            return Some(Self::Odb);
        }
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "brd" => Some(Self::Allegro),
            "pcbdoc" => Some(Self::Altium),
            "tgz" | "tar" => Some(Self::Odb),
            "pcb" => Some(Self::Pads),
            "kicad_pcb" => Some(Self::KiCad),
            "def" => Some(Self::Hfss),
            _ => None,
        }
    }
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Allegro => "allegro",
            Self::Altium => "altium",
            Self::Odb => "odb",
            Self::Pads => "pads",
            Self::KiCad => "kicad",
            Self::Hfss => "hfss",
        }
    }
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Allegro => "Allegro",
            Self::Altium => "Altium",
            Self::Odb => "ODB++",
            Self::Pads => "PADS",
            Self::KiCad => "KiCad",
            Self::Hfss => "HFSS 3D Layout",
        }
    }
    pub fn from_tag(tag: &str) -> Option<Self> {
        [
            Self::Allegro,
            Self::Altium,
            Self::Odb,
            Self::Pads,
            Self::KiCad,
            Self::Hfss,
        ]
        .into_iter()
        .find(|format| format.tag() == tag)
    }
}

/// Selects an importer without making the desktop application understand source records.
pub struct FormatImporter;

/// Stable product spelling for imported-format labels, retaining unknown tags.
pub fn format_display_name(tag: &str) -> &str {
    match BoardFormat::from_tag(tag) {
        Some(format) => format.display_name(),
        None => tag,
    }
}
impl BoardImporter for FormatImporter {
    fn probe(&self, bytes: &[u8], options: &ImportOptions) -> Result<FormatProbe, ImportError> {
        let format = if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
            BoardFormat::Altium
        } else if bytes.starts_with(&[0, 0xff]) {
            BoardFormat::Pads
        } else if bytes.get(5..17) == Some(b"$begin 'Hdr'".as_slice()) {
            BoardFormat::Hfss
        } else if bytes.starts_with(&[0x1f, 0x8b])
            || bytes.get(257..262) == Some(b"ustar".as_slice())
        {
            BoardFormat::Odb
        } else if bytes
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(bytes)
            .trim_ascii_start()
            .starts_with(b"(kicad_pcb")
        {
            BoardFormat::KiCad
        } else {
            return AllegroImporter.probe(bytes, options);
        };
        Ok(FormatProbe {
            format: format.tag().into(),
            layout_version: 0,
            writer_version: String::new(),
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
        let format = BoardFormat::from_path(path).ok_or_else(|| ImportError::Format {
            format: "unknown".into(),
            details: "Unsupported PCB file extension".into(),
        })?;
        if format == BoardFormat::Allegro {
            return AllegroImporter.import(path, options, context);
        }
        let bytes = crate::source::read_path(path, options, context)?;
        let identity = SourceIdentity {
            sha256: crate::source::content_sha256(&bytes, context)?,
            format: format.tag().into(),
            encoding: "source".into(),
        };
        let bytes = if format == BoardFormat::Odb && bytes.starts_with(&[0x1f, 0x8b]) {
            decompress(&bytes, context)?
        } else {
            bytes
        };
        (context.progress)(ImportProgress {
            stage: ImportStage::Decoding,
            completed: 0,
            total: None,
        });
        let mut output = match format {
            BoardFormat::Pads => pads::read(&bytes, context)?,
            BoardFormat::KiCad => kicad::read(&bytes, context)?,
            BoardFormat::Odb => odb::read(&bytes, context)?,
            BoardFormat::Altium => altium::read(&bytes, context)?,
            BoardFormat::Hfss => hfss::read(&bytes, context)?,
            BoardFormat::Allegro => unreachable!("Allegro is dispatched before source decoding"),
        };
        (context.progress)(ImportProgress {
            stage: ImportStage::BuildingGeometry,
            completed: 0,
            total: Some(output.zones.len() as u64),
        });
        let total = output.zones.len() as u64;
        let mut mesh_bytes = 0usize;
        for (index, zone) in output.zones.into_iter().enumerate() {
            context.check_cancelled()?;
            let mesh = CopperMesh::build(
                &zone.rings,
                &zone.paths,
                &MeshLimits::default(),
                context.cancellation,
            )
            .map_err(|source| ImportError::CopperMesh {
                key: zone.id.0,
                offset: 0,
                source,
            })?;
            mesh_bytes = mesh_bytes.saturating_add(mesh.allocation_bytes());
            if mesh_bytes > 1024 * 1024 * 1024 {
                return Err(ImportError::ResourceLimit {
                    actual: mesh_bytes as u64,
                    limit: 1024 * 1024 * 1024,
                });
            }
            output.scene.zones.push(Zone {
                id: zone.id,
                layer: zone.layer,
                net: zone.net,
                kind: ZoneKind::Unknown,
                paths: zone.paths,
                mesh,
            });
            if index.is_multiple_of(128) || index as u64 + 1 == total {
                (context.progress)(ImportProgress {
                    stage: ImportStage::BuildingGeometry,
                    completed: index as u64 + 1,
                    total: Some(total),
                });
            }
        }
        for layer in output.drawing_layers {
            let class_id = ((layer.id.0 >> 8) & 0xff) as u8;
            let subclass = (layer.id.0 & 0xff) as u8;
            output.scene.drawing_layers.push(DrawingLayer {
                id: layer.id,
                class_id,
                subclass,
                source_name: layer.name,
                color: layer.color,
                default_visible: layer.default_visible,
                class_label: Message::new(MessageKey::DrawingClassBoard),
                subclass_label: Message::new(MessageKey::DrawingSubclassUnknown)
                    .arg("subclass", u32::from(subclass)),
            });
        }
        for layer in output.special_layers {
            output.scene.special_layers.push(SpecialLayer {
                id: layer.id,
                kind: layer.kind,
                color: layer.color,
                label: Message::new(MessageKey::ImportedLayerName).arg("name", layer.name),
            });
        }
        for details in output.diagnostics {
            let mut diagnostic =
                Diagnostic::error("IMPORT_FORMAT_NOTICE", MessageKey::FormatImportNotice)
                    .with_details(details)
                    .with_path(path);
            diagnostic.severity = Severity::Warning;
            output.scene.diagnostics.push(diagnostic);
        }
        if !output.scene.bounds.is_valid() {
            return Err(ImportError::NoGeometry);
        }
        context.check_cancelled()?;
        Ok(ImportedBoard {
            scene: Arc::new(output.scene),
            identity,
            source: FormatProbe {
                format: format.tag().into(),
                layout_version: 0,
                writer_version: output
                    .info
                    .get("version")
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                    })
                    .unwrap_or_default(),
                scene_supported: true,
            },
        })
    }
}

fn decompress(bytes: &[u8], context: &ImportContext<'_>) -> Result<Vec<u8>, ImportError> {
    let mut decoder = flate2::read::MultiGzDecoder::new(bytes);
    let mut result = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        context.check_cancelled()?;
        let count = decoder.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let actual = result.len() as u64 + count as u64;
        if actual > 512 * 1024 * 1024 {
            return Err(ImportError::ResourceLimit {
                actual,
                limit: 512 * 1024 * 1024,
            });
        }
        result.extend_from_slice(&chunk[..count]);
    }
    Ok(result)
}

struct Output {
    scene: BoardScene,
    zones: Vec<SourceZone>,
    drawing_layers: Vec<SourceDrawingLayer>,
    special_layers: Vec<SourceSpecialLayer>,
    diagnostics: Vec<String>,
    info: serde_json::Value,
}

struct SourceZone {
    id: ObjectId,
    layer: LayerId,
    net: NetId,
    paths: Vec<Vec<Segment>>,
    rings: Vec<Vec<Point>>,
}

struct SourceDrawingLayer {
    id: LayerId,
    name: String,
    color: String,
    default_visible: bool,
}

struct SourceSpecialLayer {
    id: LayerId,
    name: String,
    color: String,
    kind: SpecialLayerKind,
}
