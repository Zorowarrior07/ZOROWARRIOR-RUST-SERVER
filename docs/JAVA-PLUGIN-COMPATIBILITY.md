# Java plugin compatibility

## Summary

| Capability | State |
|---|---|
| Discover `.jar` files in `plugins/` | **working** |
| Validate the JAR container | **working** (ZIP structure, entry bounds, deflate limits) |
| Parse `plugin.yml` | **working** (YAML subset used by Bukkit descriptors) |
| Check required fields and formats | **working** (`name`, `version`, `main`, class-name syntax) |
| Check `api-version` | **working** (recognises `1.13`–`1.21`) |
| Check the Java runtime requirement | **working** (reads the main class-file version) |
| Dependency ordering and cycles | **working** |
| SHA-256 per artifact | **working** |
| Load a plugin class / call `onEnable` | **not implemented** |
| Event bus, scheduler, services, configuration | **not implemented** |
| Commands, permissions at runtime | **not implemented** |

In other words: ZWRS can tell you *whether a plugin is likely to be loadable*,
but it cannot run one yet. Discovery is not compatibility, and this project does
not claim "Bukkit support".

## What a plugin needs to be reported as `METADATA_COMPATIBLE`

1. The file is a valid ZIP/JAR (end-of-central-directory + local headers).
2. `plugin.yml` exists at the root and parses.
3. `name` matches `[A-Za-z0-9_.-]{1,64}`, `version` is present, `main` is a
   fully-qualified class name.
4. `api-version` (if present) is `1.x` with `x ≤ 21`.
5. The main class file exists inside the JAR and its major version implies a
   Java release the launcher's runtime provides (`classMajor - 44 ≤ 17` here).

Anything failing one of these is reported as `INCOMPATIBLE` with the reason, and
the scan continues with the remaining plugins.

## What was actually executed

Nothing. `JavaPluginHost.supportedApis()` lists exactly what is implemented and
`unsupportedApis()` lists the Bukkit surface that is missing; the console prints
`host status: metadata validation only; no plugin code has been executed`.

To run a plugin, the project needs at minimum:

* `org.bukkit.plugin.java.JavaPlugin` (or a compatible `Plugin` interface) plus
  `JavaPluginLoader`-style class loading with a `PluginClassLoader` that isolates
  plugin classes;
* the `Server` singleton (`getPluginManager`, `getScheduler`, `getLogger`,
  `getWorld`, `getPlayer`, `broadcastMessage`, ...);
* an event bus with `Listener`/`EventHandler` registration and dispatch from the
  Rust engine's join/quit/chat events;
* permission and command registration surfaces;
* `FileConfiguration`/`YamlConfiguration` for `getConfig()`.

Each of these is a separate, testable milestone; none is started.

## Class-loading safety

When loading arrives it must:

* resolve the plugin's own classes only from its own JAR (no parent delegation
  leaks for `org.bukkit.*` internals);
* never allow a plugin to shadow JDK classes;
* catch `Throwable` around `onLoad`/`onEnable` so one broken plugin cannot take
  the server down;
* record the plugin's SHA-256 and refuse to load an artifact that changed
  silently between restarts (configurable).

## Writing a testable plugin today

`java/src/test/java/io/zorowarrior/zwrs/test/TestSupport` builds JARs with a
synthetic class file, and `PluginManifest.read(jar)` lets you assert on the
descriptor. That is how the host's tests work. A real plugin JAR can be dropped
into `plugins/` and the server will report its name, version, main class,
api-version, checksum, dependency order and validation result via the launcher
log and `plugins/` scan.
