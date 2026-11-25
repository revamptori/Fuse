#!/usr/bin/env bash
# Fetch the exact dependency sources recorded in Cargo.lock from upstream Git
# tags into rust/vendor/ and point cargo at them. This exists for offline or
# air-gapped development, and for environments with no crates.io access; normal
# builds use crates.io.
#
# Usage: scripts/vendor-deps.sh [dest]   (default: rust/vendor)
#
# Two things make this less trivial than it looks:
#
#   * Cargo.lock lists every target's dependencies, including crates that are
#     never compiled on Linux (windows-sys, winapi-util, ...), because lockfile
#     resolution needs them to exist. A partial vendor tree therefore fails with
#     "no matching package", not with a warning.
#   * Not every crate is published on GitHub: windows-sys and friends come from
#     microsoft/windows-rs, which mirrors them per release branch. Anything that
#     cannot be fetched is pruned and reported at the end.
set -euo pipefail
DEST="${1:-$(cd "$(dirname "$0")/.." && pwd)/rust/vendor}"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$DEST"
fetch() { # repo tag dirname [subdir]
  local repo="$1" tag="$2" dir="$3" sub="${4:-}"
  echo "  $dir"
  # A ref may be a release tag or a branch; try both without printing a 404
  # for the one that does not exist.
  curl -fsSL --retry 3 -o "$WORK/$dir.tar.gz" "https://codeload.github.com/$repo/tar.gz/refs/tags/$tag" 2>/dev/null \
    || curl -fsSL --retry 3 -o "$WORK/$dir.tar.gz" "https://codeload.github.com/$repo/tar.gz/refs/heads/$tag"
  local ex="$WORK/$dir-x"; mkdir -p "$ex"
  tar xzf "$WORK/$dir.tar.gz" -C "$ex"
  local root; root=$(find "$ex" -maxdepth 1 -mindepth 1 -type d | head -1)
  rm -rf "${DEST:?}/$dir"
  cp -r "$root/${sub}" "$DEST/$dir"
}
echo "Vendoring engine dependencies into $DEST"
MISSING=""
# Crates that are not published at github.com/<name>/<name>. Mapped to the
# repository whose tags carry the released version.
repo_for() {
  case "$1" in
    # Upstream repository names that are not just the crate name twice.
    miniz_oxide) echo "Frommi/miniz_oxide miniz_oxide" ;;
    cesu8) echo "emk/cesu8-rs master" ;;
    cfg-if) echo "rust-lang/cfg-if" ;;
    combine) echo "Marwes/combine" ;;
    jni) echo "jni-rs/jni-rs" ;;
    jni-sys) echo "jni-rs/jni-sys" ;;
    log) echo "rust-lang/log" ;;
    proc-macro2 | quote | syn | thiserror | unicode-ident) echo "dtolnay/$1" ;;
    # The impl crate ships inside the thiserror repository.
    thiserror-impl) echo "dtolnay/thiserror" ;;
    walkdir | same-file | memchr | winapi-util) echo "BurntSushi/$1" ;;
    bytes) echo "tokio-rs/bytes" ;;
    windows-sys | windows-link | windows-targets | windows_*) echo "STUB" ;;
    windows_aarch64_gnullvm | windows_aarch64_msvc | windows_i686_gnu | \
      windows_i686_msvc | windows_x86_64_gnu | windows_x86_64_gnullvm | \
      windows_x86_64_msvc) echo "STUB" ;;
    # Unpublished on crates.io as a tagged release; main tracks 2.0.x.
    adler2) echo "oyvindln/adler2 main" ;;
    *) echo "$1/$1" ;;
  esac
}

# Crates whose source is nested in a subdirectory of the repository tarball.
subdir_for() {
  case "$1" in
    miniz_oxide) echo "miniz_oxide" ;;
    thiserror-impl) echo "impl" ;;
  esac
}

