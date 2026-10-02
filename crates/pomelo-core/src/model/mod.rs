//! Board coordinates are millimetres and angles are radians.

use std::{collections::BTreeMap, sync::Arc as Shared};

use crate::i18n::{Locale, Message, MessageKey};
use serde::{Deserialize, Serialize};

macro_rules! id {
    ($name:ident) => {
        #[derive(
            Debug,
            Clone,
            Copy,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);
    };
}

id!(LayerId);
id!(NetId);
id!(ObjectId);

impl LayerId {
    /// Temporary path edges have no physical layer until their owner assigns one.
    pub const UNASSIGNED: Self = Self(u32::MAX);
    pub const BOND_TOP: Self = Self(0x20000);
    pub const BOND_WIRE_TOP: Self = Self(0x20001);
    pub const DIMENSION: Self = Self(0x1f901);
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: Point,
    pub max: Point,
}

impl Bounds {
    pub fn from_points(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut points = points.into_iter();
        let first = points.next()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for point in points {
            bounds.include(point);
        }
        bounds.is_valid().then_some(bounds)
    }

    pub fn include(&mut self, point: Point) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
    }

    pub fn is_valid(self) -> bool {
        [self.min.x, self.min.y, self.max.x, self.max.y]
            .into_iter()
            .all(f64::is_finite)
            && self.min.x <= self.max.x
            && self.min.y <= self.max.y
    }

    pub fn center(self) -> Point {
        Point::new(
            self.min.x * 0.5 + self.max.x * 0.5,
            self.min.y * 0.5 + self.max.y * 0.5,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerFunction {
    Conductor,
    Plane,
    Dielectric,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    /// Original source name; absent names stay empty until the presentation boundary.
    pub name: String,
    pub function: LayerFunction,
    pub color: String,
    pub source_flags: Option<u32>,
}

impl Layer {
    pub fn display_name(&self, locale: Locale) -> String {
        if self.name.is_empty() {
            Message::new(MessageKey::DefaultLayerName)
                .arg("index", u64::from(self.id.0) + 1)
                .display(locale)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Arc {
    pub center: Point,
    pub radius: f64,
    pub start: f64,
    pub sweep: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub id: ObjectId,
    pub track_id: ObjectId,
    pub layer: LayerId,
    pub net: NetId,
    pub a: Point,
    pub b: Point,
    pub width: f64,
    pub arc: Option<Arc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bond_wire: Option<BondWireInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondWireInfo {
    pub profile: String,
    pub material: Option<String>,
    pub source_pin: ObjectId,
    pub finger: ObjectId,
    pub reference: String,
    pub pin_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pad {
    pub layer: LayerId,
    pub width: f64,
    pub height: f64,
    pub offset: Point,
    /// Shape family shared with the Web geometry contract. Unknown families remain diagnostic.
    pub kind: PadKind,
    pub corner: f64,
    pub inner_diameter: Option<f64>,
    pub custom: Option<Shared<CustomPadGeometry>>,
    pub backdrill: bool,
    pub backdrill_base: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PadKind(pub u16);

impl PadKind {
    pub const CIRCLE: Self = Self(2);
    pub const CUSTOM: Self = Self(22);
    pub const DONUT: Self = Self(25);

    pub fn is_analytic(self) -> bool {
        matches!(self.0, 2 | 3 | 5 | 6 | 11 | 12 | 25 | 27 | 28)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomPadGeometry {
    pub contours: Vec<Vec<Point>>,
    /// Analytic boundaries retained alongside the fill contours.
    pub paths: Vec<Vec<Segment>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DrillShape {
    pub width: f64,
    pub height: f64,
    pub plated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BackdrillSpan {
    pub start_layer: LayerId,
    pub stop_layer: LayerId,
    pub protected_layer: LayerId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackdrillDefinition {
    pub spans: Vec<BackdrillSpan>,
    pub display_diameter: f64,
    pub start_pad_diameter: f64,
    pub label_diameter: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backdrill {
    pub definition: BackdrillDefinition,
    pub source_reference: ObjectId,
    /// Source placement uses degrees; all other board geometry angles are radians.
    pub rotation_degrees: f64,
    pub mirrored: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StackupRegion {
    pub source_reference: ObjectId,
    pub code: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pin {
    pub id: ObjectId,
    pub owner_id: ObjectId,
    pub net: NetId,
    pub name: String,
    pub reference: String,
    pub at: Point,
    pub angle: f64,
    pub mirrored: bool,
    pub drill: f64,
    pub drill_shape: DrillShape,
    pub pads: Vec<Pad>,
    pub stackup_region: Option<StackupRegion>,
    pub die: Option<DiePad>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiePad {
    pub source_reference: ObjectId,
    pub padstack_name: String,
}

/// One footprint instance; identities remain distinct even when references are duplicated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPlacement {
    pub id: ObjectId,
    pub source_reference: Option<ObjectId>,
    pub reference: String,
    pub at: Point,
    pub angle: f64,
    pub mirrored: bool,
    pub pins: Vec<ObjectId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Via {
    pub id: ObjectId,
    pub net: NetId,
    pub at: Point,
    pub drill: f64,
    pub drill_shape: DrillShape,
    pub padstack: ObjectId,
    pub padstack_name: String,
    /// Empty source padstacks retain object identity without claiming a physical span.
    pub start_layer: Option<LayerId>,
    pub end_layer: Option<LayerId>,
    pub pads: Shared<[Pad]>,
    pub backdrill: Option<Backdrill>,
    pub stackup_region: Option<StackupRegion>,
    pub angle: f64,
    pub mirrored: bool,
    pub finger: Option<BondFinger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondFinger {
    pub reference: String,
    pub name: String,
    pub source_pin: Option<ObjectId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Zone {
    pub id: ObjectId,
    pub layer: LayerId,
    pub net: NetId,
    /// Exact boundaries remain available for curved edge rendering and picking.
    pub paths: Vec<Vec<Segment>>,
    /// Outer coverage and hole coverage must be composed separately on the GPU.
    pub mesh: crate::copper::CopperMesh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardText {
    pub id: ObjectId,
    pub owner_id: Option<ObjectId>,
    pub layer: LayerId,
    pub class_id: u8,
    pub subclass: u8,
    pub text: String,
    pub at: Point,
    pub angle: f64,
    pub mirrored: bool,
    pub align: TextAlignment,
    pub font_index: u8,
    pub width: f64,
    pub height: f64,
    pub spacing: f64,
    pub line_spacing: f64,
    pub stroke_width: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlignment {
    Left,
    Right,
    Center,
}

/// Source layer identity is independent of the UI language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawingLayer {
    pub id: LayerId,
    pub class_id: u8,
    pub subclass: u8,
    pub source_name: String,
    pub color: String,
    pub default_visible: bool,
    pub class_label: Message,
    pub subclass_label: Message,
}

impl DrawingLayer {
    pub fn display_name(&self, locale: Locale) -> String {
        let subclass = if self.source_name.is_empty() {
            self.subclass_label.display(locale)
        } else {
            self.source_name.clone()
        };
        format!("{} · {subclass}", self.class_label.display(locale))
    }
}

/// Stored dimension geometry; text IDs refer to BoardScene::texts without duplicating strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardDrawing {
    pub id: ObjectId,
    pub owner_id: Option<ObjectId>,
    pub layer: LayerId,
    pub net: NetId,
    pub graphic_ids: Vec<ObjectId>,
    pub segments: Vec<Segment>,
    pub text_ids: Vec<ObjectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpecialLayerKind {
    BondWire,
    DiePad,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialLayer {
    pub id: LayerId,
    pub kind: SpecialLayerKind,
    pub color: String,
    pub label: Message,
}
impl SpecialLayer {
    pub fn display_name(&self, locale: Locale) -> String {
        self.label.display(locale)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: Box<str>,
    pub severity: Severity,
    pub message: Message,
    pub offset: Option<u64>,
    pub object: Option<ObjectId>,
    pub path: Option<Shared<std::path::Path>>,
    /// Raw third-party details are kept separately from translated user summaries.
    pub technical_details: Option<Box<str>>,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, key: MessageKey) -> Self {
        Self {
            code: code.into().into_boxed_str(),
            severity: Severity::Error,
            message: Message::new(key),
            offset: None,
            object: None,
            path: None,
            technical_details: None,
        }
    }
    pub fn with_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.path = Some(Shared::from(path.into().into_boxed_path()));
        self
    }
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.technical_details = Some(details.into().into_boxed_str());
        self
    }
}

/// An immutable snapshot after import. Consumers share it with `Arc`.
#[derive(Debug, Serialize, Deserialize)]
pub struct BoardScene {
    pub layers: Vec<Layer>,
    pub special_layers: Vec<SpecialLayer>,
    pub nets: BTreeMap<NetId, String>,
    pub segments: Vec<Segment>,
    pub pins: Vec<Pin>,
    pub components: Vec<ComponentPlacement>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    pub outline: Vec<Segment>,
    pub texts: Vec<BoardText>,
    pub drawing_layers: Vec<DrawingLayer>,
    pub drawings: Vec<BoardDrawing>,
    pub bounds: Bounds,
    pub diagnostics: Vec<Diagnostic>,
}
