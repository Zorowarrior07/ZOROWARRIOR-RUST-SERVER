# Contributing

## Ground rules

1. **Do not claim what is not tested.** Update `docs/COMPATIBILITY.md` with
   evidence, or leave the row as "not tested".
2. **Keep new dependencies justified.** The workspace currently has none.
   Adding a crate needs a note in `THIRD_PARTY_NOTICES.md` and a licence check
   first.
3. **No `unsafe` outside `crates/zwrs-jni`.** The workspace denies it; the JNI
   crate allows it because it is the FFI boundary, and every block is documented.
4. **Player data is sacred.** No code path may overwrite a corrupt database, drop
   profiles, or invent playtime.
5. Keep documentation in step with code in the same commit.

## Workflow

```bash
cargo fmt --all                     # not enforced by CI until rustfmt is pinned
cargo test --workspace --offline    # 142 tests
JAVA_HOME=/path/to/jdk scripts/test-java.sh
cargo build --release -p zwrs-jni
JAVA_HOME=/path/to/jdk scripts/bundle.sh
python3 scripts/acceptance_persistence.py --jar dist/*/ZOROWARRIOR-Rust-Server.jar
```

CI (`.github/workflows/ci.yml`) runs the Rust checks, tests, the Java suite and
the persistence acceptance test on Linux x86_64.

## Commit style

`<area>: <what changed>` where area is one of `core`, `protocol`, `engine`,
`bridge`, `launcher`, `plugins`, `docs`, `ci`, `packaging`. Explain *why* in the
body and note anything that is deliberately not done.

## Adding a Minecraft protocol feature

1. Add the packet parsing to `crates/zwrs-protocol` with round-trip tests.
2. Wire it into the connection state machine in `crates/zwrs-engine/src/server.rs`.
3. Add an end-to-end test in `crates/zwrs-engine/tests/connection.rs`.
4. Update the compatibility matrix row with the date and what was exercised.
