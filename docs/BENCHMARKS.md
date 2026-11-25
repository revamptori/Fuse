# Fuss engine benchmarks

Generated automatically by `.github/workflows/ci.yml` from the output of
`rust/fuss-core/src/bin/fuss-bench.rs` on a GitHub Actions `ubuntu-22.04`
runner (x86-64, 4 vCPU, ext4). These numbers characterise the Rust engine on
desktop-class hardware with a warm page cache; they are **not** phone numbers.
On-device figures must be measured on a real device (see docs/PERFORMANCE.md).

| operation | median | p90 | iterations |
|---|---|---|---|
| list 10k files (no stat sort) | 9.05ms | 9.37ms | 20 |
| list 10k files + natural sort | 16.45ms | 16.64ms | 20 |
| list 10k files + show hidden | 9.15ms | 9.22ms | 20 |
| sort 10k entries by size | 16.52ms | 16.64ms | 20 |
| copy 500 x 4 KiB files | 475.54ms | 569.61ms | 5 |
| copy 256 MiB (streamed) | 1508.84ms | 1524.91ms | 3 |
| dir_size 10k files | 8.89ms | 13.63ms | 10 |
| search 'file_099' in 10k | 9.92ms | 9.97ms | 10 |
| zip create 500 x 4 KiB | 28.30ms | 44.23ms | 3 |
| zip list 500 entries | 0.14ms | 0.18ms | 10 |
| delete tree (2 000 files) | 553.16ms | 591.12ms | 5 |
| cancel a 256 MiB copy | 0.01ms | 0.01ms | 5 |
| encode 200-entry listing chunk | 0.00ms | 0.00ms | 200 |

Process high-water mark during the whole benchmark:

```
VmHWM:	    4312 kB
VmRSS:	    3496 kB
```

## What each line measures

- **list 10k files** — one `readdir` + `lstat` per entry, encoded into a single buffer.
- **natural sort** — the UI's default order (dirs first, `file2` before `file10`).
- **copy 256 MiB** — a single streamed copy with a 128 KiB buffer and `fsync`.
- **delete tree** — recursive removal of 2 000 files, setup excluded.
- **cancel a 256 MiB copy** — latency from `cancel()` to the operation returning.
- **encode 200-entry listing chunk** — the per-chunk JNI payload cost.

Regenerate locally with `scripts/bench-local.sh`.

Engine source digest: `d45fa8b3e2b8`
