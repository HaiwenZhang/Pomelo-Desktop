use pomelo_core::{i18n::Locale, model::NetId, task::CancellationToken};
use pomelo_import::{
    ImportContext, ImportError, ImportOptions,
    formats::allegro::{
        database::BrdDatabase,
        decoder::{
            DecodeLimits, DecodedRecord,
            fixed::{FixedRecord, FunctionSlot, PlacedPad},
            variable::{Blob, VariableRecord},
        },
        index::{IndexLimits, RecordKey},
        semantics::connectivity::{NetworkLimits, NetworkMap},
    },
};

const START: usize = 0x1210;
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn source(links: &[(u32, u32, u32, u32)]) -> Vec<u8> {
    let mut bytes = vec![0; START + 60 + links.len() * 24 + 4];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    put(&mut bytes, 0x194, 1);
    bytes[0x1204..0x120d].copy_from_slice("日本語".as_bytes());
    bytes[START] = 0x1b;
    put(&mut bytes, START + 4, 99);
    for (i, &(key, next, net, item)) in links.iter().enumerate() {
        let at = START + 60 + i * 24;
        bytes[at] = 4;
        put(&mut bytes, at + 4, key);
        put(&mut bytes, at + 8, next);
        put(&mut bytes, at + 12, net);
        put(&mut bytes, at + 16, item);
    }
    bytes
}
fn database(bytes: Vec<u8>) -> BrdDatabase {
    BrdDatabase::read(
        bytes,
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits::default(),
        &ImportContext {
            cancellation: &CancellationToken::default(),
            progress: &|_| {},
        },
    )
    .unwrap()
}
fn build(bytes: Vec<u8>, limits: &NetworkLimits) -> Result<NetworkMap, ImportError> {
    NetworkMap::build(
        &database(bytes),
        limits,
        &ImportContext {
            cancellation: &CancellationToken::default(),
            progress: &|_| {},
        },
    )
}

#[test]
fn mixed_lists_stop_at_the_assignment_sentinel_and_preserve_source_names() {
    let map = build(
        source(&[(10, 0, 99, 20), (20, 30, 0, 0), (30, 10, 0, 0)]),
        &NetworkLimits::default(),
    )
    .unwrap();
    assert_eq!(map.nets[&NetId(99)], "日本語");
    assert_eq!(map.owner(RecordKey(20)), Some(NetId(99)));
    assert_eq!(map.owner(RecordKey(30)), Some(NetId(99)));
    assert_eq!(map.owner(RecordKey(10)), None);
    assert_eq!((map.assignment_count, map.link_visits), (3, 2));
}

#[test]
fn later_source_assignments_replace_earlier_owners_including_net_zero() {
    let map = build(
        source(&[(10, 0, 99, 30), (20, 0, 0, 30), (30, 0, 0, 0)]),
        &NetworkLimits::default(),
    )
    .unwrap();
    assert_eq!(map.owner(RecordKey(30)), Some(NetId(0)));
    assert_eq!(map.owner(RecordKey(31)), None);
    assert_eq!(
        (map.overwritten_owners, map.link_visits, map.owners().len()),
        (1, 2, 1)
    );
}

#[test]
fn chain_cycle_reports_the_link_that_closes_it_in_every_locale() {
    let error = build(
        source(&[(10, 0, 99, 20), (20, 30, 0, 0), (30, 20, 0, 0)]),
        &NetworkLimits::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, ImportError::ReferenceCycle { key: 20, offset, field: "Next" } if offset == START + 108)
    );
    let diagnostic = error.diagnostic();
    let messages: std::collections::BTreeSet<_> = Locale::ALL
        .iter()
        .map(|locale| diagnostic.message.display(*locale))
        .collect();
    assert_eq!(messages.len(), 5);
}

