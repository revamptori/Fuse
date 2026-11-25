#!/usr/bin/env bash
# Run the engine benchmarks locally and refresh docs/BENCHMARKS.md.
#
# Usage: scripts/bench-local.sh [workdir]
#   workdir defaults to a temp directory; the tree is created and removed by the
#   harness, so point it at a real (fast) disk if /tmp is slow.
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

cd "$HERE/rust"
echo "building fuss-bench (release)…" >&2
cargo build --release --bin fuss-bench
if [ "${1:-}" != "" ]; then
  ./target/release/fuss-bench "$1" | tee "$LOG"
else
  ./target/release/fuss-bench | tee "$LOG"
fi

cd "$HERE"
python3 scripts/bench-to-markdown.py "$LOG" docs/BENCHMARKS.md >&2
echo "updated docs/BENCHMARKS.md" >&2
