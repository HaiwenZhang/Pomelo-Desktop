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
                .w(relative(0.46))
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
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
    pub bonds: std::sync::Arc<BTreeSet<pomelo_core::selection::SelectedObject>>,
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
    if cancel.is_cancelled() {
        return Err(pomelo_core::geometry::PathError::Cancelled);
    }
    let mut labels = selection_source_labels(target, scene, cancel);
    if let Some(anchor) = anchor {
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
        bonds: std::sync::Arc::new(
            if matches!(
                target,
                pomelo_core::selection::SelectionTarget::Component(_)
                    | pomelo_core::selection::SelectionTarget::ComponentGroup(_)
            ) {
                target
                    .related_bond_objects(scene, cancel)?
                    .into_iter()
                    .collect()
            } else {
                BTreeSet::new()
            },
        ),
        labels,
        net: related_net(target, scene, cancel),
        component: related_component(target, scene, cancel)?,
    };
    if cancel.is_cancelled() {
        Err(pomelo_core::geometry::PathError::Cancelled)
    } else {
        Ok(prepared)
    }
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
        SelectionTarget::Component(id) => (Key::ModeComponent, id.0),
        SelectionTarget::ComponentGroup(anchor) => (Key::ModeComponent, anchor.id().0),
    }
}

pub(crate) fn selection_source_labels(
    target: pomelo_core::selection::SelectionTarget,
    scene: &BoardScene,
    cancel: &pomelo_core::task::CancellationToken,
) -> Vec<Message> {
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    let mut labels = Vec::new();
    let mut add = |key, value: &str| {
        if !value.is_empty() {
            labels.push(Message::new(key).arg("name", value));
        }
    };
    match target {
        SelectionTarget::ComponentGroup(anchor) => {
            if let Ok(Some(reference)) = anchor.reference(scene, cancel) {
                add(Key::SourceReference, reference);
            }
        }
        SelectionTarget::Net(id) => {
            if let Some(name) = scene.nets.get(&id) {
                add(Key::SourceNetName, name);
            }
        }
        SelectionTarget::Component(id) => {
            if let Some(component) = scene
                .components
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|component| component.id == id)
            {
                add(Key::SourceReference, &component.reference);
            }
        }
        SelectionTarget::Object(SelectedObject::Pin(id)) => {
            if let Some(pin) = scene
                .pins
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|pin| pin.id == id)
            {
                add(Key::SourceReference, &pin.reference);
                add(Key::SourcePinName, &pin.name);
                if let Some(name) = scene.nets.get(&pin.net) {
                    add(Key::SourceNetName, name);
                }
            }
        }
        SelectionTarget::Object(SelectedObject::Via(id)) => {
            if let Some(via) = scene
                .vias
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|via| via.id == id)
            {
                add(Key::SourcePadstackName, &via.padstack_name);
                if let Some(name) = scene.nets.get(&via.net) {
                    add(Key::SourceNetName, name);
                }
            }
        }
        SelectionTarget::Object(SelectedObject::Segment(id)) => {
            if let Some(segment) = scene
                .segments
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|segment| segment.id == id)
                && let Some(name) = scene.nets.get(&segment.net)
            {
                add(Key::SourceNetName, name);
            }
        }
        SelectionTarget::Object(SelectedObject::Zone(id)) => {
            if let Some(zone) = scene
                .zones
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|zone| zone.id == id)
                && let Some(name) = scene.nets.get(&zone.net)
            {
                add(Key::SourceNetName, name);
            }
        }
        SelectionTarget::Object(SelectedObject::Drawing(id)) => {
            if let Some(drawing) = scene
                .drawings
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|drawing| drawing.id == id)
            {
                for source in scene
                    .texts
                    .iter()
                    .take_while(|_| !cancel.is_cancelled())
                    .filter(|text| drawing.text_ids.contains(&text.id))
                    .take(16)
                {
                    add(Key::SourceText, &source.text);
                }
            }
        }
        SelectionTarget::Track(_) => {}
    }
    let mut coordinate = |key, point: pomelo_core::model::Point| {
        if point.x.is_finite() && point.y.is_finite() {
            labels.push(
                Message::new(key)
                    .arg("x", point.x.to_string())
                    .arg("y", point.y.to_string()),
            );
        }
    };
    match target {
        SelectionTarget::Component(id) => {
            if let Some(value) = scene
                .components
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|value| value.id == id)
            {
                coordinate(Key::SourcePosition, value.at);
            }
        }
        SelectionTarget::Object(SelectedObject::Pin(id)) => {
            if let Some(value) = scene
                .pins
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|value| value.id == id)
            {
                coordinate(Key::SourcePosition, value.at);
            }
        }
        SelectionTarget::Object(SelectedObject::Via(id)) => {
            if let Some(value) = scene
                .vias
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|value| value.id == id)
            {
                coordinate(Key::SourcePosition, value.at);
            }
        }
        SelectionTarget::Object(SelectedObject::Segment(id)) => {
            if let Some(value) = scene
                .segments
                .iter()
                .take_while(|_| !cancel.is_cancelled())
                .find(|value| value.id == id)
            {
                coordinate(Key::SourceStart, value.a);
                coordinate(Key::SourceEnd, value.b);
            }
        }
        _ => {}
    }
    let drill = match target {
        SelectionTarget::Object(SelectedObject::Pin(id)) => scene
            .pins
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.drill_shape),
        SelectionTarget::Object(SelectedObject::Via(id)) => scene
            .vias
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.drill_shape),
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
    if let SelectionTarget::Object(SelectedObject::Segment(id)) = target
        && let Some(segment) = scene
            .segments
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
        && segment.width.is_finite()
        && segment.width >= 0.0
    {
        labels.push(Message::new(Key::SourceTraceWidth).arg("width", segment.width.to_string()));
    }
    let placement = match target {
        SelectionTarget::Component(id) => scene
            .components
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| (value.angle, value.mirrored)),
        SelectionTarget::Object(SelectedObject::Pin(id)) => scene
            .pins
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| (value.angle, value.mirrored)),
        SelectionTarget::Object(SelectedObject::Via(id)) => scene
            .vias
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| (value.angle, value.mirrored)),
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
    let layers: BTreeSet<_> = match target {
        SelectionTarget::Object(SelectedObject::Pin(id)) => scene
            .pins
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .into_iter()
            .flat_map(|value| {
                value
                    .pads
                    .iter()
                    .take_while(|_| !cancel.is_cancelled())
                    .map(|pad| pad.layer)
            })
            .collect(),
        SelectionTarget::Object(SelectedObject::Via(id)) => scene
            .vias
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .into_iter()
            .flat_map(|value| {
                value
                    .pads
                    .iter()
                    .take_while(|_| !cancel.is_cancelled())
                    .map(|pad| pad.layer)
            })
            .collect(),
        SelectionTarget::Object(SelectedObject::Segment(id)) => scene
            .segments
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.layer)
            .into_iter()
            .collect(),
        SelectionTarget::Object(SelectedObject::Zone(id)) => scene
            .zones
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|value| value.id == id)
            .map(|value| value.layer)
            .into_iter()
            .collect(),
        SelectionTarget::Object(SelectedObject::Drawing(id)) => scene
            .drawings
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|d| d.id == id)
            .map(|d| d.layer)
            .into_iter()
            .collect(),
        _ => BTreeSet::new(),
    };
    for layer in layers {
        if cancel.is_cancelled() {
            return labels;
        }
        let name = scene
            .layers
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .map(|definition| (definition.id, definition.name.as_str()))
            .find(|(id, _)| *id == layer)
            .map(|(_, name)| name)
            .filter(|name| !name.is_empty());
        labels.push(if let Some(name) = name {
            Message::new(Key::SourceLayerName)
                .arg("id", layer.0)
                .arg("name", name)
        } else {
            Message::new(Key::SourceLayerId).arg("id", layer.0)
        });
    }
    if let SelectionTarget::Object(SelectedObject::Via(id)) = target
        && let Some(backdrill) = scene
            .vias
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|via| via.id == id)
            .and_then(|via| via.backdrill.as_ref())
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
    if let SelectionTarget::Object(SelectedObject::Segment(id)) = target
        && let Some(wire) = scene
            .segments
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|segment| segment.id == id)
            .and_then(|segment| segment.bond_wire.as_ref())
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
            if !name.is_empty() {
                labels.push(Message::new(key).arg("name", name));
            }
        }
    }
    if let SelectionTarget::Object(SelectedObject::Via(id)) = target
        && let Some(finger) = scene
            .vias
            .iter()
            .take_while(|_| !cancel.is_cancelled())
            .find(|via| via.id == id)
            .and_then(|via| via.finger.as_ref())
    {
        if let Some(pin) = finger.source_pin {
            labels.push(Message::new(Key::SourceBondPin).arg("id", pin.0));
        }
        for (key, name) in [
            (Key::SourceReference, finger.reference.as_str()),
            (Key::SourceFingerName, finger.name.as_str()),
        ] {
            if !name.is_empty() {
                labels.push(Message::new(key).arg("name", name));
            }
        }
    }
    labels
}

#[cfg(test)]
mod unit_tests {
    use super::*;
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
