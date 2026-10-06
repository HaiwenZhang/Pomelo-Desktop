use pomelo_core::task::CancellationToken;
use pomelo_import::{
    ImportContext, ImportError, ImportOptions,
    formats::allegro::{
        database::{BrdDatabase, ChainLimits, ChainRequest, LocatedRecord, ReferenceLocation},
        decoder::{DecodeLimits, DecodedRecord, fixed::FixedRecord},
        index::{FileOffset, IndexLimits, RecordKey},
    },
};

const START: usize = 0x1200 + 16;
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn bytes(links: &[(u32, u32)]) -> Vec<u8> {
    let mut bytes = vec![0; START + 24 * links.len() + 4];
    put(&mut bytes, 0, 0x140900);
    put(&mut bytes, 0x26c, 1000);
    put(&mut bytes, 0x28c, 1000);
    put(&mut bytes, 0x194, 1);
    bytes[0x1204..0x120d].copy_from_slice("日本語".as_bytes());
    for (index, &(key, next)) in links.iter().enumerate() {
        let offset = START + index * 24;
        bytes[offset] = 4;
        put(&mut bytes, offset + 4, key);
        put(&mut bytes, offset + 8, next);
    }
    bytes
}
fn read(bytes: Vec<u8>) -> BrdDatabase {
    let cancellation = CancellationToken::default();
    BrdDatabase::read(
        bytes,
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits::default(),
        &ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        },
    )
    .unwrap()
}
fn next(record: &LocatedRecord) -> Result<RecordKey, ImportError> {
    let DecodedRecord::Fixed(FixedRecord::NetAssignment(record)) = &record.fields else {
        panic!()
    };
    Ok(RecordKey(record.next))
}
fn request(start: u32, terminator: u32) -> ChainRequest<'static> {
    ChainRequest {
        start: RecordKey(start),
        terminator: RecordKey(terminator),
        expected_types: &[4],
        origin: ReferenceLocation {
            offset: FileOffset(0x100),
            field: "Head",
        },
        link_field: "Next",
        limits: ChainLimits::default(),
    }
}

#[test]
fn database_owns_one_source_and_keeps_zero_string_keys_separate_from_record_identity() {
    let bytes = bytes(&[(u32::MAX, 0)]);
    let pointer = bytes.as_ptr();
    let database = read(bytes);
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    assert_eq!(database.source_bytes().as_ptr(), pointer);
    assert_eq!(database.string(0), Some("日本語"));
    assert!(database.get(RecordKey(0), &context).unwrap().is_none());
    let record = database
        .get(RecordKey(u32::MAX), &context)
        .unwrap()
        .unwrap();
    assert_eq!(
        (record.span.key, record.span.offset),
        (RecordKey(u32::MAX), FileOffset(START as u32))
    );
}

#[test]
fn independent_queries_and_source_order_iterators_do_not_share_mutable_record_fields() {
    let database = read(bytes(&[(7, 9), (9, 0)]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let mut record = database.get(RecordKey(7), &context).unwrap().unwrap();
    let DecodedRecord::Fixed(FixedRecord::NetAssignment(fields)) = &mut record.fields else {
        panic!()
    };
    fields.next = 0;
    let keys: Vec<_> = database
        .records_of_type(4, &context)
        .map(|r| r.unwrap().span.key)
        .collect();
    assert_eq!(keys, [RecordKey(7), RecordKey(9)]);
    assert_eq!(
        next(&database.get(RecordKey(7), &context).unwrap().unwrap()).unwrap(),
        RecordKey(9)
    );
}

#[test]
fn wrong_reference_type_is_rejected_before_decoding_the_target() {
    let database = read(bytes(&[(7, 0)]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let error = database
        .require_record(RecordKey(7), &[0x1c], request(7, 0).origin, &context)
        .unwrap_err();
    assert!(matches!(
        error,
        ImportError::ReferenceType {
            key: 7,
            actual: 4,
            offset: 0x100,
            field: "Head",
            ..
        }
    ));
}

#[test]
fn chain_stops_at_its_explicit_sentinel_without_looking_up_the_sentinel() {
    let database = read(bytes(&[(u32::MAX, 7), (7, 99)]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let mut keys = Vec::new();
    let count = database
        .walk_chain(&request(u32::MAX, 99), &context, next, |r| {
            keys.push(r.span.key);
            Ok(())
        })
        .unwrap();
    assert_eq!((count, keys), (2, vec![RecordKey(u32::MAX), RecordKey(7)]));
}

#[test]
fn missing_chain_link_reports_the_referring_record_and_field() {
    let database = read(bytes(&[(7, 9)]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let error = database
        .walk_chain(&request(7, 0), &context, next, |_| Ok(()))
        .unwrap_err();
    assert!(matches!(
        error,
        ImportError::MissingReference {
            key: 9,
            offset: START,
            field: "Next"
        }
    ));
}

#[test]
fn self_links_and_multi_record_cycles_fail_after_each_member_is_visited_once() {
    for links in [&[(1, 1)][..], &[(1, 2), (2, 1)][..]] {
        let database = read(bytes(links));
        let cancellation = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        };
        let mut seen = Vec::new();
        let error = database
            .walk_chain(&request(1, 0), &context, next, |record| {
                seen.push(record.span.key);
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(
            error,
            ImportError::ReferenceCycle {
                key: 1,
                field: "Next",
                ..
            }
        ));
        assert_eq!(seen.len(), links.len());
    }
}

#[test]
fn visit_budget_fails_before_decoding_the_next_member() {
    let database = read(bytes(&[(1, 2), (2, 0)]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let mut request = request(1, 0);
    request.limits.max_visit_bytes = 64;
    let mut visited = 0;
    let error = database
        .walk_chain(&request, &context, next, |_| {
            visited += 1;
            Ok(())
        })
        .unwrap_err();
    assert!(matches!(
        error,
        ImportError::DecodeLimit {
            offset: START,
            actual: 128,
            limit: 64
        }
    ));
    assert_eq!(visited, 1);
}

#[test]
fn record_count_budget_is_independent_of_visit_allocation() {
    let database = read(bytes(&[(1, 2), (2, 0)]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let mut request = request(1, 0);
    request.limits.max_records = 1;
    assert!(matches!(
        database.walk_chain(&request, &context, next, |_| Ok(())),
        Err(ImportError::InvalidRecord {
            field: "CHAIN_RECORD_COUNT",
            value: 2,
            ..
        })
    ));
}

#[test]
fn cancellation_after_a_visitor_stops_before_the_next_member() {
    let database = read(bytes(&[(1, 2), (2, 0)]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let mut visited = 0;
    assert!(matches!(
        database.walk_chain(&request(1, 0), &context, next, |_| {
            visited += 1;
            cancellation.cancel();
            Ok(())
        }),
        Err(ImportError::Cancelled)
    ));
    assert_eq!(visited, 1);
}

#[test]
fn a_zero_head_and_a_head_equal_to_the_sentinel_are_empty_chains() {
    let database = read(bytes(&[]));
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    for request in [request(0, 1), request(1, 1)] {
        assert_eq!(
            database
                .walk_chain(&request, &context, |_| panic!(), |_| panic!())
                .unwrap(),
            0
        );
    }
}
