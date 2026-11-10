# Session system

## Model

```text
login admitted ──► SessionState::Active   (started_at = now)
                        │
        disconnect / graceful stop
                        ▼
                  SessionState::Closed    (ended_at, duration = end - start)
                        │
        process died while active
                        ▼
                SessionState::Unfinished  (ended_at = null, duration unknown)
```

A session is only opened **after** the login is accepted, and only after the
player checks (protocol range, capacity, duplicate login) pass. TCP connections
that never log in are not sessions.

### Recorded fields

| Field | Meaning |
|---|---|
| `id` | monotonic per process; stable in storage |
| `uuid`, `username` | identity at login |
| `started_at` | admission time |
| `ended_at` | logout time, or `null` |
| `state` | `active` / `closed` / `unfinished` |
| `duration_seconds` | `ended_at - started_at` for closed sessions, otherwise `null` |
| `protocol` | protocol version the client reported |

## Playtime accounting

Only measured durations are credited:

```rust
playtime += ended_at - started_at   // saturating, never negative
```

* A second logout for the same session is a no-op (`record_logout` returns `0`
  when the profile is already offline), so double-counting is impossible.
* Clock skew that would produce a negative duration is clamped to `0`.
* A crashed/`unfinished` session adds **nothing**: the logout time is unknown,
  and inventing one would corrupt statistics. `/playerinfo` shows the profile as
  offline with the last known logout timestamp, and the session is visible as
  `unfinished` with `duration_seconds: null`.

## Duplicate and rapid reconnects

* A second login with the same UUID while a session is active is refused
  (`"You are already connected to this server."`).
* A reconnect after a clean logout creates a new session; history keeps both.
* Session ids continue from the maximum restored id after a restart, so ids are
  never reused.
* Retention keeps at most 5 000 session records in memory; trimming **never**
  drops an active session.

## Crash recovery policy

On start:

1. `sessions.json` is loaded; every record in state `active` is marked
   `unfinished` (logged with a count).
2. Every profile with `online: true` is forced offline (logged with a count).
3. Profiles and sessions are re-written atomically.

Result: no player is ever reported online after a restart, and no playtime is
invented. This is asserted by
`crash_recovery_marks_interrupted_sessions_unfinished` (end-to-end, with a
simulated hard kill) and `prepare_forces_stale_online_state_offline` (unit).

## Session history queries

`/playerinfo <name>` and `PlayerRepository::playerinfo_lines` show the stored
profile with live session age for online players. Raw history lives in
`sessions.json`; the console reports counts and durations through `/status`.

## Guarantees and non-guarantees

| Guarantee | Evidence |
|---|---|
| Online flag is false after any restart | `crash_recovery_marks_interrupted_sessions_unfinished` |
| Playtime never decreases or goes negative | `duplicate_logout_does_not_double_count`, `playtime_never_negative` |
| Duplicate active sessions are impossible | `no_duplicate_active_sessions` |
| Session history survives restarts | `persisted_sessions_roundtrip`, acceptance script |
| Durations are measured, not estimated | `join_logout_accumulates_playtime` |

Non-guarantees:

* Playtime is refreshed on join, logout and shutdown. A hard kill loses the
  duration of in-flight sessions by design (they become `unfinished`).
* Session records are capped at 5 000 retained entries; older closed sessions
  are dropped from the registry, not invented.
