#!/usr/bin/env python3
"""Extract the fuss-bench table from a CI log and write docs/BENCHMARKS.md.

Usage: bench-to-markdown.py <log> <out.md> [--if-stale]

With ``--if-stale`` the file is only rewritten when the sheet it holds differs
from the new measurement by more than run-to-run noise, or when the engine
sources it was measured from have changed. Benchmark timings move a few percent
between runs on identical code, so committing every digit would make CI push a
fresh "regenerate benchmarks" commit on every push, which in turn triggers
another run: an endless loop of benchmark-churn commits.
"""
import hashlib
import pathlib
import re
import subprocess
import sys

log_path, out_path = sys.argv[1], sys.argv[2]
if_stale = "--if-stale" in sys.argv[3:]
TOLERANCE = 0.15  # 15%: well above timing noise, well below any real regression
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
def table_rows(doc):
    """Map operation -> (median, p90) in ms for a benchmark sheet."""
    rows = {}
    for line in doc.splitlines():
        if line.startswith("| ") and "ms" in line:
            cells = [c.strip() for c in line.strip("|").split("|")]
            if len(cells) < 3:
                continue
            try:
                rows[cells[0]] = (float(cells[1].rstrip("ms")), float(cells[2].rstrip("ms")))
            except ValueError:
                continue
    return rows


def engine_digest():
    """Fingerprint of the engine sources the numbers were measured from."""
    root = pathlib.Path(__file__).resolve().parent.parent / "rust" / "fuss-core" / "src"
    try:
        files = sorted(root.rglob("*.rs"))
        h = hashlib.sha256()
        for f in files:
            h.update(f.relative_to(root).as_posix().encode())
            h.update(f.read_bytes())
        return h.hexdigest()[:12]
    except OSError:
        return "unknown"


def is_stale(new_doc):
    if not pathlib.Path(out_path).exists():
        return True, "no sheet yet"
    old_doc = pathlib.Path(out_path).read_text()
    if engine_digest_of(old_doc) != engine_digest():
        return True, "engine source changed since the sheet was measured"
    old, new = table_rows(old_doc), table_rows(new_doc)
    if old.keys() != new.keys():
        return True, "the set of measured operations changed"
    for op, (med, p90) in new.items():
        omed, op90 = old[op]
        # Floors keep sub-millisecond rows from being judged on noise.
        if abs(med - omed) > TOLERANCE * max(omed, 0.05) or abs(p90 - op90) > TOLERANCE * max(op90, 0.05):
            return True, f"{op} moved beyond {TOLERANCE:.0%} (was {omed}ms, now {med}ms)"
    return False, "within noise"


def engine_digest_of(doc):
    m = re.search(r"Engine source digest: `([0-9a-f]+)`", doc)
    return m.group(1) if m else ""


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
md.append("")
md.append(f"Engine source digest: `{engine_digest()}`")
text_out = "\n".join(md) + "\n"


if if_stale:
    stale, why = is_stale(text_out)
    if not stale:
        print(f"docs/BENCHMARKS.md kept: {why}")
        sys.exit(0)
    print(f"refreshing docs/BENCHMARKS.md: {why}")

open(out_path, "w").write(text_out)
print("wrote", out_path)
