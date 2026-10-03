use pomelo_core::{
    i18n::Locale,
    model::NetId,
    search::{SearchEntry, SearchIndex, SearchTarget},
    task::CancellationToken,
};

#[test]
fn han_order_matches_web_unihan_and_each_runtime_language() {
    // Expected IDs from Node 24.18.1 / ICU 78.3 / CLDR 48.0, not Rust sorting.
    let names = ["x線", "x𠀀", "x线", "x中"];
    for (locale, expected) in [
        (Locale::English, [1, 3, 2, 0]),
        (Locale::SimplifiedChinese, [2, 0, 3, 1]),
        (Locale::TraditionalChinese, [3, 2, 0, 1]),
        (Locale::Japanese, [0, 3, 1, 2]),
        (Locale::Korean, [0, 3, 1, 2]),
    ] {
        let index = SearchIndex::new(
            names
                .iter()
                .enumerate()
                .map(|(id, name)| {
                    SearchEntry::new(SearchTarget::Net(NetId(id as u32)), (*name).into(), 1)
                })
                .collect(),
        )
        .unwrap()
        .with_locale(locale)
        .unwrap();
        let actual: Vec<_> = index
            .find("x", 20)
            .iter()
            .map(|entry| entry.target)
            .collect();
        assert_eq!(
            actual,
            expected.map(|id| SearchTarget::Net(NetId(id))),
            "{}",
            locale.tag()
        );
    }
}

#[test]
fn canonical_collation_ties_keep_source_identity_in_both_insertion_orders() {
    for names in [["Pé", "Pe\u{301}"], ["Pe\u{301}", "Pé"]] {
        let index = SearchIndex::new(
            names
                .iter()
                .enumerate()
                .map(|(id, name)| {
                    SearchEntry::new(SearchTarget::Net(NetId(id as u32)), (*name).into(), id + 1)
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(index.find("p", 1)[0].name, names[0]);
    }
}

#[test]
fn numeric_identifiers_keep_web_default_lexical_order() {
    for locale in Locale::ALL {
        let index = SearchIndex::new(
            ["U2", "U10", "U1"]
                .into_iter()
                .enumerate()
                .map(|(id, name)| {
                    SearchEntry::new(SearchTarget::Net(NetId(id as u32)), name.into(), 1)
                })
                .collect(),
        )
        .unwrap()
        .with_locale(locale)
        .unwrap();
        assert_eq!(
            index
                .find("u", 20)
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["U1", "U10", "U2"]
        );
    }
}

#[test]
fn pasted_query_uses_ecmascript_whitespace_without_normalizing_source_names() {
    let index = SearchIndex::new(vec![
        SearchEntry::new(SearchTarget::Net(NetId(1)), "\u{85}VCC\u{85}".into(), 1),
        SearchEntry::new(SearchTarget::Net(NetId(2)), "VCC".into(), 1),
    ])
    .unwrap();
    assert_eq!(
        index.find("\u{feff} VcC \u{feff}", 1)[0].target,
        SearchTarget::Net(NetId(2))
    );
    assert_eq!(
        index.find("\u{85}VcC\u{85}", 1)[0].target,
        SearchTarget::Net(NetId(1))
    );
}

#[test]
fn immutable_index_crosses_background_threads_and_cancellation_discards_results() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SearchIndex>();
    let index = SearchIndex::new(vec![SearchEntry::new(
        SearchTarget::Net(NetId(1)),
        "電源".into(),
        1,
    )])
    .unwrap()
    .with_locale(Locale::TraditionalChinese)
    .unwrap();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(
        std::thread::spawn(move || index.find_cancellable("電", 20, &cancel).is_none())
            .join()
            .unwrap()
    );
}
