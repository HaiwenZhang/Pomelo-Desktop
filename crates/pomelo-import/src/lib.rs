//! Local PCB import contracts and Allegro binary reading.

pub mod allegro;
pub mod source;

use std::{path::Path, sync::Arc};

use pomelo_core::{
    i18n::MessageKey,
    model::{BoardScene, Diagnostic},
    task::{CancellationToken, ImportProgress},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("BRD_NO_GEOMETRY")]
    NoGeometry,
    #[error("BRD_COPPER_LAYER_UNDEFINED key={key} layer={layer} offset=0x{offset:x}")]
    CopperLayerUndefined { key: u32, layer: u32, offset: usize },
    #[error("BRD_COPPER_MESH key={key} offset=0x{offset:x}: {source}")]
    CopperMesh {
        key: u32,
        offset: usize,
        source: pomelo_core::copper::MeshError,
    },
    #[error("IMPORT_SEMANTIC_CACHE_LIMIT offset=0x{offset:x} actual={actual} limit={limit}")]
    SemanticCacheLimit {
        offset: usize,
        actual: u64,
        limit: u64,
    },
    #[error("IMPORT_OUT_OF_BOUNDS offset=0x{offset:x} requested={requested} length={length}")]
    OutOfBounds {
        offset: usize,
        requested: usize,
        length: usize,
    },
    #[error("BRD_UNSUPPORTED_MAGIC 0x{0:08x}")]
    UnsupportedMagic(u32),
    #[error("BRD_INVALID_DIVISOR")]
    InvalidDivisor,
    #[error("BRD_UNSUPPORTED_UNITS units={units} divisor={divisor}")]
    UnsupportedUnits { units: u16, divisor: u32 },
    #[error("BRD_INVALID_GEOMETRY key={key} offset=0x{offset:x} field={field}")]
    InvalidGeometry {
        key: u32,
        offset: usize,
        field: &'static str,
    },
    #[error("IMPORT_GEOMETRY_LIMIT offset=0x{offset:x} actual={actual} limit={limit}")]
    GeometryLimit {
        offset: usize,
        actual: u64,
        limit: u64,
    },
    #[error("IMPORT_INVALID_ENCODING offset=0x{0:x}")]
    InvalidEncoding(usize),
    #[error("BRD_DUPLICATE_STRING key={key} offset=0x{offset:x}")]
    DuplicateString { key: u32, offset: usize },
    #[error("BRD_DUPLICATE_RECORD key={key} offset=0x{offset:x}")]
    DuplicateRecord { key: u32, offset: usize },
    #[error("BRD_UNKNOWN_RECORD type=0x{record_type:x} offset=0x{offset:x}")]
    UnknownRecord { record_type: u8, offset: usize },
    #[error("BRD_UNALIGNED_RECORD offset=0x{0:x}")]
    UnalignedRecord(usize),
    #[error("BRD_INVALID_RECORD offset=0x{offset:x} field={field} value={value}")]
    InvalidRecord {
        offset: usize,
        field: &'static str,
        value: u64,
    },
    #[error(
        "BRD_UNSUPPORTED_RECORD_LAYOUT type=0x{record_type:x} version={version} offset=0x{offset:x}"
    )]
    UnsupportedRecordLayout {
        record_type: u8,
        version: u16,
        offset: usize,
    },
    #[error("IMPORT_INDEX_LIMIT actual={actual} limit={limit}")]
    IndexLimit { actual: u64, limit: u64 },
    #[error("IMPORT_DECODE_LIMIT offset=0x{offset:x} actual={actual} limit={limit}")]
    DecodeLimit {
        offset: usize,
        actual: u64,
        limit: u64,
    },
    #[error("BRD_MISSING_REFERENCE key={key} offset=0x{offset:x} field={field}")]
    MissingReference {
        key: u32,
        offset: usize,
        field: &'static str,
    },
    #[error(
        "BRD_REFERENCE_TYPE key={key} actual=0x{actual:x} expected={expected:?} offset=0x{offset:x} field={field}"
    )]
    ReferenceType {
        key: u32,
        actual: u8,
        expected: Vec<u8>,
        offset: usize,
        field: &'static str,
    },
    #[error("BRD_REFERENCE_CYCLE key={key} offset=0x{offset:x} field={field}")]
    ReferenceCycle {
        key: u32,
        offset: usize,
        field: &'static str,
    },
    #[error("IMPORT_CANCELLED")]
    Cancelled,
    #[error("IMPORT_RESOURCE_LIMIT actual={actual} limit={limit}")]
    ResourceLimit { actual: u64, limit: u64 },
    #[error("FILE_IO_FAILED {0}")]
    Io(#[from] std::io::Error),
    #[error("IMPORT_SCENE_NOT_IMPLEMENTED")]
    SceneNotImplemented,
}

