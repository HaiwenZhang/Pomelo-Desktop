//! Read-only oracle probe for shared view-dependent f64 curve contours.
use anyhow::{Context as _, Result};
use pomelo_core::{
    geometry::curve::{CurveLimits, tessellate_curve_ring},
    model::{Bounds, Segment},
    task::CancellationToken,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

#[derive(Deserialize)]
struct Request {
    paths: Vec<Vec<Segment>>,
    queries: Vec<Query>,
}
#[derive(Deserialize)]
struct Query {
    path: usize,
    view: Bounds,
    tolerance: f64,
}
#[derive(Serialize)]
struct Response {
    results: Vec<Vec<pomelo_core::model::Point>>,
}
fn main() -> Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    anyhow::ensure!(args.len() == 2, "CURVE_FILL_PROBE_USAGE: REQUEST OUTPUT");
    let request: Request = serde_json::from_reader(BufReader::new(File::open(&args[0])?))?;
    let cancel = CancellationToken::default();
    let mut results = Vec::new();
    results.try_reserve_exact(request.queries.len())?;
    for query in request.queries {
        let path = request
            .paths
            .get(query.path)
            .context("CURVE_FILL_PROBE_PATH_MISSING")?;
        results.push(tessellate_curve_ring(
            path,
            query.view,
            query.tolerance,
            CurveLimits::default(),
            &cancel,
        )?);
    }
    serde_json::to_writer(
        BufWriter::new(File::create(&args[1])?),
        &Response { results },
    )?;
    Ok(())
}
