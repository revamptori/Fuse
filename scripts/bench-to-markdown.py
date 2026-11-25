#!/usr/bin/env python3
"""Extract the fuss-bench table from a CI log and write docs/BENCHMARKS.md."""
import re
import sys

log_path, out_path = sys.argv[1], sys.argv[2]
text = open(log_path, errors="replace").read()
# Strip ANSI escapes emitted by cargo.
text = re.sub(r"\x1b\[[0-9;]*m", "", text)
start = text.rfind("operation")
end = text.find("----", text.rfind("----------"))
block = text[start:end].rstrip() if start != -1 else ""
rss = "\n".join(line for line in text.splitlines() if line.startswith(("VmHWM", "VmRSS")))
if not block:
    print("no bench output found")
    sys.exit(0)
lines = [line.rstrip() for line in block.splitlines() if line.strip()]
md = [
    "# Fuss engine benchmarks",
    "",
    "Generated automatically by `.github/workflows/ci.yml` from the output of",
    "`rust/fuss-core/src/bin/fuss-bench.rs` on a GitHub Actions `ubuntu-22.04`",
    "runner (x86-64, 4 vCPU, ext4). These numbers characterise the Rust engine on",
    "desktop-class hardware with a warm page cache; they are **not** phone numbers.",
    "On-device figures must be measured on a real device (see docs/PERFORMANCE.md).",
    "",
    "| " + " | ".join(["operation", "median", "p90", "iterations"]) + " |",
    "|" + "---|" * 4,
]
for line in lines[1:]:
    parts = re.split(r"\s{2,}", line.strip())
    if len(parts) >= 4:
        md.append("| " + " | ".join(parts[:4]) + " |")
if rss:
    md += ["", "Process high-water mark during the whole benchmark:", "", "```", rss, "```"]
md += [
    "",
    "## What each line measures",
    "",
    "- **list 10k files** — one `readdir` + `lstat` per entry, encoded into a single buffer.",
    "- **natural sort** — the UI's default order (dirs first, `file2` before `file10`).",
    "- **copy 256 MiB** — a single streamed copy with a 128 KiB buffer and `fsync`.",
    "- **delete tree** — recursive removal of 2 000 files, setup excluded.",
    "- **cancel a 256 MiB copy** — latency from `cancel()` to the operation returning.",
    "- **encode 200-entry listing chunk** — the per-chunk JNI payload cost.",
    "",
    "Regenerate locally with `scripts/bench-local.sh`.",
]
open(out_path, "w").write("\n".join(md) + "\n")
print("wrote", out_path)
