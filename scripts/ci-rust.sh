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
  if ( "$@" ) >> "$LOG" 2>&1; then
    return 0
  fi
  echo "!! FAILED: $*" >> "$LOG"
  fail=1
  # Surface the failing test names: `cargo test` prints them after "failures:".
  if [ "$1" = "cargo" ] && { [ "$2" = "test" ] || [ "$2" = "nextest" ]; }; then
    grep -E '^(test result: FAILED|    [a-z_:]+$|failures:)' "$LOG" | tail -n 20 >> "$LOG"
  fi
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
