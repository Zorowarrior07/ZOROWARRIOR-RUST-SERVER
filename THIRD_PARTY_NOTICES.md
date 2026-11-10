# Third-party notices

ZWRS ships no third-party source code and links no third-party crates. This file
records the licences of the tools and projects that surround it, plus the
upstream projects that were evaluated but **not** integrated.

## Shipped code

| Component | Licence | Notes |
|---|---|---|
| ZWRS workspace (`crates/*`) | MIT OR Apache-2.0 | no dependencies; `Cargo.lock` contains only workspace members |
| ZWRS launcher (`java/src`) | MIT OR Apache-2.0 | Java SE only (JDK 17+), no bundled libraries |

`Cargo.lock` lists nothing outside the workspace, so the release artifacts
contain no third-party machine code other than the Rust standard library
(dual MIT/Apache-2.0) and the JVM the user supplies.

## Build and test tooling (not shipped)

| Tool | Licence | Used for |
|---|---|---|
| Rust toolchain (`rustc`, `cargo`) | MIT OR Apache-2.0 | building the engine |
| OpenJDK (Temurin build) 17.0.8 | GPL-2.0-with-Classpath-Exception | compiling and running the launcher |
| Debian GNU/Linux (build environment) | various free licences | CI/build host |
| Python 3.11 | PSF-2.0 | `scripts/acceptance_persistence.py` only; not shipped |

## Evaluated, not integrated

These projects informed the design. **No code from them is present in this
repository or in any ZWRS artifact.**

| Project | Licence | Status |
|---|---|---|
| [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) | GPL-3.0 | Inspected (2026-11-10): 12.9k stars, actively developed, Rust workspace with `pumpkin-protocol`, `pumpkin-world`, `pumpkin-plugin-api`, WASM hosts for plugin API v0.1/v0.2. **Not embedded.** Its licence would require ZWRS to be distributed under GPL-3.0 as a whole, so integration needs a deliberate licensing decision (see `docs/ARCHITECTURE.md`, "Engine evaluation"). |
| [ViaVersion](https://github.com/ViaVersion/ViaVersion) / ViaBackwards / ViaRewind | GPL-3.0 | Java protocol translators. Technically require the Bukkit API surface ZWRS has not implemented, and are GPL-3.0. Nothing is bundled. |
| Eclipse Temurin JDK distribution (jdk4py wheel) | GPL-2.0 + Classpath Exception | Used **only** as the local build toolchain in this sandbox; it is not redistributed. |
| Minecraft server/client protocol knowledge (wiki.vg practice) | facts, not code | Packet layouts were implemented from the public protocol description. |

## Minecraft

Minecraft is a trademark of Mojang Synergies AB / Microsoft. ZWRS is an
independent, unaffiliated project. It contains no Minecraft assets, code or
data.

## If you add a dependency

1. Check its licence and whether it is compatible with MIT/Apache-2.0
   distribution (GPL/AGPL components change the licence of the whole product).
2. Add it to the table above with the version and licence file.
3. Note it in `CHANGELOG.md` and in the release notes.
