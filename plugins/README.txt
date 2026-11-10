Place Java plugins (.jar) and Rust plugins (directories containing
manifest.json + .wasm) in this folder.

Java plugins are inspected by the launcher's plugin host (Bukkit-compatible
subset; see docs/JAVA-PLUGIN-COMPATIBILITY.md). Loading plugin code is not
implemented yet, so every plugin is reported as metadata-compatible or
incompatible and nothing is executed.

Rust/WASM plugins are validated by the engine (manifest, API version, \0asm
header). This build links no WASM runtime, so nothing is executed; see
docs/RUST-PLUGIN-DEVELOPMENT.md.
