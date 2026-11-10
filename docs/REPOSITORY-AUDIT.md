# Milestone 0 — repository audit (2026-11-10)

Recorded before any code was written, and kept current as the baseline for
"what existed when the work started".

## What the repository contained

```
66360d9  Initial commit        (branch main)
README.md                     25 bytes, single line: "# ZOROWARRIOR-RUST-SERVER"
```

* **Branches:** `main` only. No tags, no releases, no pull requests.
* **Code:** none. No Rust, Java, Gradle/Maven, launcher, player system, CI,
  docs, licence or tests.
* **Data:** none to preserve.
* **GitHub:** the repository and default branch were reachable through the
  configured GitHub credentials (`gh auth status` reported the `Zorowarrior07`
  account with `repo` scope).

## Build environment available for this work

| Tool | Found | Consequence |
|---|---|---|
| `cargo` / `rustc` | absent | toolchain installed locally (see below) |
| `java` / `javac` / `jar` | absent | JDK 17 installed locally |
| `static.rust-lang.org`, `index.crates.io`, `static.crates.io`, `repo1.maven.org`, apt mirrors | unreachable | build must not need a crate registry or Maven |
| `github.com`, `api.github.com` (source tree, release assets) | HTTPS blocked for large assets; API works | upstream inspection is done through the GitHub API |
| `registry.npmjs.org`, `pypi.org` | reachable | used to obtain toolchains only |

Because no crates could be downloaded, the dependency decision was made up
front: **the whole workspace uses only the Rust standard library**, with a
small, tested JSON reader/writer, MD5/SHA-256 and calendar code instead of
`serde`/`md-5`/`sha2`/`chrono`. That also makes the build reproducible on a phone
and keeps the licence surface empty (`THIRD_PARTY_NOTICES.md`).

## Toolchains used for verification

| Tool | Version | How obtained |
|---|---|---|
| rustc | 1.95.0 (59807616e 2026-04-14) | prebuilt toolchain package (npm `@rust-sandbox/rust-1-95-x86_64-linux`) |
| cargo | 1.95.0 (f2d3ce0bd 2026-03-21) | same package |
| javac / jar / java | 17.0.8 (Temurin/HotSpot) | JDK bundle obtained locally for building |

The toolchains are build-time only; nothing from them is redistributed.

## Plan that was followed

1. Milestone 0 — this audit.
2. Milestone 1 — JAR ↔ native bridge proof (built and tested here).
3. Milestone 2 — Rust server engine serving real connections (status + login).
4. Milestones 3–5 — identity, profiles, sessions (implemented and tested).
5. Milestones 6–9 — protocol breadth, plugins, admin (partially: what could be
   implemented honestly in the time available, with the gaps documented).
6. Milestone 10–11 — packaging, CI and a verified `v0.1.0`-shaped release
   artifact (built locally, CI workflow committed, tag not yet pushed).

## What was deliberately not done

* No Pumpkin integration: Pumpkin is GPL-3.0, which would relicense the whole
  product. The evaluation and its consequences are in
  [`ARCHITECTURE.md`](ARCHITECTURE.md#engine-evaluation).
* No Python/Java server pretending to be Rust: the protocol work lives in Rust
  and the JAR never accepts a connection itself.
* No fabricated compatibility: rows without evidence say "not tested".
