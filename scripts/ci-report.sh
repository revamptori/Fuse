#!/usr/bin/env bash
# Upsert a CI status comment on the current PR with the tail of the log file.
# Usage: ci-report.sh <marker> <title> <logfile> [status]
set -uo pipefail
marker="$1"; title="$2"; log="$3"; status="${4:-unknown}"
if [ -z "${GH_TOKEN:-}" ] && ! command -v gh >/dev/null 2>&1; then
  echo "no gh available; skipping report"; exit 0
fi
# A `push` event carries no pull-request number, and on a `pull_request` event
# GITHUB_SHA is GitHub's synthetic merge commit rather than this branch's head.
# Both bit us: the comment stopped updating on push, and when it did update it
# named a SHA that exists in no branch — so a green comment could describe a tree
# that was never built. Take the real head and the PR from the event payload,
# falling back to a branch lookup for pushes.
read -r PR SHA <<<"$(python3 - <<'PYEOF'
import json, os
try:
    d = json.load(open(os.environ["GITHUB_EVENT_PATH"]))
except Exception:
    d = {}
pr = d.get("pull_request") or {}
num = d.get("pull_request", {}).get("number") or (d.get("issue") or {}).get("number") or ""
sha = (pr.get("head") or {}).get("sha") or os.environ.get("GITHUB_SHA", "")
print(num or "-", sha)
PYEOF
)"
[ "$PR" = "-" ] && PR=""
if [ -z "$PR" ] && [ -n "${GITHUB_REF_NAME:-}" ] && command -v gh >/dev/null 2>&1; then
  PR=$(gh pr list --repo "${GITHUB_REPOSITORY:-}" --head "$GITHUB_REF_NAME" --state open \
        --json number --jq '.[0].number' 2>/dev/null)
fi
if [ -z "$PR" ]; then echo "no open PR for ${GITHUB_REF_NAME:-this ref}; skipping comment"; exit 0; fi
body_file=$(mktemp)
{
  echo "<!-- fuss-ci:$marker -->"
  echo "### $title — $status"
  echo ""
  if [ -n "$SHA" ]; then
    echo "commit \`${SHA:0:8}\` · run [${GITHUB_RUN_ID:-?}](/${GITHUB_REPOSITORY:-}/actions/runs/${GITHUB_RUN_ID:-})"
    echo ""
  fi
  if [ -f "$log" ]; then
    # GitHub comments cap at 65536 chars and the useful lines are the first
    # errors, not the tail. Strip ANSI, then print:
    #   * stage markers and failed stages,
    #   * up to 3 errors per source file (deduplicated by message),
    #   * the tail for context.
    clean=$(mktemp)
    sed -E 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$log" > "$clean"
    echo '```'
    grep -E '^(=====|!! FAILED|BUILD |FAILURE)' "$clean" | head -n 40
    echo ''
    echo '--- errors (max 3 per file) ---'
    # Diagnostics: Kotlin uses `e: file:…`, Rust uses `error: …` followed by
    # `  --> file:line`. Print each diagnostic plus the following context lines
    # so the location is never lost. Cap per-file messages to keep it readable.
    awk '
      function flush() {
        for (i = 0; i < nctx; i++) print ctx[i]
        nctx = 0
      }
      /^e: / || /^(error|warning)(\[[a-z:_-]+\])?: / || /^BUILD |^FAILURE|What went wrong|^> [A-Za-z]|^!! FAILED|^===== / {
        flush()
        line = $0
        # de-duplicate Kotlin cascades per file
        if (line ~ /^e: /) {
          key = line
          sub(/:[0-9]+:[0-9]+ .*/, "", key)
          msg = line
          sub(/^e: file:[^ ]*:[0-9]+:[0-9]+ /, "", msg)
          k = key "|" msg
          if (seen[k]++ > 0) next
        }
        print
        armed = 1
        next
      }
      armed && nctx < 6 { ctx[nctx++] = $0 }
      END { flush() }
    ' "$clean" | head -n 260
    echo ''
    echo '--- last 60 lines ---'
    # Drop the benchmark table from the tail; it is printed in its own section
    # below, and duplicating thirteen rows of numbers helps nobody.
    tail -n 60 "$clean" | grep -vE ' +[0-9.]+ms +[0-9.]+ms +[0-9]+$|^operation +median|^-{20,}$'
    echo '```'
    # Benchmarks are reported, never auto-committed: on a shared runner the same
    # binary measures 2.6x apart between runs, so a committed sheet either churns
    # on every push or has to ignore real regressions. The sheet in the repo is a
    # deliberate measurement (scripts/bench-local.sh); this is each run's.
    if grep -q '^operation .*median' "$clean"; then
      echo ''
      echo '#### benchmarks from this run'
      echo ''
      echo '```'
      awk '/^operation[[:space:]]+median/ {on=1} on {print} /^VmRSS:/ {on=0}' "$clean"
      echo '```'
    fi
    rm -f "$clean"
  else
    echo "(no log produced)"
  fi
} > "$body_file"
existing=$(gh api "repos/$GITHUB_REPOSITORY/issues/$PR/comments" --paginate \
  --jq ".[] | select(.body | startswith(\"<!-- fuss-ci:$marker -->\")) | .id" 2>/dev/null | head -1)
if [ -n "$existing" ]; then
  gh api -X PATCH "repos/$GITHUB_REPOSITORY/issues/comments/$existing" -F body=@"$body_file" >/dev/null \
    && echo "updated comment $existing (commit ${SHA:0:8})"
else
  gh api -X POST "repos/$GITHUB_REPOSITORY/issues/$PR/comments" -F body=@"$body_file" >/dev/null \
    && echo "created comment (commit ${SHA:0:8})"
fi
