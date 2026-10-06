//! Explicit local evidence for the newly supported V165/V166 paired-net records.
use pomelo_core::task::CancellationToken;
use pomelo_import::{
    ImportContext, ImportOptions,
    formats::allegro::{
        database::BrdDatabase,
        decoder::{DecodeLimits, DecodedRecord, variable::VariableRecord},
        index::{IndexLimits, RecordKey},
    },
};
use serde_json::json;
use std::{io::Write, path::PathBuf};

#[test]
#[ignore = "requires explicit POMELO_PAIRED_CASES_DIR and POMELO_PAIRED_REPORT paths"]
fn legacy_real_paired_nets_preserve_every_field_and_resolve_member_nets() {
    let cases =
        PathBuf::from(std::env::var_os("POMELO_PAIRED_CASES_DIR").expect("explicit cases path"));
    let report =
        PathBuf::from(std::env::var_os("POMELO_PAIRED_REPORT").expect("explicit report path"));
    let mut output = std::io::BufWriter::new(std::fs::File::create(report).unwrap());
    for name in [
        "PC4-RDIMM_V200_RC_J1_20150212.brd",
        "PC4_SODIMM_V090_RC_A0_20131025.brd",
        "PC4_SODIMM_V095_RC_D0_20131025.brd",
    ] {
        let path = cases.join(name);
        let cancellation = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        };
        let database = BrdDatabase::read_path(
            &path,
            &ImportOptions::default(),
            &IndexLimits::default(),
            DecodeLimits::default(),
            &context,
        )
        .unwrap();
        let mut count = 0;
        for span in database.index().records_of_type(0x1a) {
            assert_eq!(span.byte_length, 88);
            let start = span.offset.0 as usize;
            let source = &database.source_bytes()[start..start + 88];
            let word = |offset| u32::from_le_bytes(source[offset..offset + 4].try_into().unwrap());
            let record = database
                .get_at_offset(span.offset, &context)
                .unwrap()
                .unwrap();
            let DecodedRecord::Variable(VariableRecord::PairedNets(pair)) = record.fields else {
                panic!("expected paired nets")
            };
            assert_eq!(pair.r#type, u16::from(source[1]));
            assert_eq!(
                pair.t2,
                u16::from_le_bytes(source[2..4].try_into().unwrap())
            );
            assert_eq!(pair.key.0, word(4));
            assert!(pair.unknown.is_none());
            for (i, member) in pair.members.iter().enumerate() {
                let offset = 8 + i * 40;
                assert_eq!(member.net, word(offset));
                assert_eq!(member.next, word(offset + 4));
                assert_eq!(
                    member.metadata,
                    (0..8).map(|j| word(offset + 8 + j * 4)).collect::<Vec<_>>()
                );
                assert_eq!(
                    database
                        .index()
                        .record(RecordKey(member.net))
                        .unwrap()
                        .record_type,
                    0x1b
                );
            }
            count += 1;
        }
        assert!(count > 0);
        serde_json::to_writer(&mut output, &json!({ "path": path, "version": database.header().version, "encoding": database.encoding().tag(), "paired_records": count, "fields_match_source": true, "member_nets_resolved": true })).unwrap();
        writeln!(output).unwrap();
        output.flush().unwrap();
    }
}
