#!/usr/bin/env bash
# Builds the approval page, the daemon as a JNI library for arm64 phones and x86_64 emulators,
# runs the app's unit tests, and builds the debug APK.
set -euo pipefail
cd "$(dirname "$0")/.."
npm --prefix web ci --silent
# The library links everything it uses statically. It builds with the NDK's compilers, as
# cargo-ndk sets them out, and links with the NDK's clang for each target. Cargo runs from the
# filesystem root, so a workspace around this checkout that patches dependencies in its Cargo
# configuration cannot rewrite the lockfile this build is locked to.
manifest="$PWD/Cargo.toml"
declare -A triple=([arm64-v8a]=aarch64-linux-android [x86_64]=x86_64-linux-android)
for abi in arm64-v8a x86_64; do
  target="${triple[$abi]}"
  (
    source <(cargo ndk-env -t "$abi")
    export "CARGO_TARGET_$(tr 'a-z-' 'A-Z_' <<<"$target")_LINKER=$(dirname "$CLANG_PATH")/${target}21-clang"
    cd / && cargo build --manifest-path "$manifest" -p erisauth-android --target "$target" --release --locked
  )
  mkdir -p "android/app/src/main/jniLibs/$abi"
  rm -f "android/app/src/main/jniLibs/$abi"/*.so
  cp "target/$target/release/liberisauth_android.so" "android/app/src/main/jniLibs/$abi/"
done
./android/gradlew -p android --no-daemon testDebugUnitTest assembleDebug "$@"
echo "android/app/build/outputs/apk/debug/app-debug.apk"
