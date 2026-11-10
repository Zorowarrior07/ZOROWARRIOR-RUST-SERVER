# Changelog

All notable changes to ZWRS are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* **Java plugins run.** A Bukkit-compatible API subset (`org.bukkit.*`), a
  per-plugin class loader with server-first API resolution, `JavaPlugin` lifecycle
  (`onLoad`/`onEnable`/`onDisable`), `PluginManager.registerEvents` with priority
  ordering, `PlayerJoinEvent`/`PlayerQuitEvent` raised by the Rust engine, command
  registration and console dispatch, per-plugin data folders and logging.
* Engine `HostListener` seam and `EngineHooks` JNI callbacks (bridge version 2).
* `scripts/acceptance_plugins.py`: compiles a real plugin, runs it in the real
  server and asserts on what the plugin observed.

### Fixed

* JNI `CallStatic*Method` slot indices were the array-taking variants and
  segfaulted the JVM; all slots are now measured with `offsetof`, documented in
  one table and guarded by a test.
* Engine threads attached with `AttachCurrentThread` kept the JVM alive after
  `/stop`; they now attach as daemons.
* Console output is flushed per command so piped consoles (systemd, CI) show it
  immediately.

## [0.1.0] - 2026-11-10

First tagged milestone: the JAR-to-Rust architecture proof plus a real
connection/login/player-data pipeline. **Not a playable server** — see
`docs/COMPATIBILITY.md`.

### Added

* Workspace of four Rust crates with **no third-party dependencies**
  (`zwrs-core`, `zwrs-protocol`, `zwrs-engine`, `zwrs-jni`), buildable offline.
* `zwrs-core`: validated configuration (`config/zwrs.properties`, unknown keys
  rejected), MD5 with RFC 1321 vectors, SHA-256 with FIPS vectors, strict JSON
  reader/writer, UTC calendar arithmetic, atomic file writes with corruption
  quarantine and backup snapshots, player profiles, session registry with
  playtime accounting, profile repository with per-UUID mirror files.
* Vanilla **offline-mode UUID** derivation, cross-checked against an independent
  Java implementation in the bridge self-test.
* `zwrs-protocol`: VarInt/VarLong, packet framing with a 2 MiB ceiling, checked
  field readers/writers, protocol version name table (47–772).
* `zwrs-engine`: TCP server with handshake, status/ping, legacy `0xFE` ping,
  login (validation, protocol range, capacity, duplicate sessions), profile and
  session persistence, startup crash reconciliation, plugin discovery for Java
  and WASM artifacts, console commands, `/status` metrics, standalone binary.
* `zwrs-jni`: self-contained JNI binding (slot indices verified against OpenJDK
  17 `jni.h`), panic-safe FFI wrapper, JSON contract, idempotent stop.
* Java launcher: strict options, layout preparation, platform-tagged native
  library discovery with JAR-embedded verified extraction, SIGINT/SIGTERM
  handling, console forwarding, `--selftest`, `--diagnose`, `--status`.
* Java plugin host: defensive ZIP reader, `plugin.yml` parser, manifest
  validation, `api-version` and class-file version checks, dependency ordering
  with cycle detection, SHA-256 of each artifact. **No plugin code is executed.**
* Scripts: `bundle.sh` (JAR + native + SHA256SUMS), `test-java.sh`,
  `acceptance_persistence.py` (mandatory persistence test using the real JAR).
* Documentation: architecture, compatibility matrix, player data, sessions,
  Java plugin compatibility, Rust plugin development, Android, Linux, building,
  testing, repository audit, security, contributing.

### Verified

* 173 Rust test assertions, 37 Java tests and 10 acceptance checks pass on
  Debian 12 x86_64 with Rust 1.95.0 and OpenJDK 17.0.8.
* `java -jar` starts the Rust engine from `native/` and from inside the JAR,
  runs a self-test across the bridge, serves status pings, admits offline-mode
  logins, persists profiles/sessions and survives a restart.

### Not included (explicitly)

* Play-state packets, world/chunk/entity handling, inventory, chat, commands
  from inside the game.
* Online-mode (Mojang/Microsoft) authentication — logins are refused with an
  explanation.
* Bukkit/Spigot/Paper API implementation and plugin execution.
* A WASM runtime for Rust plugins.
* Protocol translation for versions outside 47–772.
* Android ARM64, Linux ARM64 and macOS builds (documented, not built).
