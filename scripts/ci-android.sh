#!/usr/bin/env bash
# CI entry point for the Android half: build the native libs for every ABI,
# then assemble debug + release APKs. All output is tee'd to ci-android.log for
# the PR reporter.
set -uo pipefail
LOG="${LOG:-ci-android.log}"
: > "$LOG"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
fail=0
step() {
  local name="$1"; shift
  echo "===== $name =====" >> "$LOG"
  if ! ( "$@" ) >> "$LOG" 2>&1; then
    echo "!! FAILED: $name" >> "$LOG"
    fail=1
  fi
}
python3 "$HERE/scripts/check-kotlin-imports.py" >> "$LOG" 2>&1 || { echo "!! FAILED: kotlin pre-flight" >> "$LOG"; fail=1; }
step "rust android cross-compile" bash "$HERE/scripts/build-rust-android.sh"
step "gradle assembleDebug" bash -c "cd '$HERE/android' && ./gradlew --no-daemon --stacktrace assembleDebug"
step "gradle assembleRelease" bash -c "cd '$HERE/android' && ./gradlew --no-daemon --stacktrace assembleRelease"
echo "===== artefacts =====" >> "$LOG"
# Print sizes in bytes next to a human figure so the <5 MB goal can be checked
# from the CI log alone.
find "$HERE/android/app/build/outputs/apk" -name '*.apk' -printf '%s %p\n' 2>/dev/null \
  | while read -r size path; do
      printf '%8d bytes  %s\n' "$size" "$(basename "$path")"
      case "$path" in *release*) echo "  ^ release artifact" ;; esac
    done >> "$LOG" 2>&1
echo "===== unit tests (kotlin) =====" >> "$LOG"
( cd "$HERE/android" && ./gradlew --no-daemon testDebugUnitTest ) >> "$LOG" 2>&1 || { echo "!! FAILED: kotlin unit tests" >> "$LOG"; fail=1; }
echo "===== result: $([ $fail = 0 ] && echo PASS || echo FAIL) =====" >> "$LOG"
cp "$LOG" "$HERE/ci-android.log" 2>/dev/null || true
exit $fail
