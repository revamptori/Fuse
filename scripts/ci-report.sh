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
    echo '```'
    # Keep the tail; GitHub comments cap at 65536 chars.
    tail -c 50000 "$log"
    echo '```'
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
