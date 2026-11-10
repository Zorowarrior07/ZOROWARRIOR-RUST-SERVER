//! Plugin discovery for both ecosystems.
//!
//! # What this module does
//!
//! * Finds Java `.jar` plugins in `plugins/` and reports their checksums.
//! * Finds Rust/WASM plugins (a directory with `manifest.json` + `.wasm`) and
//!   validates the manifest, the WASM header and the API version.
//! * Reports problems per plugin without aborting the whole scan.
//!
//! # What this module deliberately does **not** claim
//!
//! * Java plugins are **not** executed by the Rust engine. Reading `plugin.yml`
//!   out of a compressed JAR requires a DEFLATE implementation; the Java
//!   launcher already has `java.util.zip`, so metadata inspection and loading are
//!   documented as belonging to the Java plugin host (`docs/JAVA-PLUGIN-COMPATIBILITY.md`).
//! * WASM plugins are validated but **not executed**: there is no WASM runtime
//!   linked in this build. `docs/RUST-PLUGIN-DEVELOPMENT.md` records the status.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zwrs_core::json::Json;
use zwrs_core::sha256::sha256_hex;
use zwrs_core::{Error, Result};

/// Plugin API version understood by this build.
pub const PLUGIN_API_VERSION: u32 = 1;

/// Which ecosystem a plugin belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    /// A Bukkit/Spigot/Paper `.jar`.
    JavaJar,
    /// A Rust `.wasm` plugin directory.
    RustWasm,
}

impl PluginKind {
    /// Stable label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JavaJar => "java-jar",
            Self::RustWasm => "rust-wasm",
        }
    }
}

/// A discovered plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    /// Manifest/derived name.
    pub name: String,
    /// Directory or file that defines the plugin.
    pub path: PathBuf,
    /// Ecosystem.
    pub kind: PluginKind,
    /// Size in bytes.
    pub size_bytes: u64,
    /// SHA-256 of the entry artifact, for operator verification.
    pub sha256: String,
    /// Declared version, when a manifest could be read.
    pub version: Option<String>,
    /// Whether the plugin is loadable by this build.
    pub loadable: bool,
    /// Why it is not loadable, or notes for the operator.
    pub notes: Vec<String>,
}

/// Result of scanning the plugin directory.
#[derive(Debug, Default, Clone)]
pub struct PluginScan {
    /// Plugins that were discovered (loadable or not).
    pub plugins: Vec<DiscoveredPlugin>,
    /// Problems that are not tied to a single plugin.
    pub problems: Vec<String>,
    /// Whether the directory existed.
    pub directory_present: bool,
}

impl PluginScan {
    /// Count of plugins this build can actually load.
    #[must_use]
    pub fn loadable_count(&self) -> usize {
        self.plugins.iter().filter(|p| p.loadable).count()
    }

    /// Human-readable lines for `/status` and the console.
    #[must_use]
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.directory_present {
            lines.push("No plugins directory found.".to_string());
            return lines;
        }
        if self.plugins.is_empty() {
            lines.push("Plugins directory is empty.".to_string());
        } else {
            lines.push(format!(
                "{} plugin artifact(s) discovered, {} loadable by this build:",
                self.plugins.len(),
                self.loadable_count()
            ));
            for p in &self.plugins {
                let version = p.version.as_deref().unwrap_or("unversioned");
                lines.push(format!(
                    "  - {} [{}] {} ({version}, {} bytes, sha256 {})",
                    p.name,
                    p.kind.as_str(),
                    if p.loadable { "loadable" } else { "not loadable" },
                    p.size_bytes,
                    &p.sha256[..16.min(p.sha256.len())]
                ));
                for note in &p.notes {
                    lines.push(format!("      note: {note}"));
                }
            }
        }
        for problem in &self.problems {
            lines.push(format!("  problem: {problem}"));
        }
        lines
    }
}

