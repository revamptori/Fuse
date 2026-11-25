#!/usr/bin/env bash
# Upsert a CI status comment on the current PR with the tail of the log file.
# Usage: ci-report.sh <marker> <title> <logfile> [status]
set -uo pipefail
marker="$1"; title="$2"; log="$3"; status="${4:-unknown}"
if [ -z "${GH_TOKEN:-}" ] && ! command -v gh >/dev/null 2>&1; then
  echo "no gh available; skipping report"; exit 0
fi
PR=$(python3 -c 'import json,os; d=json.load(open(os.environ["GITHUB_EVENT_PATH"])); print(d.get("pull_request",{}).get("number") or (d.get("issue") or {}).get("number") or "")' 2>/dev/null)
if [ -z "$PR" ]; then echo "no PR number in event; skipping comment"; exit 0; fi
body_file=$(mktemp)
{
  echo "<!-- fuss-ci:$marker -->"
  echo "### $title — $status"
  echo ""
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
    tail -n 60 "$clean"
    echo '```'
    rm -f "$clean"
  else
    echo "(no log produced)"
  fi
} > "$body_file"
existing=$(gh api "repos/$GITHUB_REPOSITORY/issues/$PR/comments" --paginate \
  --jq ".[] | select(.body | startswith(\"<!-- fuss-ci:$marker -->\")) | .id" 2>/dev/null | head -1)
if [ -n "$existing" ]; then
  gh api -X PATCH "repos/$GITHUB_REPOSITORY/issues/comments/$existing" -F body=@"$body_file" >/dev/null && echo "updated comment $existing"
else
  gh api -X POST "repos/$GITHUB_REPOSITORY/issues/$PR/comments" -F body=@"$body_file" >/dev/null && echo "created comment"
fi
