# Performance

Fuss is built around a small set of measurable goals. They are goals, not
marketing claims: the numbers that have actually been measured are recorded in
[PERFORMANCE_RESULTS.md](PERFORMANCE_RESULTS.md), and anything that has not been
measured is labelled as such.

## Goals

| Metric | Goal | Current status |
| --- | --- | --- |
| Release APK size (per ABI) | < 5 MB | **met**: arm64 release APK is ≈ 0.65 MB (CI-built) |
| Idle RAM | < 20 MB | not yet measured |
| Cold start to first frame | < 300 ms | not yet measured |
| Listing a 10 000-file directory | < 100 ms | engine-only measurement in `docs/BENCHMARKS.md` |
| Large copy throughput | disk-bound, flat memory | 256 MiB in ~0.8 s on the CI host (engine) |
| Cancel latency | < 50 ms | 0.03 ms measured in the engine (poll per file/64 KiB) |
| Copies buffered in memory | 0 | enforced by design: fixed-buffer streaming |

## What is already guaranteed by construction

* **No whole-file buffering.** `copy_file`, archive writers and archive readers
  use fixed buffers (64 KiB–1 MiB depending on path) and loop with
  `read`/`write`. Peak RSS does not grow with file size. The engine benchmark
  reports peak RSS (`VmHWM`) as part of every run.
* **No main-thread filesystem work.** Every native call is inside
  `FsRepository`, which switches to `Dispatchers.IO`; the Compose layer only
  reads immutable state.
* **Cancellation is polled, not signalled.** Every long loop checks
  `CancelToken` between files and between buffer chunks, so a cancel from the UI
  takes effect within one chunk (~64 KiB) of I/O.
* **Paged listings.** `list_dir` can return a page with `has_more`, so the first
  screen of a huge directory renders while the scan is still running.
* **No per-file JNI crossings.** One call returns one encoded `ByteArray` for a
  whole page or op result; the JVM sees one crossing per user action.
* **Small native library.** Release profile: `opt-level = "s"`, fat LTO,
  `codegen-units = 1`, `strip = "symbols"`, plus `llvm-strip` on the copied `.so`.

## How to measure on a device

Cold start (with a device attached, app force-stopped first):

```bash
adb shell am force-stop cloud.fuss
adb shell am start -W -n cloud.fuss/.MainActivity   # reports launch time
adb shell dumpsys gfxinfo cloud.fuss                # frame stats
```

Memory:

```bash
adb shell dumpsys meminfo cloud.fuss | head -20     # PSS after sitting idle
```

APK size:

```bash
ls -l android/app/build/outputs/apk/release/*.apk
unzip -l android/app/build/outputs/apk/release/app-release-arm64-v8a.apk | tail -1
```

Engine timings (no device needed):

```bash
cd rust && cargo run --release --bin fuss-bench
```

Benchmark scenarios: listing a 10 000-entry directory, natural-name sorting,
copying 256 MiB, cancelling a copy, deleting 2 000 files, and creating a ZIP of
500 entries. The harness writes `key=value` lines that
`scripts/bench-to-markdown.py` converts into `docs/BENCHMARKS.md` on every CI run
of `main`.

## Known trade-offs

* **`material-icons-extended`** adds roughly 1 MB of DEX before shrinking. R8
  removes unused pictograms, but the release APK still pays for the ones the UI
  references. Replacing it with a handful of hand-written vectors is the obvious
  way to claw back space if the 5 MB goal is missed.
* **Coil** adds a few hundred KB but provides thumbnail decoding, downsampling
  and caching that would otherwise be hand-written and slower.
* **x86_64 is packaged** for emulator testing, which inflates the universal APK;
  per-ABI splits keep real-device downloads small.
