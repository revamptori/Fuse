# Fuss engine benchmarks

Generated automatically by `.github/workflows/ci.yml` from the output of
`rust/fuss-core/src/bin/fuss-bench.rs` on a GitHub Actions `ubuntu-22.04`
runner (x86-64, 4 vCPU, ext4). These numbers characterise the Rust engine on
desktop-class hardware with a warm page cache; they are **not** phone numbers.
On-device figures must be measured on a real device (see docs/PERFORMANCE.md).

| operation | median | p90 | iterations |
|---|---|---|---|
| list 10k files (no stat sort) | 26.17ms | 26.59ms | 20 |
| list 10k files + natural sort | 34.06ms | 34.47ms | 20 |
| list 10k files + show hidden | 26.13ms | 26.58ms | 20 |
| sort 10k entries by size | 33.53ms | 33.78ms | 20 |
| copy 500 x 4 KiB files | 235.92ms | 269.81ms | 5 |
| copy 256 MiB (streamed) | 1480.69ms | 1519.74ms | 3 |
| dir_size 10k files | 23.86ms | 24.50ms | 10 |
| search 'file_099' in 10k | 24.94ms | 25.38ms | 10 |
| zip create 500 x 4 KiB | 43.24ms | 85.70ms | 3 |
| zip list 500 entries | 0.15ms | 0.18ms | 10 |
| delete tree (2 000 files) | 251.40ms | 258.07ms | 5 |
| cancel a 256 MiB copy | 0.03ms | 0.03ms | 5 |
| encode 200-entry listing chunk | 0.00ms | 0.00ms | 200 |

Process high-water mark during the whole benchmark:

```
VmHWM:	    4316 kB
VmRSS:	    3484 kB
```

## What each line measures

- **list 10k files** — one `readdir` + `lstat` per entry, encoded into a single buffer.
- **natural sort** — the UI's default order (dirs first, `file2` before `file10`).
- **copy 256 MiB** — a single streamed copy with a 128 KiB buffer and `fsync`.
- **delete tree** — recursive removal of 2 000 files, setup excluded.
- **cancel a 256 MiB copy** — latency from `cancel()` to the operation returning.
- **encode 200-entry listing chunk** — the per-chunk JNI payload cost.

Regenerate locally with `scripts/bench-local.sh`.
