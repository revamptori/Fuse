# Fuss

A minimal, fast Android file manager. The filesystem work is done by a Rust
engine behind a small JNI surface; the interface is Jetpack Compose with
Material 3. No ads, no analytics, no network permission, no background services.

* **Android 11 (API 30) and newer**, arm64-v8a primary, armeabi-v7a and x86_64
  also built.
* **No broad storage permission.** Media folders are read directly; anything the
  system hides is reached through *Add folder* (Storage Access Framework), one
  folder at a time, with graceful handling when a grant is revoked.
* **Never blocks the main thread.** Every filesystem call runs on a background
  dispatcher inside the engine, and long operations are cancellable.

## Install

Grab an APK from the latest [release](../../releases) (`arm64-v8a` unless you
know otherwise), allow install-from-unknown-source when prompted, and open it.
Release notes and checksums are attached to each release. See
[docs/BUILD.md](docs/BUILD.md) for building it yourself.

## What it does

| Area | Capability |
| --- | --- |
| Browsing | Internal storage, SD cards, app-private folders, SAF trees; breadcrumbs; list and grid; hidden files toggle; sort by name, size, type, date (either direction) |
| Operations | Create file/folder, rename, copy, move, delete, multi-select and select-all, collision policy (fail / overwrite / rename) |
| Search | Incremental, debounced recursive search with glob support and a live "scanned / elapsed" readout |
| Properties | Size, counts, permissions, owner, inode, hard links, symlink target, MIME; folder size on demand |
| Archives | Browse ZIP/TAR/TAR.GZ/TGZ/GZIP without extracting; streamed extract; create ZIP/TAR/TAR.GZ |
| Analysis | Folder size breakdown, largest files, empty folders, type statistics, duplicate detection by content hash |
| Favourites | Pinned folders, recent locations, per-folder access grants |

Deliberately **not** included: cloud accounts, network access, telemetry, root
requirements, media players, or a web UI.

## How it is put together

```
android/app  (Kotlin)          rust/  (Rust)
├── ui/        Compose screens ├── fuss-core/  engine: listing, fsops, search,
├── app/       Android bits    │               archives, analysis, mounts
│   ├── StorageRoots, SafStore,│                
│   ├── SafBrowser (content://) │               
│   └── BrowseViewModel        └── fuss-jni/   JNI surface + wire encoding
├── core/      FussNative (JNI declarations), wire Decoder, FsRepository
```

* The engine deals only in real filesystem paths and never sees Android APIs.
* Kotlin owns Android specifics: storage volumes, SAF `content://` documents,
  intents, permissions, and UI state.
* The JNI surface is small and byte-oriented (`fuss-jni/src/lib.rs`); results are
  decoded once on an IO thread into plain Kotlin objects, so nothing of size is
  copied across the boundary twice.
* Architecture details, the wire format, and the reasoning behind each
  dependency are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Build

Prerequisites: Rust (stable), Android SDK with NDK 27, JDK 17.

```bash
./scripts/build-rust-android.sh      # cross-compile libfuss_core.so for each ABI
cd android && ./gradlew assembleDebug
```

The Gradle build runs the Rust step automatically; set `FUSS_SKIP_RUST=1` to
reuse existing native libraries. Full instructions, including a reproducible
offline path, are in [docs/BUILD.md](docs/BUILD.md).

## Test and benchmark

```bash
cd rust && cargo test --workspace        # engine + JNI unit tests
./scripts/rust-check.sh                  # fmt, clippy -D warnings, tests
cargo run --release --bin fuss-bench     # engine micro-benchmarks
```

Benchmark methodology and the latest numbers: [docs/PERFORMANCE.md](docs/PERFORMANCE.md)
and the machine-generated [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

## Status and honesty notes

* The Rust engine is covered by unit tests (listing, sorting, copy/move/delete,
  collision handling, cancellation, ZIP/TAR/GZIP round-trips, analysis,
  search) and those tests run on every push.
* The Android app compiles in CI and produces debug and release APKs for all
  three ABIs. Runtime behaviour on a physical device has **not** been verified by
  the project yet; treat the first builds as test releases and report what you
  find. Nothing in this README claims a runtime feature works that has never been
  executed.
* `fuss-release-*.apk` is signed with a test key unless a maintainer provides a
  distribution key. Check the certificate before trusting a build.

## Licence

See [LICENSE](LICENSE) if present in the repository.
