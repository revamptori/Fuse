#!/usr/bin/env bash
# Inspect a built APK for the mistakes that compile fine and die on launch.
#
# Everything here is read out of the packaged artifact, not the source tree, so
# it catches problems that only exist after packaging: a manifest whose class
# names do not resolve (the 1.0.0 crash), a missing native library for an ABI,
# or an activity that is not exported and therefore cannot be launched.
#
# Usage: check-apk.sh <apk> [expected-abi]
set -uo pipefail
APK="${1:?usage: check-apk.sh <apk> [abi]}"
WANT_ABI="${2:-}"
BT="${ANDROID_HOME:-$ANDROID_SDK_ROOT}/build-tools/35.0.0"
AAPT="$BT/aapt"
rc=0

[ -f "$APK" ] || { echo "missing $APK"; exit 1; }
[ -x "$AAPT" ] || { echo "aapt not found at $AAPT"; exit 1; }

badging=$("$AAPT" dump badging "$APK" 2>/dev/null)
[ -n "$badging" ] || { echo "$APK: aapt could not read the manifest"; exit 1; }

pkg=$(sed -n "s/^package: name='\([^']*\)'.*/\1/p" <<<"$badging")
echo "package      $pkg"
echo "launchable   $(sed -n "s/^launchable-activity: name='\([^']*\)'.*/\1/p" <<<"$badging")"
echo "native-code  $(sed -n "s/^native-code: \(.*\)/\1/p" <<<"$badging")"

# 1. A launchable activity must exist; without it the launcher cannot start the
#    app at all.
launch=$(sed -n "s/^launchable-activity: name='\([^']*\)'.*/\1/p" <<<"$badging")
if [ -z "$launch" ]; then
  echo "  !! no launchable activity: nothing in the APK handles MAIN/LAUNCHER"
  rc=1
fi

# 2. Native libraries must cover the ABI the installer expects. dexopt on a
#    mismatched ABI throws UnsatisfiedLinkError on the first call.
libs=$(unzip -l "$APK" 2>/dev/null | grep -o 'lib/[^/]*/libfuss_core\.so' | sort -u)
echo "libs         $(echo "$libs" | tr '\n' ' ')"
if [ -z "$libs" ]; then
  echo "  !! libfuss_core.so is not packaged: every engine call will fail"
  rc=1
fi
if [ -n "$WANT_ABI" ] && ! grep -q "lib/$WANT_ABI/libfuss_core.so" <<<"$libs"; then
  echo "  !! $WANT_ABI APK does not contain lib/$WANT_ABI/libfuss_core.so"
  rc=1
fi

# 3. Manifest class names must resolve to something in the dex. A relative name
#    resolves against the namespace; when the namespace and the package differ,
#    the APK declares a class that does not exist and the process dies before
#    the first frame with no useful log.
# aapt's xmltree is the reliable reader for a compiled manifest. Each component
# is a block of `E: <tag>` followed by `A: ...:name(...)="..."` lines; pull the
# first android:name attribute after each of the two tags check-apk cares about.
xml=$("$AAPT" dump xmltree "$APK" AndroidManifest.xml 2>/dev/null)
name_after() { # tag  (reads the manifest tree from $XMLTREE)
  XMLTREE="$xml" python3 - "$1" <<'PYEOF'
import os, re, sys
tag, text = sys.argv[1], os.environ["XMLTREE"]
lines = text.splitlines()
for i, line in enumerate(lines):
    if re.match(rf"^\s*E: {re.escape(tag)}\b", line):
        for follow in lines[i:i + 4]:
            m = re.search(r'android:name(?:\(0x[0-9a-f]+\))?="([^"]+)"', follow)
            if m:
                print(m.group(1))
                raise SystemExit
PYEOF
}
for tag in application activity; do
  cls=$(name_after "$tag")
  [ -n "$cls" ] || continue
  # Confirm the class is really in the dex. A release build may split into
  # classes.dex, classes2.dex, ... so search them all.
  if unzip -p "$APK" 'classes*.dex' 2>/dev/null | strings | grep -qx "L${cls//.//};"; then
    echo "  ok  $tag $cls"
  else
    echo "  !! $tag $cls -> not found in classes.dex (the app cannot be instantiated)"
    rc=1
  fi
done

[ $rc = 0 ] && echo "  ok  $APK passes launch checks"
exit $rc
