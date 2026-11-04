//! Persistent, named highlight groups. They are stored separately from the current
//! selection; clearing the selection never deletes a group. Colors are display
//! overrides and never change source layers or object properties.
use crate::{
    appearance::RgbColor, geometry::PathError, model::BoardScene, selection::SelectedObject,
    task::CancellationToken,
};
use std::collections::BTreeMap;

pub const MAX_HIGHLIGHT_GROUPS: usize = 64;
/// Members saved across all groups of one document. Groups are stored with the
/// document view, so this bounds the configuration file, not only memory.
pub const MAX_DOCUMENT_MEMBERS: usize = 200_000;
pub const MAX_GROUP_MEMBERS: usize = MAX_DOCUMENT_MEMBERS;
pub const MAX_NAME_CHARS: usize = 80;

/// Distinct defaults for new groups, chosen to stay legible on dark and light canvases.
pub const PALETTE: [RgbColor; 8] = [
    RgbColor([255, 99, 71]),
    RgbColor([255, 196, 0]),
    RgbColor([0, 200, 255]),
    RgbColor([190, 110, 255]),
    RgbColor([80, 220, 100]),
    RgbColor([255, 120, 200]),
    RgbColor([255, 150, 40]),
    RgbColor([120, 160, 255]),
];

fn visible_by_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HighlightGroup {
    pub id: u32,
    pub name: String,
    pub color: RgbColor,
    #[serde(default = "visible_by_default")]
    pub visible: bool,
    /// Sorted and deduplicated source identities, saved in a compact encoding.
    #[serde(with = "compact_members")]
    pub members: Vec<SelectedObject>,
}

/// Saves members as one delta-encoded decimal list per object type, e.g.
/// `{"pins": "405752,11,92"}`. Large groups then cost a few bytes per member and
/// are unaffected by pretty-printed configuration files.
mod compact_members {
    use crate::{model::ObjectId, selection::SelectedObject};
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

    type MakeObject = fn(ObjectId) -> SelectedObject;

    #[derive(Default, Serialize, Deserialize)]
    #[serde(default, deny_unknown_fields)]
    struct Encoded {
        #[serde(skip_serializing_if = "String::is_empty")]
        segments: String,
        #[serde(skip_serializing_if = "String::is_empty")]
        pins: String,
        #[serde(skip_serializing_if = "String::is_empty")]
        vias: String,
        #[serde(skip_serializing_if = "String::is_empty")]
        zones: String,
        #[serde(skip_serializing_if = "String::is_empty")]
        drawings: String,
        #[serde(skip_serializing_if = "String::is_empty")]
        placements: String,
    }

    fn id(object: SelectedObject) -> u32 {
        match object {
            SelectedObject::Segment(id)
            | SelectedObject::Pin(id)
            | SelectedObject::Via(id)
            | SelectedObject::Zone(id)
            | SelectedObject::Drawing(id)
            | SelectedObject::Placement(id) => id.0,
        }
    }

    pub fn serialize<S: Serializer>(
        members: &[SelectedObject],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut encoded = Encoded::default();
        let mut previous: Option<(std::mem::Discriminant<SelectedObject>, u32)> = None;
        for &object in members {
            let kind = std::mem::discriminant(&object);
            let list = match object {
                SelectedObject::Segment(_) => &mut encoded.segments,
                SelectedObject::Pin(_) => &mut encoded.pins,
                SelectedObject::Via(_) => &mut encoded.vias,
                SelectedObject::Zone(_) => &mut encoded.zones,
                SelectedObject::Drawing(_) => &mut encoded.drawings,
                SelectedObject::Placement(_) => &mut encoded.placements,
            };
            let value = match previous {
                Some((previous_kind, previous_id)) if previous_kind == kind => {
                    list.push(',');
                    id(object).wrapping_sub(previous_id)
                }
                _ => id(object),
            };
            list.push_str(&value.to_string());
            previous = Some((kind, id(object)));
        }
        encoded.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<SelectedObject>, D::Error> {
        let encoded = Encoded::deserialize(deserializer)?;
        let mut members = Vec::new();
        let lists: [(&str, MakeObject); 6] = [
            (&encoded.segments, SelectedObject::Segment),
            (&encoded.pins, SelectedObject::Pin),
            (&encoded.vias, SelectedObject::Via),
            (&encoded.zones, SelectedObject::Zone),
            (&encoded.drawings, SelectedObject::Drawing),
            (&encoded.placements, SelectedObject::Placement),
        ];
        for (list, make) in lists {
            let mut current: Option<u32> = None;
            for part in list.split(',').filter(|_| !list.is_empty()) {
                let value: u32 = part
                    .parse()
                    .map_err(|_| D::Error::custom("MEMBERS_INVALID"))?;
                let next = match current {
                    // Strictly increasing identities: a zero delta is a duplicate.
                    Some(_) if value == 0 => return Err(D::Error::custom("MEMBERS_INVALID")),
                    Some(previous) => previous
                        .checked_add(value)
                        .ok_or_else(|| D::Error::custom("MEMBERS_INVALID"))?,
                    None => value,
                };
                if members.len() >= super::MAX_GROUP_MEMBERS {
                    return Err(D::Error::custom("MEMBERS_LIMIT"));
                }
                members.push(make(ObjectId(next)));
                current = Some(next);
            }
        }
        Ok(members)
    }
}

impl HighlightGroup {
    pub fn new(
        id: u32,
        name: String,
        color: RgbColor,
        members: impl IntoIterator<Item = SelectedObject>,
    ) -> Self {
        let mut members: Vec<_> = members.into_iter().take(MAX_GROUP_MEMBERS).collect();
        members.sort_unstable();
        members.dedup();
        Self {
            id,
            name: normalized_name(&name),
            color,
            visible: true,
            members,
        }
    }

