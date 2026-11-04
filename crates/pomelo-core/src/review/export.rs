//! Lightweight local export of review results as CSV.
//!
//! Output is UTF-8 with a byte-order mark and CRLF row ends (RFC 4180), so spreadsheet
//! applications read non-ASCII names correctly. Lengths use the chosen unit with a fixed
//! precision. Anonymized output omits the source name and replaces net and component
//! names with stable numeric identities. Exports never modify the source design.
use super::{
    clearance::ClearanceResult,
    measure::{MeasureKind, Measurement},
};
use crate::{
    geometry::PathError,
    i18n::{Locale, Message, MessageKey as Key, text},
    model::{BoardScene, LayerId, NetId, Point},
    selection::SelectedObject,
    task::CancellationToken,
    units::LengthUnit,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy)]
pub struct ExportOptions {
    pub unit: LengthUnit,
    pub anonymize: bool,
    pub locale: Locale,
}

/// One completed clearance query.
#[derive(Debug, Clone, Copy)]
pub struct ClearanceRecord {
    pub first: SelectedObject,
    pub second: SelectedObject,
    pub result: ClearanceResult,
}

pub struct ExportContent<'a> {
    /// Display name of the source file; omitted when anonymizing.
    pub source_name: Option<&'a str>,
    pub measurements: &'a [Measurement],
    pub clearance: Option<ClearanceRecord>,
    pub selection: &'a BTreeSet<SelectedObject>,
}

