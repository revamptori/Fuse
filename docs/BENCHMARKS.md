# Fuss engine benchmarks

Generated automatically by `.github/workflows/ci.yml` from the output of
`rust/fuss-core/src/bin/fuss-bench.rs` on a GitHub Actions `ubuntu-22.04`
runner (x86-64, 4 vCPU, ext4). These numbers characterise the Rust engine on
desktop-class hardware with a warm page cache; they are **not** phone numbers.
On-device figures must be measured on a real device (see docs/PERFORMANCE.md).

| operation | median | p90 | iterations |
|---|---|---|---|
| list 10k files (no stat sort) | 12.87ms | 13.17ms | 20 |
| list 10k files + natural sort | 18.07ms | 18.28ms | 20 |
| list 10k files + show hidden | 12.90ms | 13.01ms | 20 |
| sort 10k entries by size | 17.81ms | 18.30ms | 20 |
| copy 500 x 4 KiB files | 244.73ms | 312.80ms | 5 |
| copy 256 MiB (streamed) | 1364.50ms | 1450.26ms | 3 |
| dir_size 10k files | 11.47ms | 11.88ms | 10 |
| search 'file_099' in 10k | 12.13ms | 12.29ms | 10 |
| zip create 500 x 4 KiB | 19.95ms | 20.04ms | 3 |
| zip list 500 entries | 0.10ms | 0.11ms | 10 |
| delete tree (2 000 files) | 210.06ms | 229.66ms | 5 |
| cancel a 256 MiB copy | 0.01ms | 0.02ms | 5 |
| encode 200-entry listing chunk | 0.00ms | 0.00ms | 200 |

Process high-water mark during the whole benchmark:

```
VmHWM:	    4312 kB
VmRSS:	    3480 kB
```

## What each line measures

- **list 10k files** — one `readdir` + `lstat` per entry, encoded into a single buffer.
- **natural sort** — the UI's default order (dirs first, `file2` before `file10`).
- **copy 256 MiB** — a single streamed copy with a 128 KiB buffer and `fsync`.
- **delete tree** — recursive removal of 2 000 files, setup excluded.
- **cancel a 256 MiB copy** — latency from `cancel()` to the operation returning.
- **encode 200-entry listing chunk** — the per-chunk JNI payload cost.

Regenerate locally with `scripts/bench-local.sh`.
