#!/usr/bin/env bash
# Builds the `peil` binary for arm64 Android and places it where the app
# packages it as a native library, libsensio.so (ADR-0004 section 4).
# Needs ANDROID_SDK_ROOT (or ANDROID_HOME) with an NDK, and the Rust target
# aarch64-linux-android.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/.." && pwd)"
sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
[ -n "$sdk" ] || { echo "set ANDROID_SDK_ROOT to the Android SDK" >&2; exit 1; }
# On Windows, a path cargo hands to the linker must be a Windows path.
if command -v cygpath >/dev/null; then sdk="$(cygpath -m "$sdk")"; fi
ndk="$(ls -d "$sdk"/ndk/*/ | sort -V | tail -1)"
api=31
case "$(uname -s)" in
    Linux) host=linux-x86_64; ext="" ;;
    Darwin) host=darwin-x86_64; ext="" ;;
    *) host=windows-x86_64; ext=".cmd" ;;
esac
clang="$ndk/toolchains/llvm/prebuilt/$host/bin/aarch64-linux-android${api}-clang$ext"
[ -f "$clang" ] || { echo "no NDK clang at $clang" >&2; exit 1; }

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$clang"
# 16 KB pages: Android 15 and later devices may use them, and the packager
# refuses misaligned libraries. Added to RUSTFLAGS, which overrides the
# per-target setting; with --target it reaches only the target's code.
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384"

cd "$repo"
cargo build -p sensio-cli --release --locked --target aarch64-linux-android
out="$here/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$out"
cp target/aarch64-linux-android/release/peil "$out/libsensio.so"
ls -l "$out/libsensio.so"
