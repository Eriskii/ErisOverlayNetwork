#!/usr/bin/env bash
# Builds the approval page, the daemon as a JNI library for arm64 phones and x86_64 emulators,
# runs the app's unit tests, and builds the debug APK.
set -euo pipefail
cd "$(dirname "$0")/.."
npm --prefix web ci --silent
native="$(mktemp -d)"
trap 'rm -rf "$native"' EXIT
cargo ndk -t arm64-v8a -t x86_64 -o "$native" build -p erisauth-android --release
# The library links everything it uses statically; the other libraries cargo-ndk collects are
# dependencies' own cdylibs, which nothing loads.
for abi in arm64-v8a x86_64; do
  mkdir -p "android/app/src/main/jniLibs/$abi"
  rm -f "android/app/src/main/jniLibs/$abi"/*.so
  cp "$native/$abi/liberisauth_android.so" "android/app/src/main/jniLibs/$abi/"
done
./android/gradlew -p android --no-daemon testDebugUnitTest assembleDebug "$@"
echo "android/app/build/outputs/apk/debug/app-debug.apk"
