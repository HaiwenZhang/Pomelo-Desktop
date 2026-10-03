//! Independent native search endpoint for the read-only Web oracle.
use std::{fs, path::PathBuf, time::Instant};

use anyhow::{Context, ensure};
use pomelo_core::{
    i18n::Locale,
    model::{NetId, ObjectId},
    search::{SearchEntry, SearchIndex, SearchTarget},
    task::CancellationToken,
};
use pomelo_import::{
    BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Input {
    locales: Vec<Locale>,
    queries: Vec<Query>,
    #[serde(default)]
    entries: Vec<Item>,
}
#[derive(Deserialize)]
struct Query {
    query: String,
    limit: usize,
}
#[derive(Deserialize)]
struct Item {
    kind: String,
    id: Value,
    name: String,
    count: usize,
}

fn item(entry: &SearchEntry) -> Value {
    match entry.target {
        SearchTarget::Net(id) => {
            json!({"kind":"net", "id":id.0, "name":entry.name,"count":entry.count})
        }
        _ => json!({"kind":"component", "id":entry.name,"name":entry.name,"count":entry.count}),
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 4, "SEARCH_ORDER_PROBE_USAGE");
    let source = PathBuf::from(&args[0]);
    let input: Input = serde_json::from_slice(&fs::read(&args[2])?)?;
    let cancel = CancellationToken::default();
    let entries = if args[1] == "synthetic" {
        input
            .entries
            .into_iter()
            .enumerate()
            .map(|(ordinal, value)| {
                let target = match value.kind.as_str() {
                    "net" => SearchTarget::Net(NetId(u32::try_from(
                        value.id.as_u64().context("SEARCH_ORDER_NET_ID")?,
                    )?)),
                    "component" => SearchTarget::Component(ObjectId(u32::try_from(ordinal)?)),
                    _ => anyhow::bail!("SEARCH_ORDER_KIND"),
                };
                Ok(SearchEntry::new(target, value.name, value.count))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        let board = AllegroImporter.import(
            &source,
            &ImportOptions {
                text_encoding: TextEncoding::from_tag(&args[1].to_string_lossy())
                    .context("SEARCH_ORDER_ENCODING")?,
                ..Default::default()
            },
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )?;
        SearchIndex::build(&board.scene, &cancel)?
            .context("SEARCH_ORDER_CANCELLED")?
            .entries()
            .to_vec()
    };
    let items: Vec<_> = entries.iter().map(item).collect();
    let mut locales = Vec::new();
    for locale in input.locales {
        let index = SearchIndex::new(entries.clone())?.with_locale(locale)?;
        let positions: std::collections::HashMap<_, _> = index
            .entries()
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| (std::ptr::from_ref(entry), ordinal))
            .collect();
        let mut duration = 0;
        let queries: Vec<_> = input
            .queries
            .iter()
            .map(|request| {
                let start = Instant::now();
                let matches = index.find(&request.query, request.limit);
                duration += start.elapsed().as_micros();
                let results: Vec<_> = matches
                    .iter()
                    .map(|entry| {
                        positions
                            .get(&std::ptr::from_ref(*entry))
                            .copied()
                            .context("SEARCH_ORDER_IDENTITY")
                    })
                    .collect::<anyhow::Result<_>>()?;
                Ok(json!({"query":request.query,"limit":request.limit,"results":results}))
            })
            .collect::<anyhow::Result<_>>()?;
        locales.push(
            json!({"locale":index.collation_locale().tag(),"queries":queries,
            "query_microseconds":duration}),
        );
    }
    fs::write(
        &args[3],
        serde_json::to_vec(&json!({"entries":items,"locales":locales}))?,
    )?;
    Ok(())
}
