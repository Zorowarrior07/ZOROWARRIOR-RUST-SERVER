# Building

## Requirements

| Tool | Minimum | Used for |
|---|---|---|
| Rust (cargo, rustc) | 1.75 (edition 2021) | engine + JNI library |
| JDK | 17 | launcher compile, `jar`, running tests |
| `python3` | 3.8 | acceptance script only |

No crates are downloaded: the workspace has no dependencies, so
`cargo build --offline` works even without network access.

## Standard build

```bash
cargo build --release -p zwrs-jni      # native/libzwrs_jni.so
cargo build --release -p zwrs-engine   # native/zwrs-engine (standalone)
JAVA_HOME=/path/to/jdk scripts/bundle.sh
```

`scripts/bundle.sh` produces:

```text
dist/ZOROWARRIOR-Rust-Server-<version>-<os>-<arch>/
├── ZOROWARRIOR-Rust-Server.jar     # launcher (engine also embedded inside)
├── native/<os>-<arch>/
│   ├── libzwrs_jni.so              # the Rust engine, loaded by the JAR
│   └── zwrs-engine                 # same engine as a standalone binary
├── plugins/README.txt
├── START-HERE.txt
└── SHA256SUMS
```

Options: `--output DIR`, `--target TRIPLE`, `--skip-rust`, `--skip-java`.

## Tests

```bash
cargo test --workspace --offline            # 142 tests
JAVA_HOME=/path/to/jdk scripts/test-java.sh # 37 Java tests (includes JNI integration)
JAVA_HOME=/path/to/jdk python3 scripts/acceptance_persistence.py \
    --jar dist/ZOROWARRIOR-Rust-Server-*/ZOROWARRIOR-Rust-Server.jar
```

## Offline / restricted environments

Everything is dependency-free by design. If a toolchain cannot be installed
system-wide, a prebuilt toolchain directory works:

```bash
export PATH=/path/to/toolchain/bin:$PATH
export JAVA_HOME=/path/to/jdk
cargo build --release --offline -p zwrs-jni
```

`cargo fmt` and `cargo clippy` need their components; if they are unavailable,
formatting and lints are checked in CI instead (see `.github/workflows/ci.yml`).

## Cross-compilation

### Linux ARM64 (from x86_64)

```bash
sudo apt-get install gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu
cat >> ~/.cargo/config.toml <<'TOML'
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
TOML
cargo build --release --target aarch64-unknown-linux-gnu -p zwrs-jni
cargo build --release --target aarch64-unknown-linux-gnu -p zwrs-engine
```

Put the result under `native/linux-aarch64/`. The launcher constructs that tag
from `os.name`/`os.arch`; verify with `--diagnose`.

### Android ARM64 / Termux

Requires the NDK (or Termux `rust` + `cargo-ndk`):

```bash
rustup target add aarch64-linux-android
export NDK=/path/to/android-ndk
cat >> ~/.cargo/config.toml <<'TOML'
[target.aarch64-linux-android]
linker = "/path/to/android-ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang"
TOML
cargo build --release --target aarch64-linux-android -p zwrs-jni
```

**Do not reuse a glibc `linux-aarch64` build on Android**: Bionic is not glibc and
the library will fail to load. See [ANDROID-INSTALLATION.md](ANDROID-INSTALLATION.md).

## Reproducibility notes

* Pin the toolchain in `rust-toolchain.toml` if you need byte-identical builds;
  the project has not committed one because the CI matrix may need a newer
  compiler for cross targets.
* `Cargo.lock` is committed and contains workspace members only.
* `scripts/bundle.sh` writes `SHA256SUMS` for every file it ships; verify with
  `sha256sum -c SHA256SUMS`.