/// Quote a field when needed and neutralize spreadsheet formula prefixes in text.
pub fn csv_field(value: &str) -> String {
    let formula = value.starts_with(['=', '+', '@', '\t', '\r'])
        || (value.starts_with('-')
            && !value[1..].starts_with(|c: char| c.is_ascii_digit() || c == '.'));
    let value = if formula {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if value.contains([',', '"', '\n', '\r']) || value.starts_with(' ') || value.ends_with(' ') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

struct Table {
    output: String,
}

impl Table {
    fn row<S: AsRef<str>>(&mut self, fields: impl IntoIterator<Item = S>) {
        let fields: Vec<_> = fields
            .into_iter()
            .map(|field| csv_field(field.as_ref()))
            .collect();
        self.output.push_str(&fields.join(","));
        self.output.push_str("\r\n");
    }

    fn blank(&mut self) {
        self.output.push_str("\r\n");
    }
}

fn length(value_mm: f64, unit: LengthUnit) -> String {
    let value = unit.from_millimeters(value_mm);
    match unit {
        LengthUnit::Millimeters => format!("{value:.4}"),
        LengthUnit::Mils => format!("{value:.2}"),
    }
}

fn unit_label(unit: LengthUnit, locale: Locale) -> String {
    text(
        locale,
        match unit {
            LengthUnit::Millimeters => Key::UnitMillimeters,
            LengthUnit::Mils => Key::UnitMils,
        },
    )
}

pub fn object_kind(object: SelectedObject) -> Key {
    match object {
        SelectedObject::Segment(_) => Key::SourceSegment,
        SelectedObject::Pin(_) => Key::SourcePin,
        SelectedObject::Via(_) => Key::SourceVia,
        SelectedObject::Zone(_) => Key::SourceZone,
        SelectedObject::Drawing(_) => Key::SourceDrawing,
        SelectedObject::Placement(_) => Key::ModeComponent,
    }
}

fn object_id(object: SelectedObject) -> u32 {
    match object {
        SelectedObject::Segment(id)
        | SelectedObject::Pin(id)
        | SelectedObject::Via(id)
        | SelectedObject::Zone(id)
        | SelectedObject::Drawing(id)
        | SelectedObject::Placement(id) => id.0,
    }
}

fn describe(object: SelectedObject, locale: Locale) -> String {
    Message::new(Key::ReviewObjectLabel)
        .arg("kind", text(locale, object_kind(object)))
        .arg("id", object_id(object))
        .display(locale)
}

/// Build the review summary: measurements, the last clearance and the selection.
pub fn review_csv(
    scene: &BoardScene,
    content: &ExportContent<'_>,
    options: ExportOptions,
    cancel: &CancellationToken,
) -> Result<String, PathError> {
    let locale = options.locale;
    let unit = options.unit;
    let label = |key| text(locale, key);
    let mut table = Table {
        output: String::from("\u{feff}"),
    };
    table.row([label(Key::ReviewExportTitle)]);
    if let Some(name) = content.source_name.filter(|_| !options.anonymize) {
        table.row([label(Key::ReviewColumnSource), name.to_owned()]);
    }
    table.row([label(Key::ReviewColumnUnit), unit_label(unit, locale)]);
    table.blank();

    table.row([label(Key::ReviewMeasurementsSection)]);
    table.row(
        [
            Key::ReviewColumnItem,
            Key::ReviewColumnKind,
            Key::ReviewColumnLeg,
            Key::ReviewColumnStartX,
            Key::ReviewColumnStartY,
            Key::ReviewColumnEndX,
            Key::ReviewColumnEndY,
            Key::ReviewColumnDeltaX,
            Key::ReviewColumnDeltaY,
            Key::ReviewColumnLength,
            Key::ReviewColumnAngle,
        ]
        .map(label),
    );
    for (index, measurement) in content.measurements.iter().enumerate() {
        cancel.check()?;
        let kind = label(match measurement.kind {
            MeasureKind::Distance => Key::MeasureDistance,
            MeasureKind::Polyline => Key::MeasurePolyline,
        });
        let item = (index + 1).to_string();
        for (leg_index, pair) in measurement.points.windows(2).enumerate() {
            let (a, b) = (pair[0].position, pair[1].position);
            let leg = super::measure::Leg::between(a, b);
            table.row([
                item.clone(),
                kind.clone(),
                (leg_index + 1).to_string(),
                length(a.x, unit),
                length(a.y, unit),
                length(b.x, unit),
                length(b.y, unit),
                length(leg.dx, unit),
                length(leg.dy, unit),
                length(leg.distance, unit),
                format!("{:.2}", leg.angle_degrees),
            ]);
        }
        if measurement.kind == MeasureKind::Polyline && measurement.points.len() > 2 {
            table.row([
                item,
                kind,
                label(Key::ReviewColumnTotal),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                length(measurement.total_length(), unit),
                String::new(),
            ]);
        }
    }
    table.blank();

    if let Some(record) = content.clearance {
        table.row([label(Key::ReviewClearanceSection)]);
        table.row(
            [
                Key::ReviewColumnFirst,
                Key::ReviewColumnSecond,
                Key::ReviewColumnDistance,
                Key::ReviewColumnStartX,
                Key::ReviewColumnStartY,
                Key::ReviewColumnEndX,
                Key::ReviewColumnEndY,
                Key::ReviewColumnTolerance,
            ]
            .map(label),
        );
        let result = record.result;
        table.row([
            describe(record.first, locale),
            describe(record.second, locale),
            length(result.distance_mm, unit),
            length(result.a.x, unit),
            length(result.a.y, unit),
            length(result.b.x, unit),
            length(result.b.y, unit),
            length(result.tolerance_mm, unit),
        ]);
        table.blank();
    }

    table.row([label(Key::ReviewSelectionSection)]);
    table.row(
        [
            Key::ReviewColumnKind,
            Key::ReviewColumnId,
            Key::ReviewColumnNet,
            Key::ReviewColumnLayer,
            Key::ReviewColumnComponent,
            Key::ReviewColumnName,
            Key::ReviewColumnX,
            Key::ReviewColumnY,
        ]
        .map(label),
    );
    selection_rows(scene, content.selection, options, cancel, &mut table)?;
    cancel.check()?;
    Ok(table.output)
}

fn selection_rows(
    scene: &BoardScene,
    selection: &BTreeSet<SelectedObject>,
    options: ExportOptions,
    cancel: &CancellationToken,
    table: &mut Table,
) -> Result<(), PathError> {
    let locale = options.locale;
    let layers: BTreeMap<LayerId, String> = scene
        .layers
        .iter()
        .map(|layer| (layer.id, layer.display_name(locale)))
        .chain(
            scene
                .special_layers
                .iter()
                .map(|layer| (layer.id, layer.display_name(locale))),
        )
        .chain(
            scene
                .drawing_layers
                .iter()
                .map(|layer| (layer.id, layer.display_name(locale))),
        )
        .collect();
    let net = |id: NetId| -> String {
        if id.0 == 0 {
            String::new()
        } else if options.anonymize {
            format!("#{}", id.0)
        } else {
            scene.nets.get(&id).cloned().unwrap_or_default()
        }
    };
    let layer = |id: LayerId| layers.get(&id).cloned().unwrap_or_default();
    let point = |point: Point| [length(point.x, options.unit), length(point.y, options.unit)];
    let mut row = |object: SelectedObject, fields: [String; 4], at: Option<Point>| {
        let [x, y] = at.map_or([String::new(), String::new()], point);
        let [net, layer, component, name] = fields;
        table.row([
            text(locale, object_kind(object)),
            object_id(object).to_string(),
            net,
            layer,
            component,
            name,
            x,
            y,
        ]);
    };
    for segment in &scene.segments {
        cancel.check()?;
        let object = SelectedObject::Segment(segment.id);
        if selection.contains(&object) {
            let fields = [
                net(segment.net),
                layer(segment.layer),
                String::new(),
                String::new(),
            ];
            row(object, fields, Some(segment.a));
        }
    }
    for pin in &scene.pins {
        cancel.check()?;
        let object = SelectedObject::Pin(pin.id);
        if selection.contains(&object) {
            let (component, name) = if options.anonymize {
                (format!("#{}", pin.owner_id.0), String::new())
            } else {
                (pin.reference.clone(), pin.name.clone())
            };
            row(
                object,
                [net(pin.net), String::new(), component, name],
                Some(pin.at),
            );
        }
    }
    for via in &scene.vias {
        cancel.check()?;
        let object = SelectedObject::Via(via.id);
        if selection.contains(&object) {
            let name = if options.anonymize {
                String::new()
            } else {
                via.padstack_name.clone()
            };
            row(
                object,
                [net(via.net), String::new(), String::new(), name],
                Some(via.at),
            );
        }
    }
    for zone in &scene.zones {
        cancel.check()?;
        let object = SelectedObject::Zone(zone.id);
        if selection.contains(&object) {
            row(
                object,
                [
                    net(zone.net),
                    layer(zone.layer),
                    String::new(),
                    String::new(),
                ],
                None,
            );
        }
    }
    for drawing in &scene.drawings {
        cancel.check()?;
        let object = SelectedObject::Drawing(drawing.id);
        if selection.contains(&object) {
            let fields = [
                net(drawing.net),
                layer(drawing.layer),
                String::new(),
                String::new(),
            ];
            row(object, fields, None);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{ObjectId, Segment},
        review::measure::MeasurePoint,
    };

    #[test]
    fn fields_are_quoted_and_formula_prefixes_are_neutralized() {
        assert_eq!(csv_field("GND"), "GND");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("-1.25"), "-1.25");
        assert_eq!(csv_field("-cmd"), "'-cmd");
        assert_eq!(csv_field(" padded"), "\" padded\"");
    }

    fn scene() -> BoardScene {
        let mut scene = crate::review::tests::empty_scene();
        scene.nets.insert(NetId(7), "VCC,3V3".into());
        scene.segments.push(Segment {
            id: ObjectId(4),
            track_id: ObjectId(4),
            layer: LayerId(0),
            net: NetId(7),
            a: Point::new(1.0, 2.0),
            b: Point::new(3.0, 2.0),
            width: 0.1,
            arc: None,
            bond_wire: None,
        });
        scene
    }

    #[test]
    fn summary_lists_measurements_and_selection_in_the_chosen_unit() {
        let mut measurement = Measurement::new(MeasureKind::Distance);
        measurement.push(MeasurePoint::free(Point::new(0.0, 0.0)));
        measurement.push(MeasurePoint::free(Point::new(25.4, 0.0)));
        let selection = BTreeSet::from([SelectedObject::Segment(ObjectId(4))]);
        let options = ExportOptions {
            unit: LengthUnit::Mils,
            anonymize: false,
            locale: Locale::English,
        };
        let csv = review_csv(
            &scene(),
            &ExportContent {
                source_name: Some("board.brd"),
                measurements: &[measurement],
                clearance: None,
                selection: &selection,
            },
            options,
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("board.brd"));
        assert!(csv.contains(",1000.00,0.00"));
        assert!(csv.contains("\"VCC,3V3\""));
        assert!(csv.lines().all(|line| !line.ends_with('\r')));
        assert!(csv.contains("\r\n"));
    }

    #[test]
    fn anonymized_output_omits_source_and_net_names() {
        let selection = BTreeSet::from([SelectedObject::Segment(ObjectId(4))]);
        let csv = review_csv(
            &scene(),
            &ExportContent {
                source_name: Some("secret.brd"),
                measurements: &[],
                clearance: None,
                selection: &selection,
            },
            ExportOptions {
                unit: LengthUnit::Millimeters,
                anonymize: true,
                locale: Locale::English,
            },
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(!csv.contains("secret"));
        assert!(!csv.contains("VCC"));
        assert!(csv.contains("#7"));
        assert!(csv.contains("1.0000,2.0000"));
    }
}
