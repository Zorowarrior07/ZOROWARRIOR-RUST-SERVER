# Architecture

## Goals and constraints

* Users start the server with `java -jar ZOROWARRIOR-Rust-Server.jar`.
* **Rust runs the server.** The JAR must never accept a Minecraft connection.
* No third-party crates: builds must work offline and stay auditable.
* Android/Termux and Linux ARM64/x86_64 are targets; the only platform verified
  so far is Linux x86_64.

## Components

```text
┌────────────────────────────── Java process ──────────────────────────────┐
│ java -jar ZOROWARRIOR-Rust-Server.jar                                    │
│                                                                          │
│  io.zorowarrior.zwrs.Launcher         options, layout, lifecycle, console │
│  io.zorowarrior.zwrs.NativeLibraryLoader   discovery + verified extraction│
│  io.zorowarrior.zwrs.NativeBridge     `native` methods, JSON contract     │
│  io.zorowarrior.zwrs.plugin.*         JAR reader, plugin.yml, host        │
└───────────────▲──────────────────────────────────────────┬───────────────┘
                │ JNI (libzwrs_jni.so)                     │
┌───────────────┴──────────────────────────────────────────▼───────────────┐
│ crates/zwrs-jni        safe FFI wrapper, panic fence, exceptions          │
│ crates/zwrs-engine     sockets, connection state machine, admin, plugins   │
│ crates/zwrs-protocol   VarInt, framing, field codecs, version table        │
│ crates/zwrs-core       identity, JSON, time, config, storage, profiles,    │
│                        sessions                                            │
└───────────────────────────────────────────────────────────────────────────┘
                │ TCP
                ▼
     Minecraft Java Edition clients (protocol 47–772)
```

### `zwrs-core`

Pure logic, no I/O beyond the filesystem. Owns the things that must never be
wrong: identity derivation, timestamp handling, JSON, atomic writes, profiles
and sessions. Every module has unit tests for its invariants (impossible
timelines, duplicate logouts, corrupt files).

### `zwrs-protocol`

Wire primitives only. Notably it deliberately **rejects** malformed input rather
than guessing: VarInts longer than 5 bytes, frames above 2 MiB, strings above
their declared maximum, negative or zero frame lengths.

### `zwrs-engine`

Hosts the server. The connection state machine is explicit:

```text
accept ─┬─ first byte 0xFE ─────────────► legacy ping response ── close
        │
        ├─ frame: handshake (0x00)
        │      ├─ next_state 1 (status) ─► status request ─► status JSON
        │      │                            └─ ping ─► pong ─ close
        │      └─ next_state 2 (login)  ─► login start ─► authenticate
        │                                     ├─ refuse ─► disconnect packet
        │                                     └─ admit  ─► profile + session
        │                                                  persisted, then an
        │                                                  honest notice, then
        │                                                  logout on close
        └─ anything else ───────────────► logged, closed
```

`SharedState` is the only mutable shared thing:

```rust
pub struct SharedState {
    config: Config,
    players: Mutex<PlayerRepository>,   // durable profiles
    sessions: Mutex<SessionRegistry>,   // live sessions + history
    metrics: Metrics,                   // atomics
    authenticator: Box<dyn Authenticator>,
    shutting_down: AtomicBool,
    started_at: Instant, started_wall: Timestamp,
    bound_address: Mutex<Option<SocketAddr>>,
}
```

Threads: one accept thread, one thread per connection (blocking I/O with
timeouts). Both `Mutex`es are held only for short, non-blocking critical
sections; no lock is held across a socket write.

### `zwrs-jni`

The only crate allowed to use `unsafe`. `sys.rs` declares the JNI table by slot
index; `bridge.rs` wraps every call so that:

* Rust panics become `java.lang.RuntimeException` instead of unwinding into the
  JVM or aborting the process;
* engine errors become `RuntimeException` / `IllegalStateException` /
  `UnsupportedOperationException`;
* a null `JNIEnv`, null argument or interior NUL cannot crash the JVM;
* string results are JSON, so there is exactly one parser (Java) and one schema.

The engine lives in a process-wide `Mutex<Option<RunningEngine>>` because the JVM
loads a native library exactly once.

## JNI ABI and versioning

`JNIEnv` is `const struct JNINativeInterface_ * *`. The slot numbers used by
`crates/zwrs-jni/src/sys.rs` were verified against OpenJDK 17's `jni.h` with a C
program that printed `offsetof(struct JNINativeInterface_, X) / sizeof(void*)`:

