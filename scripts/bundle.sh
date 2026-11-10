#!/usr/bin/env bash
# Builds the user-facing distribution:
#
#   ZOROWARRIOR-Rust-Server.jar
#   native/<os>-<arch>/libzwrs_jni.so
#   plugins/README.txt
#   config/, data/, logs/          (created on first start)
#   SHA256SUMS                     (checksums of everything shipped)
#
# The Rust engine inside `native/` is what runs the Minecraft server; the JAR is
# the launcher that loads it. Both artifacts of one run are always packaged
# together so their versions cannot drift apart.
#
# Usage:
#   scripts/bundle.sh [--output DIR] [--target TRIPLE] [--skip-rust] [--skip-java]
#
# Environment:
#   JAVA_HOME   JDK used to compile and jar the launcher (required unless --skip-java)
#   CARGO       cargo binary (default: cargo)
#   ZWRS_VERSION  overrides the version written into metadata (default: from Cargo.toml)

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

output="$repo_root/dist"
target=""
skip_rust=0
skip_java=0

while [ $# -gt 0 ]; do
    case "$1" in
        --output) output="$2"; shift 2 ;;
        --output=*) output="${1#*=}"; shift ;;
        --target) target="$2"; shift 2 ;;
        --target=*) target="${1#*=}"; shift ;;
        --skip-rust) skip_rust=1; shift ;;
        --skip-java) skip_java=1; shift ;;
        -h|--help)
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

version="${ZWRS_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
if [ -z "$version" ]; then
    echo "could not determine the workspace version" >&2
    exit 1
fi

cargo_bin="${CARGO:-cargo}"
jdk_home="${JAVA_HOME:-}"
java_bin="java"; javac_bin="javac"; jar_bin="jar"
if [ -n "$jdk_home" ]; then
    java_bin="$jdk_home/bin/java"; javac_bin="$jdk_home/bin/javac"; jar_bin="$jdk_home/bin/jar"
fi

os_name="$(uname -s)"
arch_name="$(uname -m)"
case "$os_name" in
    Linux*) os_tag="linux" ;;
    Darwin*) os_tag="macos" ;;
    *) os_tag="$(echo "$os_name" | tr '[:upper:]' '[:lower:]')" ;;
esac
case "$arch_name" in
    x86_64|amd64) arch_tag="x86_64" ;;
    aarch64|arm64) arch_tag="aarch64" ;;
    armv7l|armv7) arch_tag="armv7" ;;
    *) arch_tag="$arch_name" ;;
esac
platform="$os_tag-$arch_tag"
lib_name="libzwrs_jni.so"
case "$os_tag" in
    macos) lib_name="libzwrs_jni.dylib" ;;
    windows) lib_name="zwrs_jni.dll" ;;
esac

echo "== ZOROWARRIOR Rust Server $version =="
echo "platform: $platform   output: $output"

stage="$output/ZOROWARRIOR-Rust-Server-$version-$platform"
rm -rf "$stage"
mkdir -p "$stage/native/$platform" "$stage/plugins"

# ---- Rust engine ------------------------------------------------------------
if [ "$skip_rust" -eq 0 ]; then
    rust_args=(build --release --locked -p zwrs-jni)
    cargo_args=(build --release --locked --bin zwrs-engine)
    if [ -n "$target" ]; then
        rust_args+=(--target "$target")
        cargo_args+=(--target "$target")
        platform="$platform"
    fi
    echo "-- building the Rust engine (JNI library + standalone binary)"
    "$cargo_bin" "${rust_args[@]}"
    "$cargo_bin" "${cargo_args[@]}"
    target_dir="$repo_root/target"
    if [ -n "$target" ]; then
        target_dir="$repo_root/target/$target"
    fi
    cp "$target_dir/release/$lib_name" "$stage/native/$platform/"
    cp "$target_dir/release/zwrs-engine" "$stage/native/$platform/"
