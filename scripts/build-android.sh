#!/usr/bin/env bash
# Builds the approval page, the daemon as a JNI library for arm64 phones and x86_64 emulators,
# runs the app's unit tests, and builds the debug APK. Needs the Android SDK and NDK (ANDROID_HOME,
# Android Studio's ~/Android/Sdk when unset), cargo-ndk, and the aarch64-linux-android and
# x86_64-linux-android Rust targets.
set -euo pipefail
cd "$(dirname "$0")/.."
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
npm --prefix eon/web ci --silent
# The library links everything it uses statically. It builds with the NDK's compilers, as
# cargo-ndk sets them out, and links with the NDK's clang for each target. Cargo runs in this
# checkout, so the Cargo configuration around it, the Eris monorepo's, applies; its patches for
# crates this workspace does not use land in the lockfile in no fixed order, so the build may
# rewrite it and is not run --locked.
declare -A triple=([arm64-v8a]=aarch64-linux-android [x86_64]=x86_64-linux-android)
for abi in arm64-v8a x86_64; do
  target="${triple[$abi]}"
  (
    source <(cargo ndk-env -t "$abi")
    export "CARGO_TARGET_$(tr 'a-z-' 'A-Z_' <<<"$target")_LINKER=$(dirname "$CLANG_PATH")/${target}21-clang"
    cargo build -p eon-android --target "$target" --release
  )
  mkdir -p "android/app/src/main/jniLibs/$abi"
  rm -f "android/app/src/main/jniLibs/$abi"/*.so
  cp "target/$target/release/libeon_android.so" "android/app/src/main/jniLibs/$abi/"
done
./android/gradlew -p android --no-daemon testDebugUnitTest assembleDebug "$@"
echo "android/app/build/outputs/apk/debug/app-debug.apk"
