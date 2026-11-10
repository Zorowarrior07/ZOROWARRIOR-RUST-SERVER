# Android / Termux

**Status: not built or tested.** No Android device or NDK was available in the
environment where v0.1.0 was verified. This page therefore documents the
requirements, the exact commands, and the things that are known to differ from
desktop Linux. Do not treat it as an installation guide for a working release
until a release explicitly lists `android-aarch64` as tested.

## Why Android is not "Linux with a different name"

| Area | Desktop Linux | Android / Termux | Consequence |
|---|---|---|---|
| C library | glibc | Bionic | a `linux-aarch64` `.so` **will not load**; build for `aarch64-linux-android` |
| Rust target | `aarch64-unknown-linux-gnu` | `aarch64-linux-android` | needs NDK linker or `cargo-ndk` |
| Java | system JDK | Termux `openjdk-17` package (`pkg install openjdk-17`) | `java -jar` works from Termux, not from a normal Android app |
| Process model | normal | app lifecycle can signal/kill the process at any time | rely on the documented crash recovery (`unfinished` sessions) |
| Filesystem | anywhere | `/data/data/com.termux/files/home` is writable; `/sdcard` has restricted semantics | keep `data/`, `logs/` and `plugins/` under the Termux home |
| Memory | GBs | often 1–4 GB, with an aggressive low-memory killer | keep `view-distance`/`simulation-distance` small if you re-enable them |
| `sun.misc.Signal` | available | not guaranteed | the launcher falls back to the JVM shutdown hook |

## Build (on a host with the NDK)

```bash
rustup target add aarch64-linux-android
export NDK=$HOME/android-ndk-r26b
export CC_aarch64_linux_android=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang
export AR_aarch64_linux_android=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-ar
cargo build --release --target aarch64-linux-android -p zwrs-jni
cargo build --release --target aarch64-linux-android -p zwrs-engine
```

Or, with `cargo-ndk` installed: `cargo ndk -t arm64-v8a -o native/android-aarch64 build --release -p zwrs-jni`.

Ship the result as:

```text
native/android-aarch64/libzwrs_jni.so
native/android-aarch64/zwrs-engine
```

The launcher computes the tag from `os.name`/`os.arch`. On Termux `os.name` is
usually `Linux` and `java.vm.vendor` may contain `Android`; the loader warns in
that case and still expects `native/android-aarch64/` when it can detect Android.
If detection fails, pass the library explicitly:

```bash
java -Dzwrs.native.path=$PWD/native/android-aarch64/libzwrs_jni.so \
     -jar ZOROWARRIOR-Rust-Server.jar
```

## Run on Termux

```bash
pkg update && pkg install openjdk-17
cd ~/zwrs && unzip ZOROWARRIOR-Rust-Server-0.1.0-android-aarch64.zip
cd ZOROWARRIOR-Rust-Server-0.1.0-android-aarch64
sha256sum -c SHA256SUMS
java -jar ZOROWARRIOR-Rust-Server.jar --diagnose
java -jar ZOROWARRIOR-Rust-Server.jar --selftest
java -jar ZOROWARRIOR-Rust-Server.jar
```

Keep the server alive across Termux sessions:

```bash
pkg install termux-services
sv-enable zwrs     # after creating a service script, or simply use tmux:
pkg install tmux && tmux new -s zwrs 'java -jar ZOROWARRIOR-Rust-Server.jar'
```

## Battery and data notes

* Use `data/` inside Termux home; do not point `data-dir` at `/sdcard`.
* `termux-wake-lock` prevents the system from suspending the CPU while the
  server runs, at the cost of battery.
* A phone that kills the process will leave sessions `unfinished`; this is the
  documented and tested behaviour on restart.

## What is still missing for Android

1. A build of `zwrs-jni` for `aarch64-linux-android` (no NDK here).
2. A verification run on a physical device, recording the Android version,
   Termux package versions and the exact artifacts.
3. A documented, tested service script.

Until a release lists `android-aarch64` as tested, assume it does not work and
treat the commands above as the plan, not as a compatibility claim.