| Function | Slot | Used for |
|---|---|---|
| `GetVersion` | 4 | sanity check during load |
| `FindClass` | 6 | locating exception classes |
| `ThrowNew` | 14 | raising Java exceptions |
| `ExceptionOccurred` / `ExceptionCheck` / `ExceptionClear` | 15 / 228 / 17 | not masking errors |
| `DeleteLocalRef` | 23 | releasing class references |
| `NewStringUTF` | 167 | returning JSON strings |
| `GetStringUTFLength` / `GetStringUTFChars` / `ReleaseStringUTFChars` | 168 / 169 / 170 | reading arguments |

**Versioning rules**

* `JNI_BRIDGE_VERSION` (Rust) must equal `NativeBridge.EXPECTED_BRIDGE_VERSION`
  (Java). The launcher refuses to start otherwise, with a message telling the
  user to use matching artifacts.
* Bump the version when a method is added/removed or a JSON shape changes.
* Method names follow the JNI C convention exactly:
  `Java_io_zorowarrior_zwrs_NativeBridge_<method>`.

## Bridge contract

| Method | Returns | Notes |
|---|---|---|
| `nativeBridgeVersion()` | `int` | contract version |
| `nativeBuildInfo()` | JSON | version, platform, profile, protocol range, JNI version |
| `nativeSelfTest(String)` | JSON | echo + offline UUID + versions; Java recomputes the UUID |
| `nativeStart(String dir, int port, boolean onlineMode)` | JSON | binds and starts; `port == 0` = ephemeral; refuses a second start |
| `nativeCommand(String)` | JSON `{lines, keep_running, recognised}` | runs a console command |
| `nativeState()` | JSON | running flag, address, players, counters |
| `nativeStop()` | JSON `{stopped, online_players_closed}` | graceful; idempotent |

## Startup and shutdown sequence

```text
Launcher.run()
  ├─ parse options (strict)
  ├─ prepare config/ data/ plugins/ logs/ native/  (never overwrites)
  ├─ NativeBridge.load(...)      ← local, native/<os>-<arch>/, JAR payload, java.library.path
  │    └─ verify JNI_BRIDGE_VERSION
  ├─ nativeBuildInfo()           ← logged
  ├─ nativeStart(dir, port, mode)
  │    ├─ Config::load + validate
  │    ├─ PlayerRepository::open  ← quarantine corrupt files, rebuild from profiles/
  │    ├─ reconcile sessions/profiles ← unfinished sessions, forced-offline profiles
  │    ├─ bind + accept thread
  │    └─ return bound address + capability report
  ├─ plugin discovery (metadata only)
  ├─ install SIGINT/SIGTERM handlers + JVM shutdown hook
  └─ console loop ──► nativeCommand(...)

stop
  ├─ close active sessions (credit playtime)
  ├─ save players.json + profiles/*.json + sessions.json (atomic)
  └─ join accept thread
```

Shutdown is idempotent: a second `/stop`, signal or hook is a no-op.

## Engine evaluation (why not Pumpkin yet)

Pumpkin was inspected through the GitHub API on 2026-11-10:

* Rust workspace with `pumpkin-protocol`, `pumpkin-world`, `pumpkin-plugin-api`,
  `pumpkin-wasm-host-v0_1`/`v0_2`, `pumpkin-auth`; active development
  (last push same day), ~12.9k stars.
* **Licence: GPL-3.0.** Embedding it would make the combined work GPL-3.0, which
  contradicts the MIT/Apache-2.0 licensing this project can offer today and was
  not a decision to take silently.
* Building it here is impossible anyway: it requires crates.io dependencies and
  `static.crates.io` is unreachable from the build environment.

The alternative implemented instead is a small, native engine that runs the real
protocol lifecycle for the connection stages this milestone claims. `docs/COMPATIBILITY.md` states
exactly how far that goes. When Pumpkin (or another engine) is adopted, the
`crates/zwrs-engine` boundary is the seam: the JNI contract and the launcher do
not change.

## Deliberate limitations

* Play-state packets are not implemented, so an admitted player is disconnected
  with an explanation. Their profile and session are real.
* Online mode refuses logins because session authentication is not implemented.
* One process-wide engine instance (matches `System.load` semantics).
* Connection handling is thread-per-connection; fine for the modest player counts
  this build targets, and simple enough to audit.