/// Manifest for a Rust/WASM plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmManifest {
    /// Plugin name (`[A-Za-z0-9_-]{1,64}`).
    pub name: String,
    /// Plugin version (free-form, but must be non-empty).
    pub version: String,
    /// API version the plugin was built against.
    pub api_version: u32,
    /// Entry file name, relative to the plugin directory.
    pub entry: String,
}

impl WasmManifest {
    /// Parses and validates `manifest.json` contents.
    ///
    /// # Errors
    /// Returns [`Error::Config`] describing the first problem found.
    pub fn parse(text: &str) -> Result<Self> {
        let doc = Json::parse(text)?;
        let name = doc
            .get("name")
            .and_then(Json::as_str)
            .ok_or_else(|| Error::Config("manifest `name` must be a string".into()))?
            .to_string();
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error::Config(format!(
                "manifest `name` must match [A-Za-z0-9_-]{{1,64}}, got {name:?}"
            )));
        }
        let version = doc
            .get("version")
            .and_then(Json::as_str)
            .ok_or_else(|| Error::Config("manifest `version` must be a string".into()))?
            .to_string();
        if version.trim().is_empty() {
            return Err(Error::Config("manifest `version` must not be empty".into()));
        }
        let api_version = doc
            .get("api_version")
            .and_then(Json::as_u64)
            .ok_or_else(|| Error::Config("manifest `api_version` must be an integer".into()))?;
        let api_version = u32::try_from(api_version)
            .map_err(|_| Error::Config("manifest `api_version` is out of range".into()))?;
        let entry = doc
            .get("entry")
            .and_then(Json::as_str)
            .unwrap_or("plugin.wasm")
            .to_string();
        // The entry must stay inside the plugin directory.
        if entry.contains('/') || entry.contains('\\') || entry.contains("..") {
            return Err(Error::Config(format!(
                "manifest `entry` must be a plain file name, got {entry:?}"
            )));
        }
        if !entry.ends_with(".wasm") {
            return Err(Error::Config(format!(
                "manifest `entry` must end with .wasm, got {entry:?}"
            )));
        }
        Ok(Self {
            name,
            version,
            api_version,
            entry,
        })
    }
}

/// Scans `dir` for Java and WASM plugins.
///
/// # Errors
/// Only returns an error for problems that make the whole directory unreadable;
/// individual plugin problems are recorded in the result.
pub fn scan(dir: &Path) -> Result<PluginScan> {
    let mut scan = PluginScan::default();
    if !dir.exists() {
        return Ok(scan);
    }
    scan.directory_present = true;

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            if path.join("manifest.json").exists() {
                match inspect_wasm_plugin(&path) {
                    Ok(plugin) => scan.plugins.push(plugin),
                    Err(e) => scan.problems.push(format!("{}: {e}", path.display())),
                }
            }
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")) {
            match inspect_java_plugin(&path) {
                Ok(plugin) => scan.plugins.push(plugin),
                Err(e) => scan.problems.push(format!("{}: {e}", path.display())),
            }
        }
    }
    Ok(scan)
}

fn digest_file(path: &Path) -> Result<(u64, String)> {
    let bytes = std::fs::read(path)?;
    Ok((
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        sha256_hex(&bytes),
    ))
}

fn inspect_java_plugin(path: &Path) -> Result<DiscoveredPlugin> {
    let (size_bytes, sha256) = digest_file(path)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let mut notes = vec![
        "Bukkit/Spigot/Paper plugin metadata is read by the Java plugin host (java.util.zip); the Rust engine does not execute Java plugins.".to_string(),
        "Discovery alone does not imply API compatibility.".to_string(),
    ];
    // The Rust engine never loads Java plugins, so `loadable` stays false; the
    // Java host makes that decision. Container verification below only tells the
    // operator whether the file looks like a JAR at all.
    let loadable = false;
    let bytes = std::fs::read(path)?;
    if bytes.len() >= 22 && bytes.windows(4).any(|w| w == b"PK\x05\x06") {
        notes.push("ZIP container verified.".to_string());
    } else {
        notes.push("Not a ZIP archive: the file does not look like a JAR.".to_string());
    }
    Ok(DiscoveredPlugin {
        name,
        path: path.to_path_buf(),
        kind: PluginKind::JavaJar,
        size_bytes,
        sha256,
        version: None,
        loadable,
        notes,
    })
}

