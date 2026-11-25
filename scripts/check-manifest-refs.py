#!/usr/bin/env python3
"""Check that every class the manifest names actually exists.

Gradle resolves a short name like ".MainActivity" against the *namespace*
(`cloud.fuss`), not against wherever the source file happens to live. When those
two disagree the app still compiles and the APK is still produced and signed, so
CI stays green; the failure only appears on a device, where the process dies
before the first frame and the launcher reports "keeps stopping".

That is exactly what happened: the classes are in `cloud.fuss.app` while the
namespace is `cloud.fuss`. This check turns that into a build failure.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "android/app/src/main/AndroidManifest.xml"
GRADLE = ROOT / "android/app/build.gradle.kts"
SRC = ROOT / "android/app/src/main/java"

errors = []

if not MANIFEST.exists():
    sys.exit(f"manifest not found: {MANIFEST}")

manifest = MANIFEST.read_text()
gradle = GRADLE.read_text() if GRADLE.exists() else ""

m = re.search(r'namespace\s*=\s*"([^"]+)"', gradle)
if not m:
    sys.exit("could not read `namespace` from build.gradle.kts")
namespace = m.group(1)

app_id = re.search(r'applicationId\s*=\s*"([^"]+)"', gradle)
app_id = app_id.group(1) if app_id else namespace

# Every class-valued manifest attribute must name a class that exists.
#   android:name on <application>, <activity>, <service>, <receiver>, <provider>
#   android:value on <meta-data> is arbitrary data, so it is ignored.
for attr in re.finditer(r'android:(name|targetActivity|parentActivityName)="([^"]+)"', manifest):
    name = attr.group(2)
    if name.startswith("@") or name.startswith("android.") or name.startswith("androidx."):
        continue
    if "${" in name:
        continue
    if name.startswith("."):
        fqcn = namespace + name
    elif "." not in name:
        fqcn = f"{namespace}.{name}"
    else:
        fqcn = name
    # Only component declarations matter here; permissions and features are
    # plain strings such as "android.permission.READ_EXTERNAL_STORAGE".
    if fqcn.startswith(("android.", "androidx.", "android.permission")):
        continue
    parts = fqcn.split(".")
    # The class file is the last component; a nested class uses '$'.
    simple = parts[-1]
    has_class_file = False
    # Walk every source root looking for a declaration of this FQCN.
    for kt in SRC.rglob("*.kt"):
        text = kt.read_text(errors="replace")
        pkg = re.search(r"^package\s+([\w.]+)", text, re.M)
        pkg_name = pkg.group(1) if pkg else ""
        if pkg_name != ".".join(parts[:-1]):
            continue
        if re.search(rf"^(?:[\w@]+\s+)*class\s+{re.escape(simple)}\b", text, re.M):
            has_class_file = True
            break
    # Java sources too, in case any are added later.
    if not has_class_file:
        for java in SRC.rglob("*.java"):
            text = java.read_text(errors="replace")
            pkg = re.search(r"^package\s+([\w.]+)", text, re.M)
            pkg_name = pkg.group(1) if pkg else ""
            if pkg_name == ".".join(parts[:-1]) and re.search(
                rf"\bclass\s+{re.escape(simple)}\b", text
            ):
                has_class_file = True
                break
    if not has_class_file:
        hint = ""
        if name.startswith("."):
            hint = (
                f"  (the leading dot means `{namespace}` + `{name}`, "
                f"i.e. {fqcn} — write the full name if the class lives elsewhere)"
            )
        errors.append(f"manifest names {name} -> {fqcn}, but no such class in {SRC}{hint}")

if errors:
    print("manifest references classes that do not exist:")
    for e in errors:
        print("  -", e)
    sys.exit(1)

print(f"manifest component names resolve (namespace {namespace}, applicationId {app_id})")