    pub fn rename(&mut self, name: &str) -> bool {
        let name = normalized_name(name);
        if name.is_empty() || name == self.name {
            return false;
        }
        self.name = name;
        true
    }

    pub fn valid(&self) -> bool {
        !self.name.is_empty()
            && self.name.chars().count() <= MAX_NAME_CHARS
            && self.members.len() <= MAX_GROUP_MEMBERS
            && self.members.windows(2).all(|pair| pair[0] < pair[1])
    }
}

/// Trim and bound a user-entered name; control characters become spaces.
pub fn normalized_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Sorted identities of every object in a scene, for checking saved references.
pub struct ObjectCatalog {
    segments: Vec<u32>,
    pins: Vec<u32>,
    vias: Vec<u32>,
    zones: Vec<u32>,
    drawings: Vec<u32>,
    placements: Vec<u32>,
}

impl ObjectCatalog {
    pub fn build(scene: &BoardScene, cancel: &CancellationToken) -> Result<Self, PathError> {
        fn sorted(ids: impl Iterator<Item = u32>) -> Vec<u32> {
            let mut ids: Vec<_> = ids.collect();
            ids.sort_unstable();
            ids
        }
        cancel.check()?;
        let catalog = Self {
            segments: sorted(scene.segments.iter().map(|segment| segment.id.0)),
            pins: sorted(scene.pins.iter().map(|pin| pin.id.0)),
            vias: sorted(scene.vias.iter().map(|via| via.id.0)),
            zones: sorted(scene.zones.iter().map(|zone| zone.id.0)),
            drawings: sorted(scene.drawings.iter().map(|drawing| drawing.id.0)),
            placements: sorted(
                scene
                    .components
                    .iter()
                    .filter(|component| component.body.is_some())
                    .map(|component| component.id.0),
            ),
        };
        cancel.check()?;
        Ok(catalog)
    }

    pub fn contains(&self, object: SelectedObject) -> bool {
        let (ids, id) = match object {
            SelectedObject::Segment(id) => (&self.segments, id),
            SelectedObject::Pin(id) => (&self.pins, id),
            SelectedObject::Via(id) => (&self.vias, id),
            SelectedObject::Zone(id) => (&self.zones, id),
            SelectedObject::Drawing(id) => (&self.drawings, id),
            SelectedObject::Placement(id) => (&self.placements, id),
        };
        ids.binary_search(&id.0).is_ok()
    }

