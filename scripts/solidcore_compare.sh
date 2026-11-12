#!/usr/bin/env bash
# Runs the solidcore comparison (docs/pcb-3d-solidcore-experiment-plan.md) on boards given as
# arguments, every backend, writing <out>/<board>-<backend>.json and .time (peak RSS).
# Usage: scripts/solidcore_compare.sh <out-dir> <timeout-seconds> <board>...
set -u
out=$1; timeout=$2; shift 2
mkdir -p "$out"
cargo build --release -p pomelo-render --features kernel-3d --example solid_profile >&2 || exit 1
bin=target/release/examples/solid_profile
for board in "$@"; do
  name=$(basename "$board"); name=${name%.*}
  for backend in direct kernel-thread kernel-ops; do
    echo "== $name $backend" >&2
    /usr/bin/time -l "$bin" "$board" --backend "$backend" --timeout "$timeout" \
      > "$out/$name-$backend.json" 2> "$out/$name-$backend.time"
  done
done