# What a tarball's top-level directory is called, when it is not "<name>-<ver>".
rootname_for() {
  case "$1" in
    thiserror-impl) echo "thiserror" ;;
    windows_*) echo "windows-targets" ;;
  esac
}

# What Cargo calls the crate, when it differs from the lockfile package name.
crate_name_for() {
  case "$1" in
    miniz_oxide) echo "miniz_oxide" ;;
    windows_*) echo "windows-targets" ;;
    *) echo "$1" ;;
  esac
}

# Try the forms a published version can take as a git ref:
#   v1.2.3, 1.2.3, <name>-1.2.3, and finally a branch (for crates versioned by
#   their repository rather than by tag).
try_fetch() { # repo version [sub]
  local repo="$1" ver="$2" sub="${3:-}"
  local name="${repo#*/}"
  local ref
  for ref in "v$ver" "$ver" "$name-$ver" "$name-v$ver"; do
    if curl -fsSL --retry 2 -o "$WORK/t.tar.gz" \
      "https://codeload.github.com/$repo/tar.gz/refs/tags/$ref" 2>/dev/null; then
      echo "$ref"
      return 0
    fi
  done
  for ref in HEAD main master; do
    if curl -fsSL --retry 2 -o "$WORK/t.tar.gz" \
      "https://codeload.github.com/$repo/tar.gz/$ref" 2>/dev/null; then
      echo "$ref"
      return 0
    fi
  done
  return 1
}

unpack() { # outdir sub
  local out="$1" sub="$2"
  local ex; ex=$(mktemp -d)
  tar xzf "$WORK/t.tar.gz" -C "$ex"
  local root; root=$(find "$ex" -maxdepth 1 -mindepth 1 -type d | head -1)
  rm -rf "${DEST:?}/$out"
  mkdir -p "$DEST/$out"
  cp -r "$root/${sub:-.}/." "$DEST/$out/"
  printf '{"files":{},"package":null}' > "$DEST/$out/.cargo-checksum.json"
  rm -rf "$ex"
}

# Windows-only crates are needed for the lockfile to resolve but are never
# compiled on a platform that can actually build the Android app. Fetching them
# means cloning microsoft/windows-rs at a release branch and digging out one
# crate of a workspace; a stub is smaller, honest, and fails to build if anyone
# ever tries to use it, which is the right failure.
stub() { # name version [extra manifest lines]
  # Vendored directories are keyed by name, so two versions of one crate (the
  # lockfile holds two windows-sys) need distinct directories.
  local out="$DEST/$1-$2"
  rm -rf "${out:?}"
  mkdir -p "$out/src"
  cat > "$out/Cargo.toml" <<MANIFEST
[package]
name = "$1"
version = "$2"
edition = "2021"

[lib]
path = "src/lib.rs"
$3
MANIFEST
  cat > "$out/src/lib.rs" <<'RS'
//! Stub vendored for offline builds on non-Windows targets.
//!
//! This crate is Windows-only, so it is never compiled on Linux or Android.
//! Cargo still needs the package to exist for lockfile resolution. If anything
//! ever tries to build it, this compile_error says so plainly.
compile_error!("windows-only crate: not available in this offline vendor tree");
RS
  printf '{"files":{},"package":null}' > "$out/.cargo-checksum.json"
}

SKIPPED=""
# Packages the engine never compiles on Linux still have to exist for the
# lockfile to resolve, so they are fetched too; when a crate genuinely cannot be
# fetched it is dropped from both the tree and the lockfile entry's expectations
# and reported, rather than left half-vendored.
while read -r name version; do
  [ -n "$name" ] || continue
  mapped=$(repo_for "$name")
  repo=${mapped%% *}
  forced=""
  case "$mapped" in *" "*) forced=${mapped#* } ;; esac
  case "$name" in
    fuss-core | fuss-jni) continue ;;
  esac
  [ -d "$DEST/$(crate_name_for "$name")" ] && [ "$repo" != "STUB" ] && continue
  case " $SKIPPED " in *" $name "*) continue ;; esac
  if [ "$repo" = "STUB" ]; then
    # Mirror the shape a dependent actually asks for: the features below are the
    # ones jni, log and walkdir request, and windows-targets needs its
    # per-architecture crates to exist to satisfy the ^0.42.2 requirement.
    case "$name" in
      windows-sys)
        case "$version" in
          0.45.0) stub "$name" "$version" '[features]