    /// Members that no longer exist in the scene. They are reported, never rebound.
    pub fn missing_members(&self, group: &HighlightGroup) -> usize {
        group
            .members
            .iter()
            .filter(|object| !self.contains(**object))
            .count()
    }
}

/// Per-object display colors of the visible groups. When groups overlap, the group
/// listed first wins, so list order is the display priority.
pub fn object_colors(groups: &[HighlightGroup]) -> BTreeMap<SelectedObject, RgbColor> {
    let mut colors = BTreeMap::new();
    for group in groups.iter().filter(|group| group.visible) {
        for &object in &group.members {
            colors.entry(object).or_insert(group.color);
        }
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LayerId, NetId, ObjectId, Point, Segment};

    #[test]
    fn members_are_sorted_and_deduplicated() {
        let pin = SelectedObject::Pin(ObjectId(2));
        let segment = SelectedObject::Segment(ObjectId(9));
        let group = HighlightGroup::new(1, "  Power  ".into(), PALETTE[0], [pin, segment, pin]);
        assert_eq!(group.members, vec![segment, pin]);
        assert_eq!(group.name, "Power");
        assert!(group.valid());
    }

    #[test]
    fn names_are_trimmed_bounded_and_never_empty_after_rename() {
        let mut group = HighlightGroup::new(1, "A".into(), PALETTE[0], []);
        assert!(!group.rename("   "));
        assert_eq!(group.name, "A");
        assert!(group.rename(&"x".repeat(200)));
        assert_eq!(group.name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(normalized_name("a\nb\t"), "a b");
    }

    #[test]
    fn first_visible_group_wins_overlapping_members() {
        let shared = SelectedObject::Via(ObjectId(1));
        let mut first = HighlightGroup::new(1, "a".into(), PALETTE[0], [shared]);
        let second = HighlightGroup::new(
            2,
            "b".into(),
            PALETTE[1],
            [shared, SelectedObject::Pin(ObjectId(3))],
        );
        let colors = object_colors(&[first.clone(), second.clone()]);
        assert_eq!(colors[&shared], PALETTE[0]);
        assert_eq!(colors.len(), 2);
        first.visible = false;
        assert_eq!(object_colors(&[first, second])[&shared], PALETTE[1]);
    }

    #[test]
    fn missing_references_are_counted_not_rebound() {
        let scene = BoardScene {
            segments: vec![Segment {
                id: ObjectId(5),
                track_id: ObjectId(5),
                layer: LayerId(0),
                net: NetId(0),
                a: Point::default(),
                b: Point::new(1.0, 0.0),
                width: 0.1,
                arc: None,
                bond_wire: None,
            }],
            ..crate::review::tests::empty_scene()
        };
        let catalog = ObjectCatalog::build(&scene, &CancellationToken::default()).unwrap();
        let group = HighlightGroup::new(
            1,
            "g".into(),
            PALETTE[0],
            [
                SelectedObject::Segment(ObjectId(5)),
                SelectedObject::Segment(ObjectId(6)),
                SelectedObject::Pin(ObjectId(5)),
            ],
        );
        assert_eq!(catalog.missing_members(&group), 2);
    }

    #[test]
    fn members_roundtrip_in_a_compact_delta_encoding() {
        let members = [
            SelectedObject::Segment(ObjectId(400_834)),
            SelectedObject::Segment(ObjectId(400_845)),
            SelectedObject::Pin(ObjectId(7)),
            SelectedObject::Pin(ObjectId(u32::MAX)),
            SelectedObject::Drawing(ObjectId(0)),
        ];
        let group = HighlightGroup::new(1, "g".into(), PALETTE[0], members);
        let value = serde_json::to_value(&group).unwrap();
        assert_eq!(
            value["members"],
            serde_json::json!({"segments": "400834,11", "pins": "7,4294967288", "drawings": "0"})
        );
        let decoded: HighlightGroup = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, group);
        let large = HighlightGroup::new(
            2,
            "large".into(),
            PALETTE[1],
            (0..62_402).map(|id| SelectedObject::Via(ObjectId(400_000 + id * 3))),
        );
        // About two bytes per member, instead of an object per member.
        assert!(serde_json::to_vec_pretty(&large).unwrap().len() < 200_000);
    }

    #[test]
    fn malformed_member_lists_are_rejected() {
        for members in [
            serde_json::json!({"pins": "4,0"}),
            serde_json::json!({"pins": "4,x"}),
            serde_json::json!({"pins": "4294967295,1"}),
            serde_json::json!({"wires": "1"}),
        ] {
            let value =
                serde_json::json!({"id": 1, "name": "n", "color": [1, 2, 3], "members": members});
            assert!(serde_json::from_value::<HighlightGroup>(value).is_err());
        }
    }

    #[test]
    fn saved_groups_without_visibility_default_to_visible() {
        let value = serde_json::json!({
            "id": 3, "name": "n", "color": [1, 2, 3], "members": {"pins": "4"}
        });
        let group: HighlightGroup = serde_json::from_value(value).unwrap();
        assert!(group.visible);
        assert_eq!(group.members, vec![SelectedObject::Pin(ObjectId(4))]);
    }
}
