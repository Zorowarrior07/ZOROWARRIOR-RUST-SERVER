# Compatibility

**Rule for this file: a row may only be marked supported if a test exercises it
and the evidence is named.** Rows without evidence say "not tested".

## Protocol versions (status ping / login)

| Client version | Protocol | Status ping | Login | Notes |
|---|---|---|---|---|
| 1.8.x (1.8.9) | 47 | **tested** (simulated client) | **tested** (offline mode) | `crates/zwrs-engine/tests/connection.rs`, acceptance script (`protocol 47 (1.8.x) login admitted`) |
| 1.12.2 | 340 | tested via the same code path | same | covered by the version-range check, not run with a 1.12.2 client |
| 1.16.5 | 754 | **tested** (`malformed_handshake_does_not_kill_the_server` uses 754) | same | same code path |
| 1.20.1 | 763 | **tested** (acceptance script asserts protocol 772 advertised) | **tested** | login packet layout differs from 1.8 in trailing fields; only the username field is read |
| 1.21.x (up to 1.21.8) | 767–772 | **tested** | same | advertised version is 1.21.8 / protocol 772 |
| 1.6 and older | ≤ 78 | legacy `0xFE` ping **tested** | not supported | modern handshake required for login |
| 26.x and later | unknown | **not tested** | **not tested** | no client exists in the build environment |

"Tested" here means the exact byte sequences a client sends were exercised over
a real TCP socket against the real engine. **No real Minecraft client was
connected**, because none is available in this environment. Real-client testing
must be recorded in this file when it happens — with the client version, the
date, and what was observed.

## Gameplay surface

| Feature | Status | Notes |
|---|---|---|
| Server-list ping (MOTD, counts, version) | supported | real online count and player sample |
| Legacy ping | supported | `0xFE` response |
| Handshake | supported | forwarding suffixes stripped for logging only; not trusted |
| Login start | supported | username read and validated |
| Encryption / compression | **not implemented** | compression threshold + zlib are required before join game |
| Login success / join game | **not implemented** | |
| Chunks, blocks, entities, inventory | **not implemented** | |
| Chat, signed chat | **not implemented** | |
| Commands from inside the game | **not implemented** | console commands work |
| World transitions, respawn | **not implemented** | |
| Online-mode authentication | **not implemented** | logins refused with an explanation |
| Protocol translation (ViaVersion-style) | **not implemented** | no translation layer exists |

An admitted login is real in the sense that identity, profile and session are
created and persisted; the client is then disconnected with a message saying the
world is not implemented. That is the honest behaviour, and it is asserted by
tests (`offline_login_creates_a_real_profile_and_session`).

## Platform matrix

| Platform | Built | Tested | Notes |
|---|---|---|---|
| linux-x86_64 (glibc) | **yes** | **yes** | Debian 12, Rust 1.95.0, OpenJDK 17.0.8 |
| linux-aarch64 | no | no | requires an aarch64 toolchain/linker; commands in `BUILDING.md` |
| android-aarch64 (Termux) | no | no | needs `aarch64-linux-android` NDK target; see `ANDROID-INSTALLATION.md` |
| macos-x86_64 / arm64 | no | no | `libzwrs_jni.dylib` naming is handled by the loader, untested |

## Java runtime

| Runtime | Status |
|---|---|
| OpenJDK 17.0.8 (Temurin) | **tested** |
| OpenJDK 21 / 25 | not tested; the launcher only uses APIs present since 17 |
| Android system Java | not tested; Termux `pkg install openjdk-17` is the documented path |

## Plugin compatibility

| Ecosystem | Discovery | Metadata validation | Execution |
|---|---|---|---|
| Bukkit / Spigot / Paper `.jar` | **tested** | **tested** (`plugin.yml`, `api-version`, class-file version, dependency order) | **not implemented** |
| Rust/WASM plugin directory | **tested** (Rust engine) | **tested** (`manifest.json`, API v1, `\0asm` header) | **not implemented** (no WASM runtime linked) |

Loading a JAR is **not** evidence of compatibility, and neither is passing
metadata validation. See `JAVA-PLUGIN-COMPATIBILITY.md`.

## Evidence index

| Claim | Test |
|---|---|
| JAR loads the Rust engine and stops it cleanly | `BridgeIntegrationTest.testLifecycleStartCommandStateStop`, `--selftest` |
| Bridge version mismatch is detected | `NativeBridge.load` + `BridgeIntegrationTest.testBridgeVersionMatches` |
| Status ping reports real data | `status_ping_reports_real_state`, `acceptance_persistence.py` |
| Offline UUID matches vanilla | `identity::tests::offline_uuid_matches_vanilla_vectors`, `OfflineUuidTest`, `BridgeIntegrationTest.testSelfTestAgreesWithJavaUuidDerivation` |
| Profiles persist across restart | `profile_and_session_survive_a_restart`, acceptance script |
| Crash recovery is honest | `crash_recovery_marks_interrupted_sessions_unfinished`, `prepare_forces_stale_online_state_offline` |
| Corrupt data is quarantined | `corrupt_index_is_rebuilt_from_profile_files`, `corrupt_json_is_quarantined_not_overwritten` |
