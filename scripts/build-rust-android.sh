#!/usr/bin/env bash
# Cross-compile the Rust engine for every Android ABI and copy the shared
# libraries into the app's jniLibs directory.
#
# Requirements: Rust (stable) with the Android targets, Android NDK r27.
# Env: ANDROID_NDK_HOME or ANDROID_HOME (NDK version 27.2.12479018 by default).
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
RUST_DIR="$HERE/rust"
OUT_DIR="${1:-$HERE/android/app/src/main/jniLibs}"
NDK_VERSION="${NDK_VERSION:-27.2.12479018}"

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
  ANDROID_NDK_HOME="$SDK/ndk/$NDK_VERSION"
fi
if [ ! -d "$ANDROID_NDK_HOME" ]; then
  echo "error: Android NDK not found at $ANDROID_NDK_HOME" >&2
  echo "set ANDROID_NDK_HOME, or install NDK $NDK_VERSION with sdkmanager" >&2
  exit 1
fi

HOST_TAG="linux-x86_64"
case "$(uname -s)" in
  Darwin) HOST_TAG="darwin-x86_64" ;;
esac
TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG"
MIN_API="${MIN_API:-30}"
API_LEVEL="$MIN_API"

build_abi() { # abi rust-target clang-prefix
  local abi="$1" target="$2" prefix="$3"
  local cc="$TOOLCHAIN/bin/${prefix}${API_LEVEL}-clang"
  local ar="$TOOLCHAIN/bin/llvm-ar"
  local env_target
  env_target="$(echo "$target" | tr 'a-z-' 'A-Z_')"
  echo "==> $abi ($target)"
  (
    cd "$RUST_DIR"
    env \
      "CARGO_TARGET_${env_target}_LINKER=$cc" \
      "CC_${target//-/_}=$cc" \
      "AR_${target//-/_}=$ar" \
      RUSTFLAGS="-C strip=symbols" \
      cargo build --release --target "$target" -p fuss-jni
  )
  mkdir -p "$OUT_DIR/$abi"
  cp "$RUST_DIR/target/$target/release/libfuss_core.so" "$OUT_DIR/$abi/libfuss_core.so"
  "$TOOLCHAIN/bin/llvm-strip" --strip-unneeded "$OUT_DIR/$abi/libfuss_core.so" 2>/dev/null || true
  ls -la "$OUT_DIR/$abi/libfuss_core.so"
}

mkdir -p "$OUT_DIR"
build_abi arm64-v8a aarch64-linux-android aarch64-linux-android
build_abi armeabi-v7a armv7-linux-androideabi armv7a-linux-androideabi
build_abi x86_64 x86_64-linux-android x86_64-linux-android
echo "native libraries written to $OUT_DIR"
