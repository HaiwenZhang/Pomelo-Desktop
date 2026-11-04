use super::*;
use pomelo_core::task::CancellationToken;

#[test]
fn board_spans_handle_bom_whitespace_unicode_and_quoted_parentheses() {
    let cancel = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancel,
        progress: &|_| {},
    };
    let text = "\u{feff} ( kicad_pcb (segment (start 1 2)) (ignored (child \"(中文) \\\"quote\\\"\")) (version 20260206) (generator \"C:\\模型.step\")) ";
    let board = document(text.as_bytes(), &context).unwrap();
    assert_eq!(board.count("segment"), 1);
    assert_eq!(
        board.required("version").unwrap().number(1).unwrap(),
        20260206.0
    );
    assert_eq!(
        board.required("generator").unwrap().atom(1).unwrap(),
        "C:\\模型.step"
    );
    let segment = board.children("segment").next().unwrap().unwrap();
    assert_eq!(segment.required("start").unwrap().number(2).unwrap(), 2.0);
}

#[test]
fn skipped_board_payload_does_not_exhaust_the_per_object_atom_budget() {
    let mut source = b"(kicad_pcb (metadata ".to_vec();
    let start = source.len();
    source.resize(start + 16_000_001 * 2, b' ');
    for byte in source[start..].iter_mut().step_by(2) {
        *byte = b'0';
    }
    source.extend_from_slice(b") (version 20260206) (segment (start 1 2) (end 3 4)))");
    let cancel = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancel,
        progress: &|_| {},
    };
    let board = document(&source, &context).unwrap();
    assert_eq!(board.count("metadata"), 1);
    assert_eq!(board.children("segment").count(), 1);
    assert_eq!(
        board.required("version").unwrap().number(1).unwrap(),
        20260206.0
    );
}

#[test]
fn malformed_structure_is_rejected_even_inside_unparsed_metadata() {
    let cancel = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancel,
        progress: &|_| {},
    };
    for input in [
        "(kicad_pcb (unknown \"unterminated))",
        "(kicad_pcb (unknown \"truncated\\",
        "(kicad_pcb (unknown x)",
        "(kicad_pcb))",
        "(kicad_pcb)(kicad_pcb)",
        "outside (kicad_pcb)",
    ] {
        assert!(document(input.as_bytes(), &context).is_err(), "{input}");
    }
    let deep = format!("(kicad_pcb {}{})", "(x ".repeat(256), ")".repeat(256));
    assert!(document(deep.as_bytes(), &context).is_err());
}

#[test]
fn cancelled_board_scan_stops_before_index_allocation() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    let context = ImportContext {
        cancellation: &cancel,
        progress: &|_| {},
    };
    assert!(matches!(
        document(b"(kicad_pcb)", &context),
        Err(ImportError::Cancelled)
    ));
}
