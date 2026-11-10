#!/usr/bin/env bash
# Compiles the launcher and its tests, builds the native engine, then runs the
# Java test suite (which includes real JNI integration tests).
#
# Usage: scripts/test-java.sh [--skip-native]
#
# Environment: JAVA_HOME (JDK 17+), CARGO (default: cargo)

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

skip_native=0
if [ "${1:-}" = "--skip-native" ]; then
    skip_native=1
fi

jdk_home="${JAVA_HOME:?JAVA_HOME must point at a JDK}"
java_bin="$jdk_home/bin/java"
javac_bin="$jdk_home/bin/javac"
cargo_bin="${CARGO:-cargo}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "-- native library"
lib="$repo_root/target/release/libzwrs_jni.so"
case "$(uname -s)" in
    Darwin*) lib="$repo_root/target/release/libzwrs_jni.dylib" ;;
esac
if [ "$skip_native" -eq 0 ]; then
    "$cargo_bin" build --release --locked -p zwrs-jni
fi
if [ ! -f "$lib" ]; then
    echo "native library not found at $lib (run without --skip-native)" >&2
    exit 1
fi

echo "-- compiling launcher"
mkdir -p "$work/classes"
find java/src/main/java -name '*.java' > "$work/main-sources"
"$javac_bin" -encoding UTF-8 -Xlint:all -Werror -d "$work/classes" @"$work/main-sources"

echo "-- compiling tests"
mkdir -p "$work/test-classes"
find java/src/test/java -name '*.java' > "$work/test-sources"
"$javac_bin" -encoding UTF-8 -cp "$work/classes" -d "$work/test-classes" @"$work/test-sources"

echo "-- running tests"
"$java_bin" -cp "$work/classes:$work/test-classes" \
    -Dzwrs.native.path="$lib" \
    io.zorowarrior.zwrs.test.TestRunner "$@"
