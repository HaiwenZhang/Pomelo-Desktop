use pomelo_core::{
    copper::CopperMesh,
    model::{LayerId, NetId, ObjectId, Zone, ZoneKind},
};

#[test]
fn old_zone_without_identity_deserializes_to_unknown() {
    let zone = Zone {
        id: ObjectId(1),
        layer: LayerId(0),
        net: NetId(2),
        kind: ZoneKind::Static,
        paths: Vec::new(),
        mesh: CopperMesh::default(),
    };
    let mut legacy = serde_json::to_value(zone).unwrap();
    legacy.as_object_mut().unwrap().remove("kind");
    assert_eq!(
        serde_json::from_value::<Zone>(legacy).unwrap().kind,
        ZoneKind::Unknown
    );
}

#[test]
fn shape_identity_serializes_with_stable_lowercase_tags() {
    for (kind, tag) in [
        (ZoneKind::Unknown, "unknown"),
        (ZoneKind::Static, "static"),
        (ZoneKind::Dynamic, "dynamic"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), tag);
    }
}