#[test]
fn missing_first_and_later_links_retain_the_origin_field_and_offset() {
    let first = build(source(&[(10, 0, 99, 500)]), &NetworkLimits::default()).unwrap_err();
    assert!(
        matches!(first, ImportError::MissingReference { key: 500, offset, field: "ConnItem" } if offset == START + 60)
    );
    let later = build(
        source(&[(10, 0, 99, 20), (20, 500, 0, 0)]),
        &NetworkLimits::default(),
    )
    .unwrap_err();
    assert!(
        matches!(later, ImportError::MissingReference { key: 500, offset, field: "Next" } if offset == START + 84)
    );
}

#[test]
fn absent_and_missing_names_are_preserved_as_empty_source_names() {
    let mut bytes = source(&[]);
    put(&mut bytes, START + 12, 500);
    let map = build(bytes, &NetworkLimits::default()).unwrap();
    assert_eq!(map.nets[&NetId(99)], "");
}

#[test]
fn map_byte_entry_chain_and_global_work_limits_fail_before_publishing_partial_results() {
    let bytes = source(&[(10, 0, 99, 20), (20, 30, 0, 0), (30, 0, 0, 0)]);
    let mut limits = NetworkLimits::default();
    limits.maps.max_bytes = 128 + "日本語".len() + 63;
    assert!(matches!(
        build(bytes.clone(), &limits),
        Err(ImportError::SemanticCacheLimit { .. })
    ));
    limits = NetworkLimits::default();
    limits.maps.max_entries = 1;
    assert!(matches!(
        build(bytes.clone(), &limits),
        Err(ImportError::InvalidRecord {
            field: "SEMANTIC_CACHE_ENTRY_COUNT",
            ..
        })
    ));
    limits = NetworkLimits::default();
    limits.chain.max_records = 1;
    assert!(matches!(
        build(bytes.clone(), &limits),
        Err(ImportError::InvalidRecord {
            field: "CHAIN_RECORD_COUNT",
            ..
        })
    ));
    limits = NetworkLimits::default();
    limits.chain.max_visit_bytes = 63;
    assert!(matches!(
        build(bytes.clone(), &limits),
        Err(ImportError::DecodeLimit { .. })
    ));
    limits = NetworkLimits::default();
    limits.max_link_visits = 1;
    assert!(matches!(
        build(bytes, &limits),
        Err(ImportError::InvalidRecord {
            field: "NETWORK_LINK_VISITS",
            ..
        })
    ));
}

#[test]
fn overlapping_lists_charge_only_unique_map_entries_but_each_visit_counts() {
    let mut limits = NetworkLimits::default();
    limits.maps.max_entries = 2;
    limits.maps.max_bytes = 128 + "日本語".len() + 64 + 512;
    let map = build(
        source(&[(10, 0, 99, 30), (20, 0, 0, 30), (30, 0, 0, 0)]),
        &limits,
    )
    .unwrap();
    assert_eq!((map.owners().len(), map.link_visits), (1, 2));
}

#[test]
fn cancellation_is_checked_inside_one_large_chain() {
    let mut links = vec![(1, 0, 99, 1000)];
    links.extend((1000..=1299).map(|key| (key, if key == 1299 { 0 } else { key + 1 }, 0, 0)));
    let db = database(source(&links));
    let token = CancellationToken::default();
    let notify = |progress: pomelo_core::task::ImportProgress| {
        if progress.completed >= 256 {
            token.cancel();
        }
    };
    let context = ImportContext {
        cancellation: &token,
        progress: &notify,
    };
    assert!(matches!(
        NetworkMap::build(&db, &NetworkLimits::default(), &context),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn source_next_never_uses_footprint_links_or_other_pointers() {
    let placed = DecodedRecord::Fixed(FixedRecord::PlacedPad(PlacedPad {
        next: 7,
        next_in_fp: 8,
        next_in_comp_inst: 9,
        ..Default::default()
    }));
    assert_eq!(placed.next_key(), Some(RecordKey(7)));
    let optional = FixedRecord::FunctionSlot(FunctionSlot {
        next: None,
        ..Default::default()
    });
    assert_eq!(optional.next_key(), None);
    let blob = VariableRecord::Blob(Blob {
        key: RecordKey(2),
        size: 3,
    });
    assert_eq!(blob.next_key(), None);
}
