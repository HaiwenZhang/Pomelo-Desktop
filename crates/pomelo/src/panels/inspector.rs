//! Prepare source inspector fields independently of viewport interaction and GPU drawing.
//! Messages retain keys and raw source parameters until rendered in the selected language.

use pomelo_core::{
    i18n::{Message, MessageKey as Key},
    model::BoardScene,
};
use std::collections::BTreeSet;

pub(crate) fn property_row(
    label: String,
    value: String,
    cx: &gpui_kit::App,
) -> gpui_kit::AnyElement {
    use gpui_kit::{component::ActiveTheme, *};
    div()
        .flex()
        .items_start()
        .min_w_0()
        .gap_3()
        .py_0p5()
        .text_sm()
        .child(
            div()
                .w(relative(0.42))
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().text_right().child(value))
        .into_any_element()
}

/// Split only known typed fields; other source diagnostics retain their full message.
pub(crate) fn source_property(
    message: &Message,
    locale: pomelo_core::i18n::Locale,
) -> Option<(String, String)> {
    let label = match message.key {
        Key::SourceNetName => Key::PropertyNet,
        Key::SourceReference => Key::PropertyReference,
        Key::SourcePinName => Key::PropertyPinName,
        Key::SourcePadstackName => Key::PropertyPadstack,
        _ => return None,
    };
    Some((
        pomelo_core::i18n::text(locale, label),
        message.args.get("name")?.to_string(),
    ))
}

/// Format retained millimeter fields at render time so background results stay unit-neutral.
pub(crate) fn display_source_field(
    message: &Message,
    locale: pomelo_core::i18n::Locale,
    unit: pomelo_core::units::LengthUnit,
) -> String {
    use pomelo_core::i18n::MessageArg;
    let key = match message.key {
        Key::SourcePosition => Key::SourcePositionMils,
        Key::SourceStart => Key::SourceStartMils,
        Key::SourceEnd => Key::SourceEndMils,
        Key::SourceDrillSize => Key::SourceDrillSizeMils,
        Key::SourceTraceWidth => Key::SourceTraceWidthMils,
        Key::SourceBackdrillDisplayDiameter => Key::SourceBackdrillDisplayDiameterMils,
        _ => return message.display(locale),
    };
    let key = if unit == pomelo_core::units::LengthUnit::Millimeters {
        message.key
    } else {
        key
    };
    let mut converted = Message::new(key);
    for (name, value) in &message.args {
        let MessageArg::Text(value) = value else {
            return message.display(locale);
        };
        let Ok(value) = value.parse::<f64>() else {
            return message.display(locale);
        };
        if !value.is_finite() {
            return message.display(locale);
        }
        converted = converted.arg(name, format!("{:.6}", unit.from_millimeters(value)));
    }
    converted.display(locale)
}

pub(crate) struct PreparedInspection {
    pub related_objects: std::sync::Arc<BTreeSet<pomelo_core::selection::SelectedObject>>,
    pub labels: Vec<Message>,
    pub net: Option<pomelo_core::model::NetId>,
    pub component: Option<pomelo_core::model::ObjectId>,
}

pub(crate) fn prepare_inspection(
    target: pomelo_core::selection::SelectionTarget,
    anchor: Option<pomelo_core::picking_index::SelectionAnchor>,
    scene: &BoardScene,
    cancel: &pomelo_core::task::CancellationToken,
) -> Result<PreparedInspection, pomelo_core::geometry::PathError> {
    cancel.check()?;
    let mut labels = selection_source_labels(target, scene, cancel);
    // A single-object click anchors on the target itself; its labels are already here.
    if let Some(anchor) = anchor
        .filter(|anchor| target != pomelo_core::selection::SelectionTarget::Object(anchor.object))
    {
        for field in selection_source_labels(
            pomelo_core::selection::SelectionTarget::Object(anchor.object),
            scene,
            cancel,
        ) {
            if !labels.contains(&field) {
                labels.push(field);
            }
        }
    }
    let prepared = PreparedInspection {
        related_objects: std::sync::Arc::new(
            if matches!(
                target,
                pomelo_core::selection::SelectionTarget::Component(_)
                    | pomelo_core::selection::SelectionTarget::ComponentGroup(_)
            ) {
                target
                    .related_bond_objects(scene, cancel)?
                    .into_iter()
                    .chain(
                        target
                            .placement_ids(scene, cancel)?
                            .into_iter()
                            .map(pomelo_core::selection::SelectedObject::Placement),
                    )
                    .collect()
            } else {
                BTreeSet::new()
            },
        ),
        labels,
        net: related_net(target, scene, cancel),
        component: related_component(target, scene, cancel)?,
    };
    cancel.check()?;
    Ok(prepared)
}

