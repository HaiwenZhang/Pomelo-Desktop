use super::*;
use pomelo_core::model::ZoneKind;

fn static_records(state: u32) -> Vec<Vec<u8>> {
    let mut fill = shape(10, 6, 100, 0, false);
    put(&mut fill, 20, state);
    let mut records = vec![assignment(900, 0, 10), fill];
    records.extend(square(100, 10, 0.0, 0.0, 10000.0));
    records
}

fn boundary_table() -> Vec<u8> {
    let mut table = record(0x2c, 400, 44);
    table[2..4].copy_from_slice(&12u16.to_le_bytes());
    put(&mut table, 8, 30);
    put(&mut table, 12, 30);
    put(&mut table, 28, 500);
    table
}

fn pointer_array(key: u32, next: u32, member: u32) -> Vec<u8> {
    let mut array = record(0x37, key, 432);
    put(&mut array, 8, 400);
    put(&mut array, 12, next);
    put(&mut array, 16, 100);
    put(&mut array, 20, 1);
    put(&mut array, 32, member);
    array
}

fn dynamic_records(wrapped: bool, chunks: bool) -> Vec<Vec<u8>> {
    // Deliberately use raw1: association, rather than Unknown2 bit8, proves Dynamic.
    let mut records = static_records(1);
    let mut boundary = shape(30, 21, 0, 0, false);
    put(&mut boundary, 52, if wrapped { 700 } else { 400 });
    records.push(boundary);
    records.push(boundary_table());
    records.push(pointer_array(
        500,
        if chunks { 501 } else { 0 },
        if chunks { 0 } else { 10 },
    ));
    if chunks {
        let mut later = pointer_array(501, 400, 10);
        put(&mut later, 8, 500);
        records.push(later);
    }
    if wrapped {
        let mut wrapper = record(0x26, 700, 28);
        put(&mut wrapper, 8, 400);
        put(&mut wrapper, 12, 30);
        records.push(wrapper);
    }
    records
}

#[test]
fn unrelated_later_array_predecessor_prevents_static_inference() {
    let mut records = dynamic_records(false, true);
    let later = records
        .iter_mut()
        .find(|r| r[0] == 0x37 && r[4..8] == 501u32.to_le_bytes())
        .unwrap();
    put(later, 8, 999);
    assert_eq!(classified(&records).unwrap(), ZoneKind::Unknown);
}

fn classified(records: &[Vec<u8>]) -> Result<ZoneKind, ImportError> {
    let db = database(records);
    let nets = networks(&db);
    CopperDecoder::new(&db, &nets, 2, CopperLimits::default())?
        .shape(RecordKey(10), &context(&CancellationToken::default()))
        .map(|object| object.zone.unwrap().kind)
}

#[test]
fn standalone_raw1_is_static_only_after_complete_boundary_scan() {
    assert_eq!(classified(&static_records(1)).unwrap(), ZoneKind::Static);
}

#[test]
fn auto_generated_and_unproved_source_states_remain_unknown() {
    for state in [0, 257, 4097, 12289] {
        assert_eq!(
            classified(&static_records(state)).unwrap(),
            ZoneKind::Unknown
        );
    }
}

#[test]
fn boundary_association_proves_dynamic_without_unknown_bit8() {
    assert_eq!(
        classified(&dynamic_records(false, false)).unwrap(),
        ZoneKind::Dynamic
    );
}

#[test]
fn wrapped_boundary_table_and_later_array_chunk_prove_dynamic() {
    assert_eq!(
        classified(&dynamic_records(true, true)).unwrap(),
        ZoneKind::Dynamic
    );
}

#[test]
fn unsupported_association_schema_prevents_static_inference() {
    let mut records = dynamic_records(false, false);
    put(records.iter_mut().find(|r| r[0] == 0x2c).unwrap(), 12, 999);
    assert_eq!(classified(&records).unwrap(), ZoneKind::Unknown);
}

#[test]
fn missing_association_reference_preserves_typed_error_and_five_languages() {
    let mut records = dynamic_records(false, false);
    put(records.iter_mut().find(|r| r[0] == 0x2c).unwrap(), 28, 999);
    let error = classified(&records).unwrap_err();
    assert!(matches!(
        error,
        ImportError::MissingReference { key: 999, .. }
    ));
    for locale in Locale::ALL {
        assert!(!error.diagnostic().message.display(locale).is_empty());
    }
}

#[test]
fn association_cycles_preserve_reference_cycle_error() {
    let mut records = dynamic_records(true, false);
    put(records.iter_mut().find(|r| r[0] == 0x26).unwrap(), 8, 700);
    assert!(matches!(
        classified(&records),
        Err(ImportError::ReferenceCycle { key: 700, .. })
    ));
}

#[test]
fn association_count_out_of_range_keeps_identity_unknown() {
    let mut records = dynamic_records(false, false);
    put(records.iter_mut().find(|r| r[0] == 0x37).unwrap(), 20, 101);
    assert_eq!(classified(&records).unwrap(), ZoneKind::Unknown);
}

#[test]
fn unproved_array_payload_does_not_treat_scalar_words_as_references() {
    let mut records = dynamic_records(false, false);
    let array = records.iter_mut().find(|r| r[0] == 0x37).unwrap();
    put(array, 24, 10);
    put(array, 32, 1); // Scalar in an unproved payload, not a missing geometry reference.
    assert_eq!(classified(&records).unwrap(), ZoneKind::Unknown);
}

#[test]
fn association_index_retention_is_charged_to_semantic_budget() {
    let db = database(&dynamic_records(false, false));
    let nets = networks(&db);
    let mut limits = CopperLimits::default();
    limits.objects.max_bytes = 63;
    assert!(matches!(
        CopperDecoder::new(&db, &nets, 2, limits)
            .unwrap()
            .shape(RecordKey(10), &context(&CancellationToken::default())),
        Err(ImportError::SemanticCacheLimit { .. })
    ));
}

#[test]
fn shape_identity_classification_does_not_publish_after_cancellation() {
    let db = database(&dynamic_records(false, false));
    let nets = networks(&db);
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        CopperDecoder::new(&db, &nets, 2, CopperLimits::default())
            .unwrap()
            .shape(RecordKey(10), &context(&token)),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn unresolved_physical_boundary_prevents_static_negative_inference() {
    let mut records = static_records(1);
    records.push(shape(30, 21, 0, 0, false));
    assert_eq!(classified(&records).unwrap(), ZoneKind::Unknown);
}

#[test]
fn nonphysical_boundary_does_not_change_static_copper_identity() {
    let mut records = static_records(1);
    let mut irrelevant = shape(30, 0xfd15, 0, 0, false);
    put(&mut irrelevant, 52, 999);
    records.push(irrelevant);
    assert_eq!(classified(&records).unwrap(), ZoneKind::Static);
}

#[test]
fn assigned_copper_rectangles_remain_unknown() {
    let db = database(&[assignment(900, 0, 10), rectangle(0x24, 10, 6)]);
    let nets = networks(&db);
    let zone = CopperDecoder::new(&db, &nets, 2, CopperLimits::default())
        .unwrap()
        .rectangle(RecordKey(10), &context(&CancellationToken::default()))
        .unwrap()
        .unwrap();
    assert_eq!(zone.kind, ZoneKind::Unknown);
}