else
    echo "-- skipping the Rust build (--skip-rust); expecting native/$platform to be provided"
fi

# ---- Java launcher ----------------------------------------------------------
if [ "$skip_java" -eq 0 ]; then
    if [ -z "$jdk_home" ]; then
        echo "JAVA_HOME must point at a JDK to build the launcher" >&2
        exit 1
    fi
    echo "-- building the Java launcher with $("$javac_bin" -version 2>&1 | head -1)"
    classes="$output/.classes"
    rm -rf "$classes"
    mkdir -p "$classes"
    find java/src/main/java -name '*.java' > "$output/.sources"
    "$javac_bin" -encoding UTF-8 -d "$classes" @"$output/.sources"
    rm -f "$output/.sources"
    manifest="$output/.META-INF"
    mkdir -p "$manifest"
    cat > "$manifest/MANIFEST.MF" <<EOF
Manifest-Version: 1.0
Main-Class: io.zorowarrior.zwrs.Launcher
Implementation-Title: ZOROWARRIOR Rust Server
Implementation-Version: $version
Implementation-Vendor: ZOROWARRIOR
Created-By: scripts/bundle.sh
EOF
    # Package the native engine inside the JAR as well, so a single file works
    # when the extraction directory is read-only (the loader verifies sizes).
    jar_extra=()
    if [ -f "$stage/native/$platform/$lib_name" ]; then
        mkdir -p "$classes/native/$platform"
        cp "$stage/native/$platform/$lib_name" "$classes/native/$platform/"
    fi
    "$jar_bin" cfm "$stage/ZOROWARRIOR-Rust-Server.jar" "$manifest/MANIFEST.MF" -C "$classes" .
    rm -rf "$classes" "$manifest"
else
    echo "-- skipping the Java build (--skip-java)"
fi

# ---- supporting files -------------------------------------------------------
cat > "$stage/plugins/README.txt" <<'EOF'
Place Java plugins (.jar) and Rust plugins (directories containing manifest.json
and .wasm) in this folder.

Java plugins: the launcher reads plugin.yml, validates metadata, api-version and
the Java class-file version, and reports dependency order. Plugin code is NOT
executed yet (no Bukkit API implementation); see docs/JAVA-PLUGIN-COMPATIBILITY.md.

Rust/WASM plugins: the engine validates manifest.json, the API version and the
\0asm header. No WASM runtime is linked in this build, so nothing is executed;
see docs/RUST-PLUGIN-DEVELOPMENT.md.
EOF

cat > "$stage/START-HERE.txt" <<EOF
ZOROWARRIOR Rust Server $version ($platform)

Start the server:

    cd $(basename "$stage")
    java -jar ZOROWARRIOR-Rust-Server.jar

The JAR loads the Rust engine from native/$platform/$lib_name.
The engine runs the Minecraft server; the JAR only launches and manages it.

Useful commands:

    java -jar ZOROWARRIOR-Rust-Server.jar --selftest     verify the Java <-> Rust bridge
    java -jar ZOROWARRIOR-Rust-Server.jar --diagnose     show platform/JNI details
    java -jar ZOROWARRIOR-Rust-Server.jar --dir=/path    run a server in another directory

Requirements: Java 17 or newer. Minecraft Java clients: protocol 47-772
(1.8.x through 1.21.8) for server-list ping and login; play-state packets are
not implemented in this build.

Verify the download:

    sha256sum -c SHA256SUMS
EOF

# ---- checksums --------------------------------------------------------------
echo "-- writing SHA256SUMS"
(
    cd "$stage"
    if command -v sha256sum >/dev/null 2>&1; then
        find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
    else
        find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS
    fi
)

echo
echo "distribution ready:"
ls -l "$stage"
echo
if [ -f "$stage/SHA256SUMS" ]; then
    echo "checksums:"; cat "$stage/SHA256SUMS"
fi
