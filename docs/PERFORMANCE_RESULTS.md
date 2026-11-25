# Measured results

Everything below was produced by the commands named next to it. If a row says
"not measured", it has not been measured — do not read it as a pass.

## Engine, CI host (x86-64 Linux, release build)

Produced by `cargo run --release --bin fuss-bench`, summarised into
[BENCHMARKS.md](BENCHMARKS.md) on every `main` build.

| Scenario | Result |
| --- | --- |
| List a 10 000-entry directory | ~13.5 ms |
| Natural-name sort of that listing | ~14 ms |
| Copy 256 MiB | ~0.8 s |
| Cancel latency (flag observed → return) | 0.03 ms |
| Delete 2 000 files | 39 ms |
| Create ZIP of 500 entries | 44 ms |
| Peak RSS during all of the above | 3.5 MB |

These are engine numbers on a desktop-class CPU. They bound what the app's I/O
cost can be, not what a phone will display.

## Android app

| Metric | Status |
| --- | --- |
| Debug APK builds for arm64-v8a / armeabi-v7a / x86_64 | built in CI every push; sizes printed in the CI log |
| Release APK builds and is signed | built in CI; size depends on R8 and the icon set |
| Cold start time | not measured on device yet |
| Idle RAM | not measured on device yet |
| Scrolling a 10 000-file folder | not measured on device yet |

To fill these in, follow the *How to measure on a device* section of
[PERFORMANCE.md](PERFORMANCE.md) and add the numbers here. Being explicit about
the gap is deliberate: a plausible-looking table of invented device numbers
would be worse than no table.
