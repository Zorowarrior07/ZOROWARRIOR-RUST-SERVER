# Testing

Every claim in `docs/COMPATIBILITY.md` points at one of these tests. Tests that
use simulated packets are labelled as such; no test pretends to be a real
Minecraft client.

## Rust

```bash
cargo test --workspace --offline
```

| Suite | Count | What it proves |
|---|---|---|
| `zwrs-core` unit tests | 74 | MD5 vectors, SHA-256 vectors, offline UUID convention, username/UUID validation, JSON round-trips and rejection cases, UTC calendar and parsing, configuration validation (unknown/duplicate keys, cross-field rules), atomic writes, corrupt-file quarantine, snapshot/prune, profile serialisation and invariants, session lifecycle/retention, repository rebuild and `playerinfo` output |
| `zwrs-protocol` unit tests | 16 | VarInt/VarLong known encodings and over-long rejection, framing bounds, field codecs, UTF-8 rejection |
| `zwrs-engine` unit tests | 34 | handshake/status/login packet parsing, legacy ping layout, disconnect escaping, status JSON shape, admin commands against real state, capability honesty, plugin scanning and manifest validation |
| `zwrs-jni` unit tests | 10 | JSON escaping of the bridge contract, engine lifecycle helpers, JNI slot table consistency (the guard against the segfault class of bug), host-absent behaviour |
| `zwrs-engine` integration (`tests/connection.rs`) | 15 | **real TCP sockets**: status/ping round trip, legacy ping, offline login → persisted profile + session, restart survival, duplicate login refusal, invalid username, unsupported protocol, online-mode refusal, crash recovery, clean idempotent stop, malformed input resilience, zero-length/oversized frame handling |

Totals: 74 + 16 + 38 + 10 + 15 + 1 doc-test = **152 tests**.

The three extra engine integration tests cover the host-hook seam: join/quit
events carry the persisted identity, plugin commands dispatch while built-ins are
never delegated, and a listener that panics in every method leaves the engine
serving.

## Java

```bash
JAVA_HOME=/path/to/jdk scripts/test-java.sh
```

42 tests: JSON parsing/escaping, offline-UUID cross-check, ZIP/JAR reader
(deflate, truncation, missing entries), `plugin.yml` parsing and rejection cases,
plugin host validation (compatible / newer Java / missing main class / broken
JAR / empty directory), launcher option parsing, layout preparation,
diagnostics, and **real JNI integration**: version handshake, build info,
self-test agreement between Java and Rust UUID derivation, start/command/state/
stop lifecycle, double-start refusal, command-while-stopped error, invalid
arguments, online-mode capability report.

Plugin-runtime tests (5) load a plugin JAR built at test time with a real class,
check ENABLED/FAILED/DISABLED transitions, event delivery with the engine's own
JSON, command dispatch, failure isolation and a missing main class.

## Acceptance (mandatory player persistence)

```bash
JAVA_HOME=/path/to/jdk python3 scripts/acceptance_persistence.py \
    --jar dist/ZOROWARRIOR-Rust-Server-*/ZOROWARRIOR-Rust-Server.jar
```

This starts the **real JAR** with `java -jar`, connects over TCP, and asserts on
the **persisted files**:

1. server started through the JAR (Rust engine listening)
2. status ping answered with real data
3. offline login admitted
4. profile created for the real offline UUID, marked online
5. session opened and persisted while online
6. disconnect recorded (`online=false`, `last_logout` set)
7. session closed with a duration
8. `/stop` shuts the server down
9. profile, playtime, logout timestamp and session history survive a restart
10. protocol 47 (1.8.x) login admitted

Exit code 0 means all ten passed.

## Acceptance (Java plugins)

```bash
JAVA_HOME=/path/to/jdk python3 scripts/acceptance_plugins.py \
    --jar dist/ZOROWARRIOR-Rust-Server-*/ZOROWARRIOR-Rust-Server.jar
```

Compiles a real plugin against the launcher's API, packages it into a JAR, starts
the real server, logs in over TCP and asserts on the file the plugin wrote:

1. the plugin JAR was built from real plugin source
2. the good plugin was enabled while a deliberately broken plugin failed with
   its own exception message and the server kept running
3. a real login was admitted
4. `PlayerJoinEvent` carried the real UUID and protocol
5. `PlayerQuitEvent` reported `isOnline() == false`
6. `/hello Arena` reached plugin code and its output appeared on the console
7. the server still answers `/status` afterwards
8. `onDisable()` ran on shutdown

## Bridge self-test (also used by CI)

```bash
java -jar ZOROWARRIOR-Rust-Server.jar --selftest
```

Nine checks over the real JNI boundary, including an offline UUID computed
independently in Java and in Rust, a `/status` command through the bridge, an
idempotent stop and the player-database file creation. Returns non-zero on
failure.

## What is **not** tested

* A real Minecraft client of any version (none available in the build
  environment). Everything above uses hand-built packets.
* Android/Termux, Linux ARM64 and macOS builds.
* Online-mode authentication (not implemented).
* Gameplay, because play-state packets are not implemented.
* Plugin execution (not implemented).

## Manual smoke test

```bash
JAVA_HOME=/path/to/jdk scripts/bundle.sh
cd dist/ZOROWARRIOR-Rust-Server-*/ && sha256sum -c SHA256SUMS
java -jar ZOROWARRIOR-Rust-Server.jar --selftest
java -jar ZOROWARRIOR-Rust-Server.jar --port=25565
# in another terminal: add the server in Minecraft and look at the server list,
# then type /status and /playerinfo <your name> in the console
```

Expected: the server appears with the configured MOTD, protocol 772 and the real
player count; joining shows the message that play-state packets are not
implemented; `/playerinfo` shows the profile that login created.
