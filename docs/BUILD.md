# Building Fuss

## What you need

| Tool | Version used by CI | Notes |
| --- | --- | --- |
| Rust | stable (2025) | `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android` |
| Android SDK | platform 35, build-tools 35.0.0 | via Android Studio or `sdkmanager` |
| Android NDK | 27.2.12479018 | the exact version Gradle expects |
| JDK | 17 | AGP 8.7 requires 17+ |
| Gradle | 8.11.1 | the wrapper downloads it; no install needed |

`ANDROID_HOME` (or `ANDROID_SDK_ROOT`) must point at the SDK; the build scripts
find the NDK under `$ANDROID_HOME/ndk/27.2.12479018` or `ANDROID_NDK_HOME`.

## One command

```bash
./scripts/build-rust-android.sh   # native engine for every ABI
cd android && ./gradlew assembleDebug
```

Installs to a connected device with `./gradlew installDebug`.

The Gradle build already depends on the Rust step (`cargoBuildAndroid` →
`preBuild`), so plain `./gradlew assembleRelease` is enough after the first
native cross-compile. Set `FUSS_SKIP_RUST=1` to skip it while iterating on Kotlin.

### What the Rust step produces

For each ABI in `arm64-v8a`, `armeabi-v7a`, `x86_64`:

```
android/app/src/main/jniLibs/<abi>/libfuss_core.so
```

It uses `cargo build -p fuss-jni --release --target <triple>` with the NDK's
clang as linker, then runs `llvm-strip` on the result. Nothing else is copied.

## Flavours and outputs

```
android/app/build/outputs/apk/debug/   app-debug-<abi>.apk, app-debug-universal.apk
android/app/build/outputs/apk/release/ app-release-<abi>.apk, ...
```

Debug builds carry the `.debug` application id suffix so they can be installed
next to a release build.

## Signing a release

Without configuration, release APKs are left unsigned and CI ships debug-signed
APKs for sideloading. To sign properly, provide either environment variables or
`~/.fuss-keystore.properties`:

```properties
FUSS_STORE_FILE=/absolute/path/release.jks
FUSS_STORE_PASSWORD=…
FUSS_KEY_ALIAS=fuss
FUSS_KEY_PASSWORD=…
```

Generate a key with:

```bash
keytool -genkeypair -v -keystore release.jks -alias fuss \
  -keyalg RSA -keysize 4096 -validity 10000
```

The same four values can be set as environment variables in CI.

## Reproducible / offline builds

Normal builds use crates.io. For a network-restricted or audit-friendly build,
the dependency sources can be vendored:

```bash
./scripts/vendor-deps.sh                 # fetch exact versions into rust/vendor
cp rust/.cargo/config-vendor.toml rust/.cargo/config.toml
cd rust && cargo build --workspace --offline
```

`vendor-deps.sh` writes each crate's `package` checksum from `Cargo.lock` into
`.cargo-checksum.json`, which cargo requires before it will trust a vendored
source. Remove `rust/.cargo/config.toml` to go back to crates.io.

The Gradle build is reproducible to the extent its dependencies are pinned: AGP,
Kotlin and the Compose BOM are exact versions in the build files, and the Gradle
wrapper version is fixed in `gradle-wrapper.properties`.

## CI

* `.github/workflows/ci.yml` — Rust: fmt, clippy `-D warnings`, debug and release
  tests, benchmark; posts a diagnostics comment on the pull request.
* `.github/workflows/android.yml` — cross-compiles the engine, assembles debug
  and release APKs for all ABIs, runs the Kotlin unit tests, uploads the APKs and
  posts the same kind of diagnostics comment.
* `.github/workflows/release.yml` — on a `v*` tag, builds the APKs, writes
  `SHA256SUMS.txt`, and attaches everything to the GitHub release.

## Troubleshooting

* **`error: linker 'aarch64-linux-android-clang' not found`** — the NDK is
  missing or `ANDROID_NDK_HOME` is wrong. Install NDK 27.2.12479018 exactly.
* **`Unresolved reference: compose` everywhere** — the Compose BOM or plugin is
  missing from `android/app/build.gradle.kts`; check that the `dependencies`
  block is intact.
* **`checksum for … could not be calculated`** — you are building offline from a
  stale vendor directory. Re-run `./scripts/vendor-deps.sh`.
* **JDK version errors** — `./gradlew -version` and `java -version` must both
  report 17 or newer.
