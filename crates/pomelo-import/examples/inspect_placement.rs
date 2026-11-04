//! Read-only placement diagnostics for source rectangles and their footprint owners.
use pomelo_core::task::CancellationToken;
use pomelo_import::{
    ImportContext, ImportOptions,
    formats::allegro::{
        database::BrdDatabase,
        decoder::{DecodeLimits, DecodedRecord, fixed::FixedRecord},
        index::IndexLimits,
    },
};
use std::{collections::BTreeMap, path::PathBuf};
fn main() -> anyhow::Result<()> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("BRD path required"))?,
    );
    let cancel = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancel,
        progress: &|_| {},
    };
    let database = BrdDatabase::read_path(
        &path,
        &ImportOptions::default(),
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context,
    )?;
    let mut rects = BTreeMap::new();
    let mut samples = vec![];
    let mut owners = std::collections::BTreeSet::new();
    for record in database.records_of_type(0x0e, &context) {
        let record = record?;
        if let DecodedRecord::Fixed(FixedRecord::FootprintRectangle(r)) = record.fields {
            *rects.entry(r.layer).or_insert(0usize) += 1;
            owners.insert(r.fp_ptr);
            if samples.len() < 8 {
                let owner = database.get(
                    pomelo_import::formats::allegro::index::RecordKey(r.fp_ptr),
                    &context,
                )?;
                samples.push(serde_json::json!({"rectangle":r,"owner":owner.map(|v|v.fields)}));
            }
        }
    }
    let mut area_layers = BTreeMap::new();
    let mut endings = BTreeMap::new();
    let mut placements = BTreeMap::new();
    let mut shape_samples = vec![];
    for record in database.records_of_type(0x2d, &context) {
        let record = record?;
        if let DecodedRecord::Fixed(FixedRecord::FootprintInstance(fp)) = record.fields {
            *placements.entry(fp.layer).or_insert(0usize) += 1;
            let mut key = fp.areas_ptr;
            let mut seen = std::collections::BTreeSet::new();
            while key != 0 && key != fp.key.0 && !database.header().sentinel_keys.contains(&key) {
                if !seen.insert(key) {
                    anyhow::bail!("area cycle");
                }
                let Some(record) = database.get(
                    pomelo_import::formats::allegro::index::RecordKey(key),
                    &context,
                )?
                else {
                    anyhow::bail!("missing area");
                };
                match record.fields {
                    DecodedRecord::Fixed(FixedRecord::FootprintRectangle(r)) => {
                        *area_layers.entry(r.layer).or_insert(0usize) += 1;
                        key = r.next;
                    }
                    DecodedRecord::Fixed(FixedRecord::Shape(r)) => {
                        *area_layers.entry(r.layer).or_insert(0usize) += 1;
                        if shape_samples.len() < 4 {
                            shape_samples.push(serde_json::json!({"fp":fp.key,"shape":r}));
                        }
                        key = r.next;
                    }
                    _ => {
                        *endings.entry(record.span.record_type).or_insert(0usize) += 1;
                        break;
                    }
                }
            }
        }
    }
    let board = pomelo_import::BoardImporter::import(
        &pomelo_import::formats::allegro::AllegroImporter,
        &path,
        &ImportOptions::default(),
        &context,
    )?;
    let missing:Vec<_>=board.scene.components.iter().filter(|c|c.body.is_none()).map(|c|serde_json::json!({"id":c.id,"reference":c.reference,"pins":c.pins.len(),"at":c.at})).collect();
    let selected: Vec<_> = board
        .scene
        .components
        .iter()
        .filter(|c| ["J3301", "J3302", "U4303"].contains(&c.reference.as_str()))
        .collect();
    let texts: Vec<_> = board
        .scene
        .texts
        .iter()
        .filter(|c| ["J3301", "J3302", "U1010"].contains(&c.text.as_str()))
        .collect();
    println!(
        "{}",
        serde_json::json!({"rectangles_by_layer":rects,"owners":owners.len(),"samples":samples,"area_layers":area_layers,"area_endings":endings,"placement_flags":placements,"shape_samples":shape_samples,"missing_bodies":missing,"selected":selected,"texts":texts})
    );
    Ok(())
}
