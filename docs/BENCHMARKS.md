# Fuss engine benchmarks

Generated automatically by `.github/workflows/ci.yml` from the output of
`rust/fuss-core/src/bin/fuss-bench.rs` on a GitHub Actions `ubuntu-22.04`
runner (x86-64, 4 vCPU, ext4). These numbers characterise the Rust engine on
desktop-class hardware with a warm page cache; they are **not** phone numbers.
On-device figures must be measured on a real device (see docs/PERFORMANCE.md).

| operation | median | p90 | iterations |
|---|---|---|---|
| list 10k files (no stat sort) | 13.13ms | 14.69ms | 20 |
| list 10k files + natural sort | 13.38ms | 14.35ms | 20 |
| list 10k files + show hidden | 12.66ms | 14.93ms | 20 |
| sort 10k entries by size | 13.64ms | 15.55ms | 20 |
| copy 500 x 4 KiB files | 278.20ms | 305.23ms | 5 |
| copy 256 MiB (streamed) | 487.74ms | 780.79ms | 3 |
| dir_size 10k files | 10.42ms | 12.90ms | 10 |
| search 'file_099' in 10k | 12.43ms | 14.89ms | 10 |
| zip create 500 x 4 KiB | 43.60ms | 49.66ms | 3 |
| zip list 500 entries | 0.18ms | 0.22ms | 10 |
| delete tree (2 000 files) | 40.98ms | 51.50ms | 5 |
| cancel a 256 MiB copy | 0.03ms | 0.06ms | 5 |
| encode 200-entry listing chunk | 0.00ms | 0.00ms | 200 |

Process high-water mark during the whole benchmark:

```
VmHWM:	    4168 kB
VmRSS:	    3272 kB
```

## What each line measures

- **list 10k files** — one `readdir` + `lstat` per entry, encoded into a single buffer.
- **natural sort** — the UI's default order (dirs first, `file2` before `file10`).
- **copy 256 MiB** — a single streamed copy with a 128 KiB buffer and `fsync`.
- **delete tree** — recursive removal of 2 000 files, setup excluded.
- **cancel a 256 MiB copy** — latency from `cancel()` to the operation returning.
- **encode 200-entry listing chunk** — the per-chunk JNI payload cost.

Regenerate locally with `scripts/bench-local.sh`. The curated summary and the
device-measurement gap live in [PERFORMANCE_RESULTS.md](PERFORMANCE_RESULTS.md).
