# Fuss engine benchmarks

Generated automatically by `.github/workflows/ci.yml` from the output of
`rust/fuss-core/src/bin/fuss-bench.rs` on a GitHub Actions `ubuntu-22.04`
runner (x86-64, 4 vCPU, ext4). These numbers characterise the Rust engine on
desktop-class hardware with a warm page cache; they are **not** phone numbers.
On-device figures must be measured on a real device (see docs/PERFORMANCE.md).

| operation | median | p90 | iterations |
|---|---|---|---|
| list 10k files (no stat sort) | 23.67ms | 25.34ms | 20 |
| list 10k files + natural sort | 31.50ms | 33.14ms | 20 |
| list 10k files + show hidden | 24.06ms | 26.97ms | 20 |
| sort 10k entries by size | 33.51ms | 34.05ms | 20 |
| copy 500 x 4 KiB files | 464.39ms | 1538.48ms | 5 |
| copy 256 MiB (streamed) | 1502.89ms | 1870.33ms | 3 |
| dir_size 10k files | 24.33ms | 24.76ms | 10 |
| search 'file_099' in 10k | 26.28ms | 29.40ms | 10 |
| zip create 500 x 4 KiB | 52.01ms | 230.01ms | 3 |
| zip list 500 entries | 0.15ms | 0.17ms | 10 |
| delete tree (2 000 files) | 283.82ms | 375.93ms | 5 |
| cancel a 256 MiB copy | 0.03ms | 0.03ms | 5 |
| encode 200-entry listing chunk | 0.00ms | 0.00ms | 200 |

Process high-water mark during the whole benchmark:

```
VmHWM:	    4300 kB
VmRSS:	    3444 kB
```

Engine source digest: `d45fa8b3e2b8`

## What each line measures

- **list 10k files** — one `readdir` + `lstat` per entry, encoded into a single buffer.
- **natural sort** — the UI's default order (dirs first, `file2` before `file10`).
- **copy 256 MiB** — a single streamed copy with a 128 KiB buffer and `fsync`.
- **delete tree** — recursive removal of 2 000 files, setup excluded.
- **cancel a 256 MiB copy** — latency from `cancel()` to the operation returning.
- **encode 200-entry listing chunk** — the per-chunk JNI payload cost.

Regenerate locally with `scripts/bench-local.sh`.