fn inspect_wasm_plugin(dir: &Path) -> Result<DiscoveredPlugin> {
    let manifest_path = dir.join("manifest.json");
    let manifest_text = std::fs::read_to_string(&manifest_path)?;
    let manifest = WasmManifest::parse(&manifest_text)?;
    let entry_path = dir.join(&manifest.entry);
    let mut notes = Vec::new();
    let (size_bytes, sha256) = if entry_path.exists() {
        digest_file(&entry_path)?
    } else {
        notes.push(format!(
            "entry file `{}` is missing from the plugin directory",
            manifest.entry
        ));
        (0, String::new())
    };

    // No WASM runtime is linked, so nothing is reported as loadable even when
    // the manifest and binary header are valid.
    let loadable = false;
    if entry_path.exists() {
        let bytes = std::fs::read(&entry_path)?;
        if bytes.starts_with(b"\0asm") {
            notes.push("WebAssembly binary header verified.".to_string());
            if manifest.api_version != PLUGIN_API_VERSION {
                notes.push(format!(
                    "plugin targets API v{}, this build implements v{PLUGIN_API_VERSION}",
                    manifest.api_version
                ));
            } else {
                notes.push(
                    "Manifest is valid, but this build does not link a WASM runtime, so the plugin is not executed."
                        .to_string(),
                );
            }
        } else {
            notes.push("Not a WebAssembly binary (missing \\0asm header).".to_string());
        }
    }

    Ok(DiscoveredPlugin {
        name: manifest.name,
        path: dir.to_path_buf(),
        kind: PluginKind::RustWasm,
        size_bytes,
        sha256,
        version: Some(manifest.version),
        loadable,
        notes,
    })
}

