# Testing

Three levels, in the order you should run them.

## 1. Engine (host, no device)

```bash
cd rust
cargo test --workspace          # debug
cargo test --workspace --release
./../scripts/rust-check.sh      # fmt + clippy -D warnings + both test runs
```

What is covered today:

| Test group | Checks |
| --- | --- |
| `listing` | entry decoding, hidden-file filtering, page/offset behaviour, natural name order, dirs-first, descending sorts, stat failures |
| `fsops` | create/rename, copy file and tree, move across directories, collision modes (fail/overwrite/rename), delete recursion, batch partial-failure reporting, cancellation mid-copy |
| `archive` | ZIP create → list → extract round-trip, stored and deflated entries, CRC verification, truncated/corrupt archives, TAR and TAR.GZ round-trips, path-traversal rejection on extraction |
| `analyze` | folder sizes, largest files, empty folders, type statistics, duplicate detection on identical and near-identical content |
| `search` | substring and glob matching, hidden-file behaviour, result caps, cancellation |
| `path` | normalisation, traversal rejection, parent/child checks |
| `fuss-jni` | cancel handle lifecycle, progress reporting, codec ok/error encoding |

Run a single test with `cargo test -p fuss-core collision` for example.

## 2. Kotlin unit tests (host JVM)

```bash
cd android && ./gradlew testDebugUnitTest
```

`DecoderTest` pins the wire format from the consumer side (all field widths,
entry flags, error payloads, null payloads). `FormatTest` covers size and
permission formatting. These do not load the native library, which is why all of
`FsRepository`'s decoding lives in a pure-Kotlin `Decoder`.

## 3. Device tests (instrumented)

The engine's Android behaviour can only be verified on a device or emulator with
API 30+. There is an instrumented suite in `android/app/src/androidTest` that you
run with:

```bash
cd android && ./gradlew connectedDebugAndroidTest
```

It expects the native library to be present for the target ABI (the debug build
does this automatically) and exercises, on real storage:

* the JNI `version()` and `selfTest()` calls,
* listing a real directory and decoding entries,
* create → rename → copy → move → delete in app-private storage,
* a cancelled copy reporting "cancelled" rather than success,
* a low-space failure being reported as `no_space` rather than swallowed,
* SAF: granting a tree, listing it, importing a file, and revoking the grant to
  confirm the UI reports it instead of showing an empty folder.

On a physical device, also try by hand: a folder with tens of thousands of files
(scrolling and sorting), interrupting a large copy, moving between internal and
SD storage, and opening files with no installed handler.

## Manual checklist for a release candidate

1. Cold start on a mid-range phone feels immediate; no white flash.
2. Browse internal storage, an SD card, and a SAF-granted folder.
3. Create, rename, copy, move, delete — including a name collision and a deep
   recursive delete.
4. Cancel a large copy mid-way; the partial result is reported honestly.
5. Search while a copy is running; the UI stays responsive.
6. Open a ZIP without extracting, then extract it.
7. Run storage analysis on a large folder; tabs populate and the size bars match
   the folder list.
8. Turn the screen during a long operation; state survives.
9. Revoke the SAF grant from system settings, return to the app, open the tree.
10. Check the app requests no network permission (`adb shell dumpsys package cloud.fuss | grep -i permission`).

## Benchmarks

```bash
cd rust && cargo run --release --bin fuss-bench
```

Writes machine-readable timings to stdout; CI turns them into
[docs/BENCHMARKS.md](BENCHMARKS.md). Methodology and how to read the numbers are
in [docs/PERFORMANCE.md](PERFORMANCE.md).