pub(crate) fn hit_layer_name(
    layer: pomelo_core::model::LayerId,
    scene: &BoardScene,
    locale: pomelo_core::i18n::Locale,
) -> String {
    if layer == pomelo_core::model::LayerId::UNASSIGNED {
        return pomelo_core::i18n::text(locale, Key::DrillLayer);
    }
    if let Some(value) = scene.layers.iter().find(|value| value.id == layer) {
        return value.display_name(locale);
    }
    if let Some(value) = scene.drawing_layers.iter().find(|value| value.id == layer) {
        return value.display_name(locale);
    }
    if let Some(value) = scene.special_layers.iter().find(|value| value.id == layer) {
        return value.display_name(locale);
    }
    Message::new(Key::SourceLayerId)
        .arg("id", layer.0)
        .display(locale)
}

pub(crate) fn related_component(
    target: pomelo_core::selection::SelectionTarget,
    scene: &BoardScene,
    cancel: &pomelo_core::task::CancellationToken,
) -> Result<Option<pomelo_core::model::ObjectId>, pomelo_core::geometry::PathError> {
    use pomelo_core::selection::SelectionTarget;
    let SelectionTarget::Object(object) = target else {
        return Ok(None);
    };
    object
        .resolve(
            scene,
            pomelo_core::interaction::SelectionMode::Component,
            cancel,
        )
        .map(|target| match target {
            Some(SelectionTarget::Component(id)) => Some(id),
            _ => None,
        })
}

pub(crate) fn related_net(
    target: pomelo_core::selection::SelectionTarget,
    scene: &BoardScene,
    cancel: &pomelo_core::task::CancellationToken,
) -> Option<pomelo_core::model::NetId> {
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    let net = match target {
        SelectionTarget::Object(SelectedObject::Segment(id)) => scene
            .segments
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.net),
        SelectionTarget::Object(SelectedObject::Pin(id)) => scene
            .pins
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.net),
        SelectionTarget::Object(SelectedObject::Via(id)) => scene
            .vias
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.net),
        SelectionTarget::Object(SelectedObject::Zone(id)) => scene
            .zones
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.net),
        _ => None,
    };
    net.filter(|net| net.0 != 0)
}

pub(crate) fn selection_identity(target: pomelo_core::selection::SelectionTarget) -> (Key, u32) {
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    match target {
        SelectionTarget::Object(SelectedObject::Segment(id)) => (Key::SourceSegment, id.0),
        SelectionTarget::Object(SelectedObject::Pin(id)) => (Key::SourcePin, id.0),
        SelectionTarget::Object(SelectedObject::Via(id)) => (Key::SourceVia, id.0),
        SelectionTarget::Object(SelectedObject::Zone(id)) => (Key::SourceZone, id.0),
        SelectionTarget::Object(SelectedObject::Drawing(id)) => (Key::SourceDrawing, id.0),
        SelectionTarget::Track(id) => (Key::ModeTrack, id.0),
        SelectionTarget::Net(id) => (Key::ModeNet, id.0),
        SelectionTarget::Component(id) | SelectionTarget::Object(SelectedObject::Placement(id)) => {
            (Key::ModeComponent, id.0)
        }
        SelectionTarget::ComponentGroup(anchor) => (Key::ModeComponent, anchor.id().0),
    }
}

