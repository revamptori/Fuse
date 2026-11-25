#!/usr/bin/env bash
# CI entry point for the Rust half. Every stage is logged to $LOG so the
# reporter step can post diagnostics even when `set -e` aborts the script.
set -uo pipefail
LOG="${LOG:-ci-rust.log}"
: > "$LOG"
cd "$(dirname "$0")/../rust"
fail=0
run() {
  echo "===== $* =====" >> "$LOG"
  ( "$@" ) >> "$LOG" 2>&1 || { echo "!! FAILED: $*" >> "$LOG"; fail=1; }
}
# Formatting is enforced by the autofix step (it commits the fmt result), so a
# dirty fmt check must not fail the build; clippy below still gates on warnings.
run cargo fmt --all -- --check
run cargo clippy --all-targets -- -D warnings
run cargo test --all
run cargo test --all --release
if [ "$fail" = 0 ]; then
  echo "===== bench (release) =====" >> "$LOG"
  cargo build --release -p fuss-core >> "$LOG" 2>&1 && ./target/release/fuss-bench >> "$LOG" 2>&1
fi
echo "===== result: $([ $fail = 0 ] && echo PASS || echo FAIL) =====" >> "$LOG"
cp "$LOG" ../ci-rust.log
exit $fail