impl ImportError {
    /// Keep source positions and typed arguments until the presentation boundary.
    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::NoGeometry => Diagnostic::error("BRD_NO_GEOMETRY", MessageKey::NoGeometry),
            Self::CopperLayerUndefined { key, layer, offset } => {
                let mut diagnostic = Diagnostic::error(
                    "BRD_COPPER_LAYER_UNDEFINED",
                    MessageKey::CopperLayerUndefined,
                );
                diagnostic.message = diagnostic.message.arg("key", *key).arg("layer", *layer);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.object = Some(pomelo_core::model::ObjectId(*key));
                diagnostic
            }
            Self::CopperMesh {
                key,
                offset,
                source,
            } => {
                use pomelo_core::copper::MeshError;
                let mut diagnostic = match source {
                    MeshError::Cancelled => Self::Cancelled.diagnostic(),
                    MeshError::Limit {
                        resource: "bytes",
                        actual,
                        limit,
                    } => Self::GeometryLimit {
                        offset: *offset,
                        actual: *actual as u64,
                        limit: *limit as u64,
                    }
                    .diagnostic(),
                    _ => {
                        let mut diagnostic =
                            Diagnostic::error("BRD_COPPER_MESH", MessageKey::CopperMeshFailed);
                        diagnostic.message = diagnostic.message.arg("key", *key);
                        diagnostic
                    }
                }
                .with_details(source.to_string());
                diagnostic.offset = Some(*offset as u64);
                diagnostic.object = Some(pomelo_core::model::ObjectId(*key));
                diagnostic
            }
            Self::SemanticCacheLimit {
                offset,
                actual,
                limit,
            } => {
                let mut diagnostic = Diagnostic::error(
                    "IMPORT_SEMANTIC_CACHE_LIMIT",
                    MessageKey::SemanticCacheLimit,
                );
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", *actual)
                    .arg("limit", *limit);
                diagnostic
            }
            Self::OutOfBounds {
                offset,
                requested,
                length,
            } => {
                let mut diagnostic =
                    Diagnostic::error("IMPORT_OUT_OF_BOUNDS", MessageKey::OutOfBounds);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("offset", format!("0x{offset:x}"))
                    .arg("requested", *requested)
                    .arg("length", *length);
                diagnostic
            }
            Self::UnsupportedMagic(magic) => {
                let mut diagnostic =
                    Diagnostic::error("BRD_UNSUPPORTED_MAGIC", MessageKey::UnsupportedMagic);
                diagnostic.message = diagnostic.message.arg("magic", format!("0x{magic:08x}"));
                diagnostic
            }
            Self::InvalidDivisor => {
                Diagnostic::error("BRD_INVALID_DIVISOR", MessageKey::InvalidDivisor)
            }
            Self::UnsupportedUnits { units, divisor } => {
                let mut diagnostic =
                    Diagnostic::error("BRD_UNSUPPORTED_UNITS", MessageKey::UnsupportedUnits);
                diagnostic.message = diagnostic
                    .message
                    .arg("units", *units)
                    .arg("divisor", *divisor);
                diagnostic
            }
            Self::InvalidGeometry { key, offset, field } => {
                let mut diagnostic =
                    Diagnostic::error("BRD_INVALID_GEOMETRY", MessageKey::InvalidGeometry)
                        .with_details(format!("field={field}"));
                diagnostic.offset = Some(*offset as u64);
                diagnostic.object = Some(pomelo_core::model::ObjectId(*key));
                diagnostic.message = diagnostic
                    .message
                    .arg("key", *key)
                    .arg("offset", format!("0x{offset:x}"));
                diagnostic
            }
            Self::GeometryLimit {
                offset,
                actual,
                limit,
            } => {
                let mut diagnostic =
                    Diagnostic::error("IMPORT_GEOMETRY_LIMIT", MessageKey::GeometryLimit);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", *actual)
                    .arg("limit", *limit);
                diagnostic
            }
            Self::InvalidEncoding(offset) => {
                let mut diagnostic =
                    Diagnostic::error("IMPORT_INVALID_ENCODING", MessageKey::InvalidEncoding);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic.message.arg("offset", format!("0x{offset:x}"));
                diagnostic
            }
            Self::Cancelled => Diagnostic::error("IMPORT_CANCELLED", MessageKey::Cancelled),
            Self::DuplicateString { key, offset } | Self::DuplicateRecord { key, offset } => {
                let (code, message) = if matches!(self, Self::DuplicateString { .. }) {
                    ("BRD_DUPLICATE_STRING", MessageKey::DuplicateString)
                } else {
                    ("BRD_DUPLICATE_RECORD", MessageKey::DuplicateRecord)
                };
                let mut diagnostic = Diagnostic::error(code, message);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("key", *key)
                    .arg("offset", format!("0x{offset:x}"));
                diagnostic
            }
            Self::UnknownRecord {
                record_type,
                offset,
            } => {
                let mut diagnostic =
                    Diagnostic::error("BRD_UNKNOWN_RECORD", MessageKey::UnknownRecord);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("record_type", format!("0x{record_type:x}"))
                    .arg("offset", format!("0x{offset:x}"));
                diagnostic
            }
            Self::UnalignedRecord(offset) | Self::InvalidRecord { offset, .. } => {
                let (code, message) = if matches!(self, Self::UnalignedRecord(_)) {
                    ("BRD_UNALIGNED_RECORD", MessageKey::UnalignedRecord)
                } else {
                    ("BRD_INVALID_RECORD", MessageKey::InvalidRecord)
                };
                let mut diagnostic = Diagnostic::error(code, message);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic.message.arg("offset", format!("0x{offset:x}"));
                if let Self::InvalidRecord { field, value, .. } = self {
                    diagnostic = diagnostic.with_details(format!("{field}={value}"));
                }
                diagnostic
            }
            Self::UnsupportedRecordLayout {
                record_type,
                version,
                offset,
            } => {
                let mut diagnostic = Diagnostic::error(
                    "BRD_UNSUPPORTED_RECORD_LAYOUT",
                    MessageKey::UnsupportedRecordLayout,
                );
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("record_type", format!("0x{record_type:x}"))
                    .arg("version", *version)
                    .arg("offset", format!("0x{offset:x}"));
                diagnostic
            }
            Self::IndexLimit { actual, limit } => {
                let mut diagnostic =
                    Diagnostic::error("IMPORT_INDEX_LIMIT", MessageKey::IndexLimit);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", *actual)
                    .arg("limit", *limit);
                diagnostic
            }
            Self::DecodeLimit {
                offset,
                actual,
                limit,
            } => {
                let mut diagnostic =
                    Diagnostic::error("IMPORT_DECODE_LIMIT", MessageKey::DecodeLimit);
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", *actual)
                    .arg("limit", *limit);
                diagnostic
            }
            Self::MissingReference { key, offset, field }
            | Self::ReferenceCycle { key, offset, field }
            | Self::ReferenceType {
                key, offset, field, ..
            } => {
                let (code, message) = match self {
                    Self::MissingReference { .. } => {
                        ("BRD_MISSING_REFERENCE", MessageKey::MissingReference)
                    }
                    Self::ReferenceCycle { .. } => {
                        ("BRD_REFERENCE_CYCLE", MessageKey::ReferenceCycle)
                    }
                    _ => ("BRD_REFERENCE_TYPE", MessageKey::ReferenceType),
                };
                let mut diagnostic =
                    Diagnostic::error(code, message).with_details(format!("field={field}"));
                diagnostic.offset = Some(*offset as u64);
                diagnostic.message = diagnostic
                    .message
                    .arg("key", *key)
                    .arg("offset", format!("0x{offset:x}"));
                if let Self::ReferenceType {
                    actual, expected, ..
                } = self
                {
                    let expected = expected
                        .iter()
                        .map(|kind| format!("0x{kind:x}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    diagnostic.message = diagnostic
                        .message
                        .arg("actual", format!("0x{actual:x}"))
                        .arg("expected", expected);
                }
                diagnostic
            }
            Self::ResourceLimit { actual, limit } => {
                let mut diagnostic =
                    Diagnostic::error("IMPORT_RESOURCE_LIMIT", MessageKey::ResourceLimit);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", *actual)
                    .arg("limit", *limit);
                diagnostic
            }
            Self::Io(error) => {
                let key = match error.kind() {
                    std::io::ErrorKind::NotFound => MessageKey::FileNotFound,
                    std::io::ErrorKind::PermissionDenied => MessageKey::PermissionDenied,
                    _ => MessageKey::FileIoFailed,
                };
                Diagnostic::error("FILE_IO_FAILED", key).with_details(error.to_string())
            }
            Self::SceneNotImplemented => Diagnostic::error(
                "IMPORT_SCENE_NOT_IMPLEMENTED",
                MessageKey::SceneNotImplemented,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum TextEncoding {
    #[default]
    Auto,
    Utf8,
    Gbk,
    ShiftJis,
    Big5,
    Windows1252,
}

impl TextEncoding {
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Utf8 => "utf-8",
            Self::Gbk => "gbk",
            Self::ShiftJis => "shift_jis",
            Self::Big5 => "big5",
            Self::Windows1252 => "windows-1252",
        }
    }
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "auto" => Some(Self::Auto),
            "utf-8" => Some(Self::Utf8),
            "gbk" => Some(Self::Gbk),
            "shift_jis" => Some(Self::ShiftJis),
            "big5" => Some(Self::Big5),
            "windows-1252" => Some(Self::Windows1252),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub text_encoding: TextEncoding,
    pub max_file_bytes: u64,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            text_encoding: TextEncoding::Auto,
            max_file_bytes: 1024 * 1024 * 1024,
        }
    }
}

pub struct ImportContext<'a> {
    pub cancellation: &'a CancellationToken,
    pub progress: &'a dyn Fn(ImportProgress),
}

impl ImportContext<'_> {
    pub fn check_cancelled(&self) -> Result<(), ImportError> {
        if self.cancellation.is_cancelled() {
            Err(ImportError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FormatProbe {
    pub format: String,
    pub layout_version: u16,
    pub writer_version: String,
    /// Header recognition does not imply complete scene support.
    pub scene_supported: bool,
}

pub struct ImportedBoard {
    pub scene: Arc<BoardScene>,
    pub source: FormatProbe,
    pub identity: pomelo_core::view_state::SourceIdentity,
}

/// Importers own format knowledge; the application only receives board models.
pub trait BoardImporter: Send + Sync {
    fn probe(&self, bytes: &[u8], options: &ImportOptions) -> Result<FormatProbe, ImportError>;
    fn import(
        &self,
        path: &Path,
        options: &ImportOptions,
        context: &ImportContext<'_>,
    ) -> Result<ImportedBoard, ImportError>;
}
