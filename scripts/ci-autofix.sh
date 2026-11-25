#!/usr/bin/env bash
# Commit a `cargo fmt` fixup if the tree is not rustfmt-clean.
#
# Formatting is enforced by committing the fix rather than failing the build, so
# a contributor never has to install rustfmt to get a green run.
#
# Deliberately *not* here: benchmark numbers. Auto-committing them created a
# commit on every push whose measurement differed from the last, and on a shared
# runner the same binary measures 2.6x apart (23.7ms vs 9.1ms for list-10k in
# consecutive runs), so no tolerance separates noise from regression. Benchmarks
# are reported in the PR comment instead, and the committed sheet is refreshed on
# purpose with scripts/bench-local.sh.
#
# Cargo's vendored-source override (`rust/.cargo/config.toml`) is git-ignored, so
# it can never leak into these commits.
set -uo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
cd "$HERE"

if ( cd rust && cargo fmt --all -- --check >/dev/null 2>&1 ); then
  echo "already rustfmt-clean"
  exit 0
fi
( cd rust && cargo fmt --all ) && echo "applied rustfmt"

if git diff --quiet -- rust; then
  echo "nothing to commit"
  exit 0
fi
git config user.name "fuss-ci"
git config user.email "ci@fuss.invalid"
git add rust
git commit -q -m "chore: cargo fmt (automated)"
git push origin "HEAD:${GITHUB_REF_NAME}" || echo "push failed (protected branch?)"
