#!/usr/bin/env bash
# When rustfmt reports differences, apply them and push a fixup commit back to
# the same branch. Cargo's `.cargo/config.toml` vendored-source override is
# git-ignored, so it never leaks into the commit.
set -uo pipefail
cd "$(dirname "$0")/../rust"
if cargo fmt --all -- --check >/dev/null 2>&1; then
  echo "formatting already clean"
  exit 0
fi
cargo fmt --all
cd ..
if git diff --quiet -- rust; then
  echo "nothing to commit"
  exit 0
fi
git config user.name "fuss-ci"
git config user.email "ci@fuss.invalid"
git add rust
git commit -m "style: cargo fmt (automated)" -q
git push origin "HEAD:${GITHUB_REF_NAME}" || echo "push failed (protected branch?)"
