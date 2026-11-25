#!/usr/bin/env python3
"""Catch missing Compose imports before a four-minute CI/Gradle round trip.

Kotlin's compiler reports these as a cascade of "Unresolved reference", so this
performs the obvious checks instead:

  * `Modifier.<name>(` requires `import androidx.compose.foundation.layout.<name>`
    (or the matching foundation/ui import) unless the file defines its own
    extension of that name;
  * `Icons.<Style>` requires `import androidx.compose.material.icons.<style>.<Icon>`
    (or `material-icons-core`'s `filled` shorthand) for every icon used;
  * every `@Composable` file that references `ExperimentalMaterial3Api` must
    annotate the function(s) that call experimental APIs.

Exit status 1 with a readable list when something is missing.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / "android/app/src/main/java"
LAYOUT = "androidx.compose.foundation.layout"
# Modifiers from foundation/ui that are not under foundation.layout
OTHER_MODIFIERS = {
    "background", "border", "clickable", "combinedClickable", "clip", "alpha",
    "drawBehind", "pointerInput", "focusable", "weight", "fillParentMaxSize",
    "animateContentSize", "verticalScroll", "horizontalScroll", "systemBarsPadding",
    "statusBarsPadding", "navigationBarsPadding", "imePadding", "safeDrawingPadding",
    "windowInsetsPadding", "scrollable", "testTag", "graphicsLayer", "zIndex",
    # Scoped members of RowScope/ColumnScope/BoxScope: resolved by the receiver,
    # never imported.
    "weight", "align", "matchParentSize", "fillParentMaxWidth",
}
FOUNDATION_PKG = "androidx.compose.foundation"
UI_PKG = "androidx.compose.ui"
MATERIAL3 = "androidx.compose.material3"

# Scoped members of RowScope/ColumnScope/BoxScope: resolved by the receiver, so
# they are never imported and must not be flagged.
SCOPED = {"weight", "align", "matchParentSize", "fillParentMaxWidth"}

problems = []


def check_file(path: pathlib.Path) -> None:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    imports = [l for l in lines if l.startswith("import ")]

    def has(suffix: str) -> bool:
        return any(i.split("import ", 1)[1].strip() == suffix for i in imports)

    # local extension definitions like `private fun Modifier.foo(...)`
    defined = set(re.findall(r"fun\s+Modifier\.(\w+)\s*\(", text))
    used = set(re.findall(r"Modifier\s*\n?\s*\.\s*(\w+)\s*[({]", text))
    used |= set(re.findall(r"\.(\w+)\s*\(", text)) & {
        "fillMaxWidth", "fillMaxSize", "fillMaxHeight", "height", "width", "size",
        "padding", "aspectRatio", "weight", "heightIn", "widthIn", "sizeIn",
        "offset", "wrapContentWidth", "wrapContentHeight", "defaultMinSize",
    }
    for name in sorted(used - defined):
        if name in SCOPED:
            continue
        if name in OTHER_MODIFIERS:
            ok = (
                has(f"{FOUNDATION_PKG}.{name}")
                or has(f"{UI_PKG}.{name}")
                or has(f"{UI_PKG}.draw.{name}")
                or has(f"{FOUNDATION_PKG}.shape.{name}")
            )
            if not ok:
                problems.append(f"{path.name}: Modifier.{name}() used but '{FOUNDATION_PKG}.{name}' not imported")
        else:
            ok = has(f"{LAYOUT}.{name}")
            # Some files import layout.* wholesale
            if not ok:
                problems.append(f"{path.name}: Modifier.{name}() used but '{LAYOUT}.{name}' not imported")

    # icons: Icons.Outlined.Foo / Icons.AutoMirrored.Outlined.Foo
    for style in ("Outlined", "Filled", "Rounded", "Sharp", "TwoTone"):
        for icon in set(re.findall(r"Icons(?:\.[A-Za-z]+)?\." + style + r"\.(\w+)", text)):
            candidates = [
                f"androidx.compose.material.icons.{style.lower()}.{icon}",
                f"androidx.compose.material.icons.automirrored.{style.lower()}.{icon}",
            ]
            if not any(has(c) for c in candidates):
                # `filled` icons are often imported as `androidx.compose.material.icons.filled.X`
                problems.append(f"{path.name}: Icons.{style}.{icon} used but no matching icon import")

    # experimental material3 usage without opt-in
    exp = re.compile(r"\b(TopAppBar|Scaffold|SegmentedButton|SingleChoiceSegmentedButtonRow|ExposedDropdownMenuBox)\s*\(")
    if exp.search(text) and "ExperimentalMaterial3Api" in text and "@OptIn(ExperimentalMaterial3Api::class" not in text:
        problems.append(f"{path.name}: calls experimental M3 APIs but has no @OptIn(ExperimentalMaterial3Api::class)")


def main() -> int:
    files = sorted(ROOT.rglob("*.kt"))
    for f in files:
        check_file(f)
    if problems:
        print("Kotlin pre-flight problems:\n")
        for p in problems:
            print("  -", p)
        print(f"\n{len(problems)} problem(s) in {len(files)} files")
        return 1
    print(f"kotlin pre-flight: {len(files)} files, looking good")
    return 0


if __name__ == "__main__":
    sys.exit(main())