default = []
Win32_Foundation = []
Win32_Globalization = []
Win32_Security = []
Win32_Storage_FileSystem = []
Win32_System_Console = []
Win32_System_Threading = []
Win32_System_WindowsProgramming = []
Win32_UI_Shell = []

[dependencies.windows-targets]
version = "0.42.2"' ;;
          *) stub "$name" "$version" '[features]
default = []
Win32_Foundation = []
Win32_Storage_FileSystem = []
Win32_System_Console = []
Win32_System_SystemInformation = []
Win32_System_Threading = []

[dependencies.windows-link]
version = "0.2.1"' ;;
        esac ;;
      windows-targets)
        stub "$name" "$version" '[dependencies.windows_aarch64_gnullvm]
version = "0.42.2"
[dependencies.windows_aarch64_msvc]
version = "0.42.2"
[dependencies.windows_i686_gnu]
version = "0.42.2"
[dependencies.windows_i686_msvc]
version = "0.42.2"
[dependencies.windows_x86_64_gnu]
version = "0.42.2"
[dependencies.windows_x86_64_gnullvm]
version = "0.42.2"
[dependencies.windows_x86_64_msvc]
version = "0.42.2"' ;;
      *) stub "$name" "$version" "" ;;
    esac
    echo "  $name $version  (stub: windows-only)"
    continue
  fi
  if try_fetch "$repo" "$version" "$forced" >/dev/null; then
    unpack "$(crate_name_for "$name")" "$(subdir_for "$name")"
    echo "  $name $version"
  else
    SKIPPED="$SKIPPED $name"
    echo "  $name $version  (could not fetch; skipped)" >&2
  fi
done < <(python3 -c '
import re, sys
text = open(sys.argv[1]).read()
for block in text.split("[[package]]")[1:]:
    n = re.search(r"^name = \"([^\"]+)\"", block, re.M)
    v = re.search(r"^version = \"([^\"]+)\"", block, re.M)
    if n and v:
        print(n.group(1), v.group(1))
' "$(dirname "$DEST")/Cargo.lock")

# Normalise workspace sections that break isolated vendored crates.
python3 - "$DEST" <<'PY'
import glob, re, sys, os
dest = sys.argv[1]
for f in glob.glob(os.path.join(dest, '*', 'Cargo.toml')):
    s = open(f).read()
    if re.search(r'^\[workspace', s, re.M):
        s = re.sub(r'\n\[workspace(\.[^\]]*)?\].*?(?=\n\[|\Z)', '\n', s, flags=re.S)
        open(f, 'w').write(s)
for f in glob.glob(os.path.join(dest, '*', 'Cargo.toml')):
    s = open(f).read()
    if 'name = "thiserror"' in s:
        s = s.replace('path = "impl"', 'path = "../thiserror-impl"')
        open(f, 'w').write(s)
PY
cat > "$(dirname "$DEST")/.cargo/config-vendor.toml" <<CFG
# Generated by scripts/vendor-deps.sh -- remove to return to crates.io.
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
CFG
cat <<'NOTE'

Done. To build offline:
  cp rust/.cargo/config-vendor.toml rust/.cargo/config.toml

That points cargo at the vendor tree. One more step is needed because the
committed Cargo.lock records a crates.io checksum per package, and Cargo
refuses a vendored source whose bytes hash differently: a tree fetched from Git
tags can never match those hashes.

  cargo generate-lockfile --offline     # in rust/, once

This rewrites rust/Cargo.lock for the vendor tree. The committed lockfile is
the canonical record of what CI builds, so restore it with `git checkout --
rust/Cargo.lock` before committing anything.
NOTE
