#!/usr/bin/env bash
# Commit build artifacts that must live in the repository:
#
#   * `cargo fmt` fixups (formatting is enforced by committing the result rather
#     than by failing the build, so a contributor never has to run rustfmt);
#   * docs/BENCHMARKS.md, generated from the benchmark run in the same job.
#
# Cargo's vendored-source override (`rust/.cargo/config.toml`) is git-ignored, so
# it can never leak into these commits.
set -uo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
cd "$HERE"

# 1. Formatting.
if ( cd rust && ! cargo fmt --all -- --check >/dev/null 2>&1 ); then
  ( cd rust && cargo fmt --all ) && echo "applied rustfmt"
fi

# 2. Benchmarks -> markdown (only when the harness actually ran).
if grep -q 'fuss-bench\|list 10k' ci-rust.log 2>/dev/null; then
  python3 scripts/bench-to-markdown.py ci-rust.log docs/BENCHMARKS.md --if-stale \
    && echo "regenerated docs/BENCHMARKS.md"
fi

# Only commit what this script owns.
if git diff --quiet -- rust docs/BENCHMARKS.md; then
  echo "nothing to commit"
  exit 0
fi
git config user.name "fuss-ci"
git config user.email "ci@fuss.invalid"
git add rust docs/BENCHMARKS.md
git commit -q -m "chore: cargo fmt + regenerate benchmarks (automated)"
git push origin "HEAD:${GITHUB_REF_NAME}" || echo "push failed (protected branch?)"