/// One typed source object, looked up once per inspection target so each field
/// below reads it directly instead of rescanning the scene by ID.
#[derive(Clone, Copy)]
enum SourceObject<'a> {
    Component(&'a pomelo_core::model::ComponentPlacement),
    Pin(&'a pomelo_core::model::Pin),
    Via(&'a pomelo_core::model::Via),
    Segment(&'a pomelo_core::model::Segment),
    Zone(&'a pomelo_core::model::Zone),
    Drawing(&'a pomelo_core::model::BoardDrawing),
}

fn find_by_id<'a, T>(
    items: &'a [T],
    id: pomelo_core::model::ObjectId,
    item_id: impl Fn(&T) -> pomelo_core::model::ObjectId,
    cancel: &pomelo_core::task::CancellationToken,
) -> Option<&'a T> {
    items
        .iter()
        .take_while(|_| !cancel.is_cancelled())
        .find(|item| item_id(item) == id)
}

fn resolve_source<'a>(
    target: pomelo_core::selection::SelectionTarget,
    scene: &'a BoardScene,
    cancel: &pomelo_core::task::CancellationToken,
) -> Option<SourceObject<'a>> {
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    match target {
        SelectionTarget::Component(id) | SelectionTarget::Object(SelectedObject::Placement(id)) => {
            find_by_id(&scene.components, id, |c| c.id, cancel).map(SourceObject::Component)
        }
        SelectionTarget::Object(SelectedObject::Pin(id)) => {
            find_by_id(&scene.pins, id, |p| p.id, cancel).map(SourceObject::Pin)
        }
        SelectionTarget::Object(SelectedObject::Via(id)) => {
            find_by_id(&scene.vias, id, |v| v.id, cancel).map(SourceObject::Via)
        }
        SelectionTarget::Object(SelectedObject::Segment(id)) => {
            find_by_id(&scene.segments, id, |s| s.id, cancel).map(SourceObject::Segment)
        }
        SelectionTarget::Object(SelectedObject::Zone(id)) => {
            find_by_id(&scene.zones, id, |z| z.id, cancel).map(SourceObject::Zone)
        }
        SelectionTarget::Object(SelectedObject::Drawing(id)) => {
            find_by_id(&scene.drawings, id, |d| d.id, cancel).map(SourceObject::Drawing)
        }
        SelectionTarget::Net(_)
        | SelectionTarget::Track(_)
        | SelectionTarget::ComponentGroup(_) => None,
    }
}

fn push_name(labels: &mut Vec<Message>, key: Key, value: &str) {
    if !value.is_empty() {
        labels.push(Message::new(key).arg("name", value));
    }
}

fn push_point(labels: &mut Vec<Message>, key: Key, point: pomelo_core::model::Point) {
    if point.x.is_finite() && point.y.is_finite() {
        labels.push(
            Message::new(key)
                .arg("x", point.x.to_string())
                .arg("y", point.y.to_string()),
        );
    }
}

pub(crate) fn selection_source_labels(
    target: pomelo_core::selection::SelectionTarget,
    scene: &BoardScene,
    cancel: &pomelo_core::task::CancellationToken,
) -> Vec<Message> {
    use pomelo_core::selection::SelectionTarget;
    let mut labels = Vec::new();
    let net_name = |net| scene.nets.get(&net).map(String::as_str);
    match target {
        SelectionTarget::ComponentGroup(anchor) => {
            if let Ok(Some(reference)) = anchor.reference(scene, cancel) {
                push_name(&mut labels, Key::SourceReference, reference);
            }
            return labels;
        }
        SelectionTarget::Net(id) => {
            if let Some(name) = net_name(id) {
                push_name(&mut labels, Key::SourceNetName, name);
            }
            return labels;
        }
        _ => {}
    }
    let Some(source) = resolve_source(target, scene, cancel) else {
        return labels;
    };
    match source {
        SourceObject::Component(component) => {
            push_name(&mut labels, Key::SourceReference, &component.reference);
        }
        SourceObject::Pin(pin) => {
            push_name(&mut labels, Key::SourceReference, &pin.reference);
            push_name(&mut labels, Key::SourcePinName, &pin.name);
            if let Some(name) = net_name(pin.net) {
                push_name(&mut labels, Key::SourceNetName, name);
            }
        }
        SourceObject::Via(via) => {
            push_name(&mut labels, Key::SourcePadstackName, &via.padstack_name);
            if let Some(name) = net_name(via.net) {
                push_name(&mut labels, Key::SourceNetName, name);
            }
        }
        SourceObject::Segment(segment) => {
            if let Some(name) = net_name(segment.net) {
                push_name(&mut labels, Key::SourceNetName, name);
            }
        }
        SourceObject::Zone(zone) => {
            if let Some(name) = net_name(zone.net) {
                push_name(&mut labels, Key::SourceNetName, name);
            }
        }
        SourceObject::Drawing(drawing) => {
            for source in scene
                .texts
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .filter(|text| drawing.text_ids.contains(&text.id))
                .take(16)
            {
                push_name(&mut labels, Key::SourceText, &source.text);
            }
        }
    }
    match source {
        SourceObject::Component(component) => {
            push_point(&mut labels, Key::SourcePosition, component.at);
        }
        SourceObject::Pin(pin) => push_point(&mut labels, Key::SourcePosition, pin.at),
        SourceObject::Via(via) => push_point(&mut labels, Key::SourcePosition, via.at),
        SourceObject::Segment(segment) => {
            push_point(&mut labels, Key::SourceStart, segment.a);
            push_point(&mut labels, Key::SourceEnd, segment.b);
        }
        SourceObject::Zone(_) | SourceObject::Drawing(_) => {}
    }
    let drill = match source {
        SourceObject::Pin(pin) => Some(pin.drill_shape),
        SourceObject::Via(via) => Some(via.drill_shape),
        _ => None,
    };
    if let Some(drill) = drill.filter(|drill| {
        drill.width.is_finite()
            && drill.height.is_finite()
            && drill.width > 0.0
            && drill.height > 0.0
    }) {
        labels.push(
            Message::new(Key::SourceDrillSize)
                .arg("width", drill.width.to_string())
                .arg("height", drill.height.to_string()),
        );
    }
    if let SourceObject::Segment(segment) = source
        && segment.width.is_finite()
        && segment.width >= 0.0
    {
        labels.push(Message::new(Key::SourceTraceWidth).arg("width", segment.width.to_string()));
    }
    let placement = match source {
        SourceObject::Component(component) => Some((component.angle, component.mirrored)),
        SourceObject::Pin(pin) => Some((pin.angle, pin.mirrored)),
        SourceObject::Via(via) => Some((via.angle, via.mirrored)),
        _ => None,
    };
    if let Some((angle, mirrored)) = placement {
        let degrees = angle.to_degrees();
        if degrees.is_finite() {
            labels.push(Message::new(Key::SourceRotation).arg("degrees", format!("{degrees:.6}")));
        }
        labels.push(Message::new(if mirrored {
            Key::SourceMirrored
        } else {
            Key::SourceUnmirrored
        }));
    }
    let layers: BTreeSet<_> = match source {
        SourceObject::Pin(pin) => pin.pads.iter().map(|pad| pad.layer).collect(),
        SourceObject::Via(via) => via.pads.iter().map(|pad| pad.layer).collect(),
        SourceObject::Segment(segment) => BTreeSet::from([segment.layer]),
        SourceObject::Zone(zone) => BTreeSet::from([zone.layer]),
        SourceObject::Drawing(drawing) => BTreeSet::from([drawing.layer]),
        SourceObject::Component(_) => BTreeSet::new(),
    };
    for layer in layers {
        if cancel.is_cancelled() {
            return labels;
        }
        let name = scene
            .layers
            .iter()
            .find(|definition| definition.id == layer)
            .map(|definition| definition.name.as_str())
            .filter(|name| !name.is_empty());
        labels.push(if let Some(name) = name {
            Message::new(Key::SourceLayerName)
                .arg("id", layer.0)
                .arg("name", name)
        } else {
            Message::new(Key::SourceLayerId).arg("id", layer.0)
        });
    }
    if let SourceObject::Via(via) = source
        && let Some(backdrill) = via.backdrill.as_ref()
    {
        labels.push(
            Message::new(Key::SourceBackdrillReference).arg("id", backdrill.source_reference.0),
        );
        let diameter = backdrill.definition.display_diameter;
        if diameter.is_finite() && diameter > 0.0 {
            labels.push(
                Message::new(Key::SourceBackdrillDisplayDiameter)
                    .arg("diameter", diameter.to_string()),
            );
        }
        for span in &backdrill.definition.spans {
            if cancel.is_cancelled() {
                return labels;
            }
            labels.push(
                Message::new(Key::SourceBackdrillSpan)
                    .arg("start", span.start_layer.0)
                    .arg("stop", span.stop_layer.0)
                    .arg("protected", span.protected_layer.0),
            );
        }
    }
    if let SourceObject::Segment(segment) = source
        && let Some(wire) = segment.bond_wire.as_ref()
    {
        labels.push(Message::new(Key::SourceBondPin).arg("id", wire.source_pin.0));
        labels.push(Message::new(Key::SourceBondFinger).arg("id", wire.finger.0));
        for (key, name) in [
            (Key::SourceBondProfile, wire.profile.as_str()),
            (
                Key::SourceBondMaterial,
                wire.material.as_deref().unwrap_or_default(),
            ),
            (Key::SourceReference, wire.reference.as_str()),
            (Key::SourcePinName, wire.pin_name.as_str()),
        ] {
            push_name(&mut labels, key, name);
        }
    }
    if let SourceObject::Via(via) = source
        && let Some(finger) = via.finger.as_ref()
    {
        if let Some(pin) = finger.source_pin {
            labels.push(Message::new(Key::SourceBondPin).arg("id", pin.0));
        }
        for (key, name) in [
            (Key::SourceReference, finger.reference.as_str()),
            (Key::SourceFingerName, finger.name.as_str()),
        ] {
            push_name(&mut labels, key, name);
        }
    }
    labels
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use pomelo_core::{
        model::{Bounds, DrillShape, LayerId, NetId, ObjectId, Pad, Pin, Point},
        selection::{SelectedObject, SelectionTarget},
        task::CancellationToken,
    };

    fn pin_scene() -> BoardScene {
        BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: [(NetId(7), "GND".to_owned())].into(),
            segments: vec![],
            pins: vec![Pin {
                id: ObjectId(3),
                owner_id: ObjectId(1),
                net: NetId(7),
                name: "1".into(),
                reference: "U1".into(),
                at: Point::new(1.0, 2.0),
                angle: 0.0,
                mirrored: false,
                drill: 0.3,
                drill_shape: DrillShape {
                    rectangular: false,
                    width: 0.3,
                    height: 0.3,
                    plated: true,
                },
                pads: vec![Pad::circle(LayerId(2), 1.0), Pad::circle(LayerId(1), 1.0)],
                stackup_region: None,
                die: None,
            }],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            bounds: Bounds {
                min: Point::default(),
                max: Point::new(3.0, 3.0),
            },
            diagnostics: vec![],
        }
    }

    #[test]
    fn pin_labels_keep_their_field_order() {
        let scene = pin_scene();
        let target = SelectionTarget::Object(SelectedObject::Pin(ObjectId(3)));
        let keys: Vec<_> = selection_source_labels(target, &scene, &CancellationToken::default())
            .into_iter()
            .map(|message| message.key)
            .collect();
        assert_eq!(
            keys,
            [
                Key::SourceReference,
                Key::SourcePinName,
                Key::SourceNetName,
                Key::SourcePosition,
                Key::SourceDrillSize,
                Key::SourceRotation,
                Key::SourceUnmirrored,
                Key::SourceLayerId,
                Key::SourceLayerId,
            ]
        );
    }

    #[test]
    fn placement_anchor_inspects_the_component_and_highlights_its_own_area() {
        use pomelo_core::{
            display::DisplayCategory,
            model::{ComponentBody, ComponentPlacement},
            picking_index::SelectionAnchor,
        };
        let mut scene = pin_scene();
        scene.components.push(ComponentPlacement {
            id: ObjectId(1),
            source_reference: None,
            reference: "U1".into(),
            at: Point::new(1.0, 2.0),
            angle: 0.0,
            mirrored: false,
            pins: vec![ObjectId(3)],
            body: Some(ComponentBody {
                layer: LayerId(2),
                regions: vec![],
            }),
        });
        let cancel = CancellationToken::default();
        let anchor = SelectionAnchor {
            object: SelectedObject::Placement(ObjectId(1)),
            layer: LayerId(2),
            category: DisplayCategory::Placement,
        };
        let target = pomelo_core::selection::canvas_target(
            &scene,
            anchor.object,
            pomelo_core::interaction::SelectionMode::Object,
            &cancel,
        )
        .unwrap();
        assert_eq!(target, SelectionTarget::Component(ObjectId(1)));
        let prepared = prepare_inspection(target, Some(anchor), &scene, &cancel).unwrap();
        assert!(prepared.labels.iter().any(
            |field| field.key == Key::SourceReference && field.args["name"].to_string() == "U1"
        ));
        assert!(
            !prepared
                .labels
                .iter()
                .any(|field| field.key == Key::SourcePinName)
        );
        assert_eq!(
            prepared.net, None,
            "the body is not assigned the net of an arbitrary pin"
        );
        assert!(
            prepared
                .related_objects
                .contains(&SelectedObject::Placement(ObjectId(1)))
        );
        assert_eq!(
            target.pin_ids(&scene, &cancel).unwrap(),
            [ObjectId(3)].into()
        );
    }

    #[test]
    fn missing_or_cancelled_sources_produce_no_labels() {
        let scene = pin_scene();
        let missing = SelectionTarget::Object(SelectedObject::Pin(ObjectId(99)));
        assert!(selection_source_labels(missing, &scene, &CancellationToken::default()).is_empty());
        let cancel = CancellationToken::default();
        cancel.cancel();
        let target = SelectionTarget::Object(SelectedObject::Pin(ObjectId(3)));
        assert!(selection_source_labels(target, &scene, &cancel).is_empty());
    }
    #[test]
    fn dimensions_round_only_after_unit_conversion() {
        use pomelo_core::{i18n::Locale, units::LengthUnit};
        let source = 0.0000004_f64;
        let message = Message::new(Key::SourceTraceWidth).arg("width", source.to_string());
        let mils = display_source_field(&message, Locale::English, LengthUnit::Mils);
        assert!(mils.contains("0.000016"));
        let millimeters = display_source_field(&message, Locale::English, LengthUnit::Millimeters);
        assert!(millimeters.contains("0.000000"));
        assert_eq!(
            message.args["width"].to_string().parse::<f64>().unwrap(),
            source
        );
    }
    #[test]
    fn retained_dimensions_switch_units_without_mutating_source_messages() {
        use pomelo_core::{i18n::Locale, units::LengthUnit};
        let message = Message::new(Key::SourceTraceWidth).arg("width", "0.025400");
        let original = message.clone();
        for locale in Locale::ALL {
            let mils = display_source_field(&message, locale, LengthUnit::Mils);
            assert!(mils.contains("1.000000"));
            assert!(mils.contains("mil"));
            assert_eq!(
                display_source_field(&message, locale, LengthUnit::Millimeters),
                original.display(locale)
            );
        }
        assert_eq!(message, original);
    }
}
