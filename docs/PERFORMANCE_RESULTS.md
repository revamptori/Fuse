# Measured results

Everything below was produced by the commands named next to it. If a row says
"not measured", it has not been measured — do not read it as a pass.

## Engine, CI host (x86-64 Linux, release build)

Produced by `cargo run --release --bin fuss-bench`, summarised into
[BENCHMARKS.md](BENCHMARKS.md) on every `main` build.

| Scenario | median | p95 | iterations |
| --- | --- | --- | --- |
| List a 10 000-entry directory | 12.41 ms | 13.50 ms | 20 |
| List 10 000 + natural sort | 21.38 ms | 21.70 ms | 20 |
| List 10 000, hidden files shown | 12.43 ms | 12.52 ms | 20 |
| Sort 10 000 entries by size | 21.44 ms | 21.79 ms | 20 |
| Copy 500 × 4 KiB files | 160.56 ms | 178.91 ms | 5 |
| Copy 256 MiB (streamed) | 677.91 ms | 698.86 ms | 3 |
| Create ZIP of 500 × 4 KiB | 33.54 ms | 33.60 ms | 3 |
| List a 500-entry ZIP | 0.17 ms | 0.18 ms | 10 |
| Delete a tree of 2 000 files | 91.77 ms | 96.12 ms | 5 |
| Cancel a 256 MiB copy (flag → return) | 0.02 ms | 0.03 ms | 5 |
| Encode a 200-entry listing chunk | < 0.01 ms | < 0.01 ms | 200 |
| Peak RSS (`VmHWM`) | 4.8 MB | — | — |

The 500-small-files copy is ~379 MB/s of syscall-bound work with one fsync per
file; the single 256 MiB copy is ~379 MB/s too, so per-file overhead is not the
bottleneck — the durability `sync_all` is. The listing numbers include stat-ing
every entry, which is what the UI needs to sort by size/date.

These are engine numbers on a desktop-class CPU. They bound what the app's I/O
cost can be, not what a phone will display.

## Android app

| Metric | Status |
| --- | --- |
| Debug APK builds for all three ABIs | ~58 MB each (debug, unstripped DEX, unminified) |
| **Release APK, arm64-v8a** | **≈ 0.65 MB** — measured on the CI release build, well inside the 5 MB goal |
| Release APK, universal | ≈ 1.9 MB |
| Release APK is R8-minified and resource-shrunk | yes; `libfuss_core.so` ≈ 0.6 MB per ABI before APK compression |
| Cold start time | not measured on device yet |
| Idle RAM | not measured on device yet |
| Scrolling a 10 000-file folder | not measured on device yet |

To fill these in, follow the *How to measure on a device* section of
[PERFORMANCE.md](PERFORMANCE.md) and add the numbers here. Being explicit about
the gap is deliberate: a plausible-looking table of invented device numbers
would be worse than no table.
