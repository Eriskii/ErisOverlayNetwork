#!/usr/bin/env bash
# Builds the approval page, the daemon as a JNI library for arm64 phones and x86_64 emulators,
# runs the app's unit tests, and builds the debug APK. Needs the Android SDK and NDK (ANDROID_HOME,
# where Android Studio puts them when unset), cargo-ndk, and the aarch64-linux-android and
# x86_64-linux-android Rust targets. It runs on the bash macOS ships, 3.2.
set -euo pipefail
cd "$(dirname "$0")/.."
checkout="$PWD"
if [[ "$(uname)" == Darwin ]]; then studio_sdk="$HOME/Library/Android/sdk"; else studio_sdk="$HOME/Android/Sdk"; fi
export ANDROID_HOME="${ANDROID_HOME:-$studio_sdk}"
npm --prefix eon/web ci --silent
# The library links everything it uses statically. It builds with the NDK's compilers, as
# cargo-ndk sets them out, and links with the NDK's clang for each target. Cargo runs from the
# filesystem root, where the Cargo configuration of a workspace around this checkout, the Eris
# monorepo's, does not apply: its patches for crates this workspace does not use would rewrite
# the lockfile.
for pair in arm64-v8a:aarch64-linux-android x86_64:x86_64-linux-android; do
  abi="${pair%%:*}"
  target="${pair#*:}"
  (
    ndk_env="$(cargo ndk-env -t "$abi")"
    eval "$ndk_env"
    export "CARGO_TARGET_$(tr 'a-z-' 'A-Z_' <<<"$target")_LINKER=$(dirname "$CLANG_PATH")/${target}21-clang"
    cd /
    cargo build --locked --manifest-path "$checkout/Cargo.toml" -p eon-android --target "$target" --release
  )
  mkdir -p "android/app/src/main/jniLibs/$abi"
  rm -f "android/app/src/main/jniLibs/$abi"/*.so
  cp "target/$target/release/libeon_android.so" "android/app/src/main/jniLibs/$abi/"
done
./android/gradlew -p android --no-daemon testDebugUnitTest assembleDebug "$@"
echo "android/app/build/outputs/apk/debug/app-debug.apk"
