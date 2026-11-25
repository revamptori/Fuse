#!/usr/bin/env bash
# Local developer helper: format, lint, test and bench the Rust engine.
set -euo pipefail
cd "$(dirname "$0")/../rust"
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all "$@"
cargo build --release -p fuss-core
./target/release/fuss-bench
