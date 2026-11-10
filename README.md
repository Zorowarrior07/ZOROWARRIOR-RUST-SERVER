# ZOROWARRIOR Rust Server (ZWRS)

**A Rust-powered Minecraft: Java Edition server, distributed as a Java `.jar`.**

```bash
# download, then:
java -jar ZOROWARRIOR-Rust-Server.jar
```

**Downloads:** [v0.1.0 release](https://github.com/Zorowarrior07/ZOROWARRIOR-RUST-SERVER/releases/tag/v0.1.0)

* `ZOROWARRIOR-Rust-Server.jar` - the launcher on its own; the Rust engine is embedded inside it and extracted at start (size-verified).
* `ZOROWARRIOR-Rust-Server-0.1.0-linux-x86_64.tar.gz` - full distribution: JAR + `native/linux-x86_64/` (engine and the standalone `zwrs-engine` binary).
* `release-SHA256SUMS` - checksums for both assets.

```bash
sha256sum -c release-SHA256SUMS && java -jar ZOROWARRIOR-Rust-Server.jar --selftest
```

A prebuilt copy of both files also lives in [`releases/`](releases/) with a
`SHA256SUMS` file, so the artifact is available even without a network.

The JAR is a launcher: it locates the native Rust engine for your operating
system, loads it through JNI, starts it, forwards your console commands, and
shuts it down cleanly. **The Rust engine runs the server** — it owns the
listening socket, speaks the Minecraft protocol, persists player data and
reports its own capabilities.

> ## Status: v0.1.0 — architecture proof, not a playable server
>
> Implemented and tested: the JAR → JNI → Rust startup path, Minecraft
> **handshake, server-list ping (protocol 47–772, i.e. 1.8.x–1.21.8)**,
> the legacy `0xFE` ping, **login** with username validation and offline-mode
> identity derivation, **persistent player profiles and sessions** (atomic JSON,
> crash recovery), and admin commands backed by real data.
>
> **Not implemented:** play-state (you cannot join and move around), online-mode
> authentication (Mojang/Microsoft session service), world/chunks/entities/
> inventory/chat delivery, and WASM plugin execution. Java plugins **do** run:
> they are loaded, enabled, and receive real join/quit events and console
> commands. The engine reports every gap in `/status`, `/version`, the status
> ping and its capability report instead of pretending.
> See [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

---

## What actually works right now

| Area | State | Evidence |
|---|---|---|
| `java -jar` loads the Rust engine via JNI | **working** | `--selftest` (9 checks), `BridgeIntegrationTest` |
| Engine runs on its own threads, graceful stop | **working** | `tests/connection.rs`, acceptance script |
| Server-list ping, 1.8.x–1.21.8 | **working** | `status_ping_reports_real_state`, acceptance script |
| Legacy `0xFE` ping | **working** | `legacy_ping_is_answered` |
| Login (offline mode), username validation, protocol range | **working** | `offline_login_creates_a_real_profile_and_session` |
| Player profiles (JSON, atomic, restart-safe) | **working** | `profile_and_session_survive_a_restart` |
| Sessions, playtime, crash recovery | **working** | `crash_recovery_marks_interrupted_sessions_unfinished` |
| `/help /version /status /players /playerinfo /plugins /reload /stop` | **working** | `src/admin.rs` tests |
| Java plugin discovery, `plugin.yml` validation, checksums | **working** | `JavaPluginHostTest` |
| **Java plugins actually run** (load, enable, events, commands) | **working** | `PluginRuntimeTest`, `acceptance_plugins.py` |
| WASM plugin validation (manifest, API version, header) | **working** (no execution) | `plugins::tests` |
| Play state (world, movement, inventory, chat) | **not implemented** | reported by `/version` |
| Java plugin code execution, events, commands | **working** (Bukkit subset) | `PluginRuntimeTest`, `acceptance_plugins.py` |
| Online-mode authentication | **not implemented** (logins refused) | `online_mode_refuses_logins_instead_of_pretending` |
| Bukkit API surface beyond the documented subset | **not implemented** | `PluginRuntime.supportedApis()` |

Nothing in this list is claimed without a test that exercises it. Tests that use
simulated packets are labelled as such; tests with a real Minecraft client have
not been run (see the compatibility matrix).

## Quick start (Linux x86_64)

```bash
# 1. Build (needs Rust 1.75+ and JDK 17+)
cargo build --release -p zwrs-jni
JAVA_HOME=/path/to/jdk scripts/bundle.sh

# 2. Run
cd dist/ZOROWARRIOR-Rust-Server-0.1.0-linux-x86_64
java -jar ZOROWARRIOR-Rust-Server.jar

# 3. Verify the distribution and the bridge
sha256sum -c SHA256SUMS
java -jar ZOROWARRIOR-Rust-Server.jar --selftest
```

The first start creates:

```text
config/zwrs.properties      documented, validated configuration
data/players/players.json   profiles (authoritative, atomic writes)
data/players/sessions.json  session history
data/players/profiles/*.json one readable file per player
logs/latest.log             launcher log
plugins/                    Java .jar and Rust/WASM plugin artifacts
native/<os>-<arch>/         the Rust engine
```

Point your Minecraft client at `localhost:25565`. You will see the server in the
list with its real MOTD, player counts and protocol version. Joining reports
that play-state packets are not implemented yet — the login itself is real and
the profile it creates is persisted.

## Console commands

```text
/help              command list
/version           engine, platform, protocol range, auth mode, and what is missing
/status            uptime, address, players, connections, plugin summary
/players           players online right now
/playerinfo <name> persisted profile (UUID, joins, playtime, logout, history)
/plugins           discovered Java and Rust plugin artifacts with checksums
/reload            flush player data and rescan the plugin directory
/stop              graceful shutdown
```

The same commands are available through the JNI bridge (`nativeCommand`) and the
standalone engine binary (`native/<os>-<arch>/zwrs-engine`).

## Architecture in one diagram

```text
java -jar ZOROWARRIOR-Rust-Server.jar
        │
        ▼
  Java launcher (JAR)                     io.zorowarrior.zwrs.Launcher
  · argument parsing, layout, plugins     · NativeLibraryLoader (JNI, verified)
        │  nativeStart / nativeCommand / nativeStop / nativeState
        ▼
  libzwrs_jni.so  (crates/zwrs-jni)       safe FFI wrapper, panic-safe
        │
        ▼
  Rust engine (crates/zwrs-engine)        owns the socket and the server
  · handshake / status / login            crate::server, crate::packet
  · identity, profiles, sessions          zwrs-core (admin, profile, session)
  · atomic JSON storage                   zwrs-core::storage
        │
        ▼
  Minecraft Java Edition clients (protocol 47–772)
```

Details, including the JNI ABI and its versioning rules:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Repository layout

```text
crates/zwrs-core/      identity, JSON, time, config, storage, profiles, sessions
crates/zwrs-protocol/  VarInt, framing, protocol version table
crates/zwrs-engine/    server: sockets, packets, status, login, admin, plugins
crates/zwrs-jni/       JNI cdylib + safe wrapper (the only crate with `unsafe`)
java/src/main/         launcher, JNI binding, plugin host (no third-party deps)
java/src/test/         launcher/plugin/bridge tests with a tiny built-in runner
scripts/               bundle.sh, test-java.sh, acceptance_persistence.py
docs/                  architecture, compatibility, player data, plugins, builds
```

## Verified environments

| Item | Value |
|---|---|
| OS / arch | Debian 12 (bookworm), linux-x86_64, 2 vCPU, 4 GB RAM |
| Rust | 1.95.0 (rustc 59807616e 2026-04-14), cargo 1.95.0 |
| Java | OpenJDK 17.0.8 (Temurin/HotSpot), `javac`/`jar` 17.0.8 |
| Third-party crates | none (the entire workspace builds with `--offline`) |
| Test results | 142 Rust tests + 37 Java tests + 10 acceptance checks, all passing |

Android/Termux, Linux ARM64 and macOS have **not** been built or tested here;
their requirements and the exact commands are documented rather than claimed.
See [docs/ANDROID-INSTALLATION.md](docs/ANDROID-INSTALLATION.md) and
[docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).

## Documentation

* [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — components, JNI ABI, lifecycle, threading
* [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) — version matrix with evidence and gaps
* [docs/PLAYER-DATA.md](docs/PLAYER-DATA.md) — storage format, atomicity, recovery
* [docs/SESSION-SYSTEM.md](docs/SESSION-SYSTEM.md) — sessions, playtime, crash policy
* [docs/JAVA-PLUGIN-COMPATIBILITY.md](docs/JAVA-PLUGIN-COMPATIBILITY.md) — the Bukkit subset that runs, and what does not
* [docs/RUST-PLUGIN-DEVELOPMENT.md](docs/RUST-PLUGIN-DEVELOPMENT.md) — manifest, API version, SDK plan
* [docs/ANDROID-INSTALLATION.md](docs/ANDROID-INSTALLATION.md) — Termux requirements and caveats
* [docs/LINUX-INSTALLATION.md](docs/LINUX-INSTALLATION.md) — install, run, systemd
* [docs/BUILDING.md](docs/BUILDING.md) — toolchains, offline builds, cross-compilation
* [docs/TESTING.md](docs/TESTING.md) — how to run every test and what each proves
* [docs/REPOSITORY-AUDIT.md](docs/REPOSITORY-AUDIT.md) — initial repository state and plan
* [CHANGELOG.md](CHANGELOG.md) · [SECURITY.md](SECURITY.md) · [CONTRIBUTING.md](CONTRIBUTING.md)
* [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) · [LICENSE](LICENSE)

## Licensing

The code in this repository is MIT OR Apache-2.0 (your choice). ZWRS currently
depends on **no** third-party code, and it does **not** include, fork or link
Pumpkin or ViaVersion. If either is integrated later, its own licence (Pumpkin is
GPL-3.0) will be recorded in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
before any release that contains it.