/// Capability report for `/version` and the launcher.
#[must_use]
pub fn capabilities(java_enabled: bool, wasm_enabled: bool) -> BTreeMap<&'static str, String> {
    let mut map = BTreeMap::new();
    map.insert(
        "minecraft_protocol",
        "handshake, status ping (all supported protocol versions), legacy 0xFE ping".to_string(),
    );
    map.insert(
        "minecraft_login",
        "login start parsing, username validation, offline-mode identity; online-mode authentication NOT implemented".to_string(),
    );
    map.insert(
        "minecraft_play",
        "NOT implemented: chunks, entities, inventory, chat, world transitions".to_string(),
    );
    map.insert(
        "player_profiles",
        "implemented: atomic JSON persistence, restart-safe, corruption quarantine".to_string(),
    );
    map.insert(
        "sessions",
        "implemented: login/logout events, playtime, crash-recovery policy".to_string(),
    );
    map.insert(
        "java_plugins",
        if java_enabled {
            "discovery and checksums in the Rust engine; loading and Bukkit APIs are the Java host's responsibility (not yet implemented)".to_string()
        } else {
            "disabled by configuration".to_string()
        },
    );
    map.insert(
        "wasm_plugins",
        if wasm_enabled {
            format!("discovery and manifest validation (API v{PLUGIN_API_VERSION}); no WASM runtime linked, so nothing is executed")
        } else {
            "disabled by configuration".to_string()
        },
    );
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("zwrs-plugins-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn missing_directory_is_not_an_error() {
        let scan = scan(Path::new("/definitely/not/here")).unwrap();
        assert!(!scan.directory_present);
        assert!(scan.plugins.is_empty());
    }

    #[test]
    fn java_jar_is_discovered_with_checksum() {
        let dir = temp_dir("jar");
        // Minimal ZIP: local header signature is enough for this test's container check.
        let mut jar = b"PK\x03\x04".to_vec();
        jar.extend_from_slice(&[0u8; 40]);
        jar.extend_from_slice(b"PK\x05\x06");
        jar.extend_from_slice(&[0u8; 18]);
        std::fs::write(dir.join("ExamplePlugin.jar"), &jar).unwrap();

        let scan = scan(&dir).unwrap();
        assert_eq!(scan.plugins.len(), 1);
        let plugin = &scan.plugins[0];
        assert_eq!(plugin.name, "ExamplePlugin");
        assert_eq!(plugin.kind, PluginKind::JavaJar);
        assert_eq!(plugin.sha256, sha256_hex(&jar));
        assert!(!plugin.loadable);
        assert!(plugin.notes.iter().any(|n| n.contains("ZIP")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn non_zip_jar_is_flagged() {
        let dir = temp_dir("bogus");
        std::fs::write(dir.join("Fake.jar"), b"not a zip at all").unwrap();
        let scan = scan(&dir).unwrap();
        assert!(scan.plugins[0].notes.iter().any(|n| n.contains("Not a ZIP")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wasm_plugin_manifest_and_header_validation() {
        let dir = temp_dir("wasm");
        let plugin_dir = dir.join("hello");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("manifest.json"),
            r#"{"name":"hello","version":"0.1.0","api_version":1,"entry":"plugin.wasm"}"#,
        )
        .unwrap();
        std::fs::write(plugin_dir.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00").unwrap();

        let scan = scan(&dir).unwrap();
        assert_eq!(scan.plugins.len(), 1);
        let plugin = &scan.plugins[0];
        assert_eq!(plugin.name, "hello");
        assert_eq!(plugin.kind, PluginKind::RustWasm);
        assert_eq!(plugin.version.as_deref(), Some("0.1.0"));
        assert!(!plugin.loadable, "no WASM runtime is linked");
        assert!(plugin.notes.iter().any(|n| n.contains("does not link a WASM runtime")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wasm_manifest_rejects_bad_values() {
        assert!(WasmManifest::parse(r#"{"name":"bad name","version":"1","api_version":1}"#).is_err());
        assert!(WasmManifest::parse(r#"{"name":"ok","version":"","api_version":1}"#).is_err());
        assert!(WasmManifest::parse(r#"{"name":"ok","version":"1"}"#).is_err());
        assert!(WasmManifest::parse(r#"{"name":"ok","version":"1","api_version":1,"entry":"../x.wasm"}"#).is_err());
        assert!(WasmManifest::parse(r#"{"name":"ok","version":"1","api_version":1,"entry":"x.txt"}"#).is_err());
        assert!(WasmManifest::parse(r#"{"name":"ok","version":"1","api_version":1}"#).is_ok());
    }

    #[test]
    fn invalid_wasm_binary_is_reported() {
        let dir = temp_dir("badwasm");
        let plugin_dir = dir.join("broken");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("manifest.json"),
            r#"{"name":"broken","version":"1.0.0","api_version":1,"entry":"plugin.wasm"}"#,
        )
        .unwrap();
        std::fs::write(plugin_dir.join("plugin.wasm"), b"ELF...").unwrap();
        let scan = scan(&dir).unwrap();
        assert!(scan.plugins[0]
            .notes
            .iter()
            .any(|n| n.contains("Not a WebAssembly binary")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn api_version_mismatch_is_reported() {
        let dir = temp_dir("apiv");
        let plugin_dir = dir.join("future");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("manifest.json"),
            r#"{"name":"future","version":"2.0.0","api_version":99,"entry":"plugin.wasm"}"#,
        )
        .unwrap();
        std::fs::write(plugin_dir.join("plugin.wasm"), b"\0asm").unwrap();
        let scan = scan(&dir).unwrap();
        assert!(scan.plugins[0]
            .notes
            .iter()
            .any(|n| n.contains("targets API v99")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
