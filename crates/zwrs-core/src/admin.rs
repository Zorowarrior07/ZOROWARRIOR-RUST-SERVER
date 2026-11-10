//! Player profile repository and the admin query surface.
//!
//! # Storage layout
//!
//! ```text
//! data/
//! └── players/
//!     ├── players.json          # authoritative index, atomic writes
//!     ├── sessions.json         # session history, atomic writes
//!     └── profiles/
//!         └── <uuid>.json       # one readable document per player
//! ```
//!
//! `players.json` is the authoritative store. The per-UUID files are a
//! human-inspectable mirror that also serves as the documented recovery source
//! if `players.json` is ever damaged (see [`PlayerRepository::open`]).
//!
//! # Guarantees
//!
//! * Nothing is invented: a profile only exists after a real, admitted join.
//! * A malformed file is quarantined, never silently replaced.
//! * Every mutation is followed by an atomic save when the caller asks for it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::identity::{PlayerIdentity, Username, Uuid};
use crate::json::Json;
use crate::profile::PlayerProfile;
use crate::storage::{self, read_json};
use crate::time::Timestamp;

/// What happened when a join was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinOutcome {
    /// Whether this was the player's first ever join.
    pub first_join: bool,
    /// Previous username, when the player joined under a new name.
    pub renamed_from: Option<String>,
}

/// Player profile repository.
#[derive(Debug)]
pub struct PlayerRepository {
    players_file: PathBuf,
    profiles_dir: PathBuf,
    profiles: BTreeMap<Uuid, PlayerProfile>,
    /// Lower-cased username -> UUID.
    name_index: BTreeMap<String, Uuid>,
    store_ip: bool,
    /// Notes produced during load (quarantine, recovery) for the engine to log.
    recovery_notes: Vec<String>,
}

impl PlayerRepository {
    /// Opens (or creates) the repository.
    ///
    /// If `players.json` is corrupt it is quarantined and the per-profile files
    /// are used to rebuild the index; if there is nothing to rebuild from, the
    /// error is returned so the operator can restore a backup instead of the
    /// engine starting with an empty database.
    ///
    /// # Errors
    /// Propagates I/O and storage errors.
    pub fn open(players_file: &Path, profiles_dir: &Path, store_ip: bool) -> Result<Self> {
        let mut repo = Self {
            players_file: players_file.to_path_buf(),
            profiles_dir: profiles_dir.to_path_buf(),
            profiles: BTreeMap::new(),
            name_index: BTreeMap::new(),
            store_ip,
            recovery_notes: Vec::new(),
        };

        match read_json(players_file) {
            Ok(None) => {}
            Ok(Some(doc)) => {
                for record in storage::records(&doc)? {
                    let profile = PlayerProfile::from_json(&record)?;
                    repo.insert(profile);
                }
            }
            Err(e) => {
                let rebuilt = repo.load_profile_files()?;
                if rebuilt == 0 {
                    return Err(e);
                }
                repo.recovery_notes.push(format!(
                    "{e}; rebuilt {rebuilt} profile(s) from {}",
                    repo.profiles_dir.display()
                ));
            }
        }

        if !store_ip {
            for profile in repo.profiles.values_mut() {
                profile.clear_ip();
            }
        }
        Ok(repo)
    }

    fn load_profile_files(&mut self) -> Result<usize> {
        if !self.profiles_dir.exists() {
            return Ok(0);
        }
        let mut loaded = 0;
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&self.profiles_dir)?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        entries.sort();
        for path in entries {
            let Ok(Some(doc)) = read_json(&path) else {
                self.recovery_notes
                    .push(format!("could not use {}", path.display()));
                continue;
            };
            match PlayerProfile::from_json(&doc) {
                Ok(profile) => {
                    self.insert(profile);
                    loaded += 1;
                }
                Err(e) => self
                    .recovery_notes
                    .push(format!("ignoring {}: {e}", path.display())),
            }
        }
        Ok(loaded)
    }

    fn insert(&mut self, profile: PlayerProfile) {
        let name = profile.username().canonical();
        self.name_index.insert(name, profile.uuid());
        self.profiles.insert(profile.uuid(), profile);
    }

    /// Notes produced while loading (quarantines, rebuilds).
    #[must_use]
    pub fn recovery_notes(&self) -> &[String] {
        &self.recovery_notes
    }

    /// Number of known profiles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Whether no profiles exist yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Profile by UUID.
    #[must_use]
    pub fn get(&self, uuid: Uuid) -> Option<&PlayerProfile> {
        self.profiles.get(&uuid)
    }

    /// Profile by username (case-insensitive), including historical names.
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Option<&PlayerProfile> {
        let lowered = name.to_ascii_lowercase();
        if let Some(uuid) = self.name_index.get(&lowered) {
            return self.profiles.get(uuid);
        }
        // Fall back to historical names so `/playerinfo OldName` still works.
        self.profiles.values().find(|p| {
            p.previous_usernames()
                .iter()
                .any(|u| u.eq_ignore_ascii_case(name))
        })
    }

    /// All profiles with `online == true`, sorted by username.
    #[must_use]
    pub fn online(&self) -> Vec<&PlayerProfile> {
        let mut online: Vec<&PlayerProfile> = self
            .profiles
            .values()
            .filter(|p| p.online())
            .collect();
        online.sort_by_key(|p| p.username().canonical());
        online
    }

    /// All profiles sorted by username.
    #[must_use]
    pub fn all(&self) -> Vec<&PlayerProfile> {
        let mut all: Vec<&PlayerProfile> = self.profiles.values().collect();
        all.sort_by_key(|p| p.username().canonical());
        all
    }

    /// Records a successful join, creating the profile when needed.
    ///
    /// # Errors
    /// Propagates I/O errors from the immediate save.
    pub fn record_join(
        &mut self,
        identity: &PlayerIdentity,
        now: Timestamp,
        ip: Option<String>,
    ) -> Result<JoinOutcome> {
        let ip = if self.store_ip { ip } else { None };
        let outcome = match self.profiles.get_mut(&identity.uuid()) {
            Some(profile) => {
                let renamed_from = if profile.username().as_str() != identity.username().as_str() {
                    Some(profile.username().to_string())
                } else {
                    None
                };
                // Drop the stale index entry for the old spelling.
                self.name_index.remove(&profile.username().canonical());
                profile.record_join(identity, now, ip);
                JoinOutcome {
                    first_join: false,
                    renamed_from,
                }
            }
            None => {
                self.profiles.insert(
                    identity.uuid(),
                    PlayerProfile::new(identity, now, ip),
                );
                JoinOutcome {
                    first_join: true,
                    renamed_from: None,
                }
            }
        };
        self.name_index
            .insert(identity.username().canonical(), identity.uuid());
        self.save()?;
        Ok(outcome)
    }

    /// Records a logout and returns the credited session duration.
    ///
    /// # Errors
    /// Propagates I/O errors from the immediate save.
    pub fn record_logout(&mut self, uuid: Uuid, now: Timestamp) -> Result<Option<u64>> {
        let Some(profile) = self.profiles.get_mut(&uuid) else {
            return Ok(None);
        };
        if !profile.online() {
            return Ok(Some(0));
        }
        let duration = profile.record_logout(now);
        self.save()?;
        Ok(Some(duration))
    }

    /// Forces every profile offline (crash recovery / startup).
    ///
    /// Returns the number of profiles that were incorrectly marked online plus
    /// the number of seconds each is credited with (always zero, because the
    /// real logout time is unknown).
    pub fn force_all_offline(&mut self) -> Vec<(Uuid, String)> {
        let mut affected = Vec::new();
        for profile in self.profiles.values_mut() {
            if profile.online() {
                profile.force_offline();
                affected.push((profile.uuid(), profile.username().to_string()));
            }
        }
        affected
    }

    /// Persists the index and the per-player mirror files atomically.
    ///
    /// # Errors
    /// Propagates I/O errors; on failure the previous files remain intact.
    pub fn save(&self) -> Result<()> {
        let records: Vec<Json> = self
            .profiles
            .values()
            .map(PlayerProfile::to_json)
            .collect();
        let doc = storage::document(records);
        storage::write_atomic(&self.players_file, doc.to_pretty().as_bytes())?;

        std::fs::create_dir_all(&self.profiles_dir)?;
        for profile in self.profiles.values() {
            let path = self.profiles_dir.join(format!("{}.json", profile.uuid()));
            storage::write_atomic(&path, profile.to_json().to_pretty().as_bytes())?;
        }
        Ok(())
    }

    /// Lines describing all online players, for `/players`.
    #[must_use]
    pub fn players_lines(&self) -> Vec<String> {
        let online = self.online();
        if online.is_empty() {
            return vec!["No players are online.".to_string()];
        }
        let mut lines = vec![format!(
            "{} player(s) online ({} known profile(s)):",
            online.len(),
            self.profiles.len()
        )];
        for p in online {
            lines.push(format!("  - {} ({})", p.username(), p.uuid()));
        }
        lines
    }

    /// Lines for `/playerinfo <name>`; `session_age` is seconds since the
    /// player's current session started, if they are online.
    #[must_use]
    pub fn playerinfo_lines(
        &self,
        name: &str,
        now: Timestamp,
        session_age: Option<u64>,
    ) -> Vec<String> {
        match self.find_by_name(name) {
            Some(profile) => profile.summary(now, session_age),
            None => vec![format!(
                "No stored profile for `{name}`. This server only reports players that have actually joined."
            )],
        }
    }
}

/// Parses a username argument from a command line, returning a clear message.
///
/// # Errors
/// Returns [`Error::Config`] when the name is not a valid Minecraft username.
pub fn parse_player_argument(raw: &str) -> Result<Username> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::Config("a username is required".into()));
    }
    Username::parse(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json as J;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!(
                "zwrs-admin-{tag}-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn players(&self) -> PathBuf {
            self.0.join("players.json")
        }
        fn profiles(&self) -> PathBuf {
            self.0.join("profiles")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ident(name: &str) -> PlayerIdentity {
        PlayerIdentity::offline(Username::parse(name).unwrap())
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix(secs)
    }

    #[test]
    fn first_join_creates_and_persists_profile() {
        let dir = TempDir::new("first");
        let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        assert!(repo.is_empty());

        let who = ident("Notch");
        let outcome = repo.record_join(&who, at(1_000), Some("198.51.100.4".into())).unwrap();
        assert!(outcome.first_join);
        assert_eq!(repo.len(), 1);

        // Reloading from disk yields the same data.
        let reloaded = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        let p = reloaded.get(who.uuid()).expect("profile");
        assert_eq!(p.username().as_str(), "Notch");
        assert_eq!(p.join_count(), 1);
        assert!(p.online());
        assert_eq!(p.first_join(), at(1_000));
        // IP storage is disabled by default.
        assert_eq!(p.last_ip(), None);
        assert!(dir.profiles().join(format!("{}.json", who.uuid())).exists());
    }

    #[test]
    fn logout_then_rejoin_accumulates_playtime_across_restart() {
        let dir = TempDir::new("playtime");
        let who = ident("Notch");
        {
            let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
            repo.record_join(&who, at(1_000), None).unwrap();
            assert_eq!(repo.record_logout(who.uuid(), at(1_600)).unwrap(), Some(600));
        }
        {
            let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
            let p = repo.get(who.uuid()).unwrap();
            assert_eq!(p.total_playtime_seconds(), 600);
            assert!(!p.online());
            assert_eq!(p.last_logout(), Some(at(1_600)));
            let outcome = repo.record_join(&who, at(2_000), None).unwrap();
            assert!(!outcome.first_join);
            assert_eq!(repo.get(who.uuid()).unwrap().join_count(), 2);
        }
        let repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        assert_eq!(repo.get(who.uuid()).unwrap().online(), true);
    }

    #[test]
    fn username_change_is_tracked_for_a_stable_identity() {
        let dir = TempDir::new("rename");
        let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        // A rename that keeps the UUID is what happens in online mode, where the
        // UUID is issued by the auth service rather than derived from the name.
        let uuid = Uuid::parse("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
        let old = PlayerIdentity::online(uuid, Username::parse("OldName").unwrap());
        repo.record_join(&old, at(1_000), None).unwrap();
        repo.record_logout(uuid, at(1_100)).unwrap();

        let renamed = PlayerIdentity::online(uuid, Username::parse("NewName").unwrap());
        let outcome = repo.record_join(&renamed, at(2_000), None).unwrap();
        assert!(!outcome.first_join);
        assert_eq!(outcome.renamed_from.as_deref(), Some("OldName"));
        assert_eq!(repo.len(), 1);
        let profile = repo.get(uuid).unwrap();
        assert_eq!(profile.username().as_str(), "NewName");
        assert_eq!(profile.previous_usernames(), &["OldName".to_string()]);
        assert!(repo.find_by_name("newname").is_some());
        assert!(repo.find_by_name("OLDNAME").is_some(), "history stays searchable");
    }

    #[test]
    fn offline_rename_is_a_new_identity() {
        let dir = TempDir::new("offline-rename");
        let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        let old = ident("OldName");
        repo.record_join(&old, at(1_000), None).unwrap();
        let renamed = ident("NewName");
        assert_ne!(old.uuid(), renamed.uuid());
        repo.record_join(&renamed, at(2_000), None).unwrap();
        assert_eq!(repo.len(), 2, "offline mode derives a different UUID per name");
    }

    #[test]
    fn crash_recovery_forces_profiles_offline() {
        let dir = TempDir::new("recover");
        let who = ident("Notch");
        {
            let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
            repo.record_join(&who, at(1_000), None).unwrap();
        }
        let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        assert_eq!(repo.online().len(), 1, "persisted state says online");
        let affected = repo.force_all_offline();
        assert_eq!(affected.len(), 1);
        repo.save().unwrap();
        let reloaded = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        assert!(reloaded.online().is_empty());
        // No playtime was invented for the crashed session.
        assert_eq!(reloaded.get(who.uuid()).unwrap().total_playtime_seconds(), 0);
    }

    #[test]
    fn corrupt_index_is_rebuilt_from_profile_files() {
        let dir = TempDir::new("rebuild");
        let who = ident("Notch");
        {
            let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
            repo.record_join(&who, at(1_000), None).unwrap();
            repo.record_logout(who.uuid(), at(1_500)).unwrap();
        }
        std::fs::write(dir.players(), b"{ this is corrupted").unwrap();

        let repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        assert_eq!(repo.len(), 1);
        assert!(!repo.recovery_notes().is_empty());
        assert!(dir
            .0
            .read_dir()
            .unwrap()
            .filter_map(std::result::Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-")));
        assert_eq!(repo.get(who.uuid()).unwrap().total_playtime_seconds(), 500);
    }

    #[test]
    fn corrupt_index_without_profiles_refuses_to_start_empty() {
        let dir = TempDir::new("norebuild");
        std::fs::write(dir.players(), b"}").unwrap();
        let err = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap_err();
        assert!(err.to_string().contains("preserved"), "{err}");
        assert!(!dir.players().exists(), "corrupt file is preserved, not deleted");
    }

    #[test]
    fn ip_is_stored_only_when_enabled() {
        let dir = TempDir::new("ip");
        let who = ident("Notch");
        {
            let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), true).unwrap();
            repo.record_join(&who, at(1_000), Some("203.0.113.9".into())).unwrap();
            repo.record_logout(who.uuid(), at(1_100)).unwrap();
            assert_eq!(repo.get(who.uuid()).unwrap().last_ip(), Some("203.0.113.9"));
        }
        // Disabling the option strips existing addresses on load.
        let repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        assert_eq!(repo.get(who.uuid()).unwrap().last_ip(), None);
    }

    #[test]
    fn playerinfo_reads_persisted_values_only() {
        let dir = TempDir::new("info");
        let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        let who = ident("Notch");
        repo.record_join(&who, at(1_000), None).unwrap();

        let lines = repo.playerinfo_lines("notch", at(1_030), Some(30)).join("\n");
        assert!(lines.contains("Username: Notch"), "{lines}");
        assert!(lines.contains("Online: yes"), "{lines}");
        assert!(lines.contains("Playtime: 30s (30 seconds)"), "{lines}");
        assert!(lines.contains(&who.uuid().to_string()), "{lines}");

        let missing = repo.playerinfo_lines("Ghost", at(1_030), None).join("\n");
        assert!(missing.contains("No stored profile"), "{missing}");
    }

    #[test]
    fn players_lines_lists_online_only() {
        let dir = TempDir::new("lines");
        let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        assert_eq!(repo.players_lines()[0], "No players are online.");
        let a = ident("Alpha");
        let b = ident("Bravo");
        repo.record_join(&a, at(1_000), None).unwrap();
        repo.record_join(&b, at(1_000), None).unwrap();
        repo.record_logout(b.uuid(), at(1_100)).unwrap();
        let lines = repo.players_lines();
        assert!(lines[0].contains("1 player(s) online"));
        assert!(lines.iter().any(|l| l.contains("Alpha")));
        assert!(!lines.iter().any(|l| l.contains("Bravo")));
    }

    #[test]
    fn save_writes_valid_documents() {
        let dir = TempDir::new("doc");
        let mut repo = PlayerRepository::open(&dir.players(), &dir.profiles(), false).unwrap();
        repo.record_join(&ident("Notch"), at(1_000), None).unwrap();
        let text = std::fs::read_to_string(dir.players()).unwrap();
        let doc = J::parse(&text).expect("players.json must be valid json");
        assert_eq!(doc.get("format").and_then(J::as_u64), Some(1));
        assert_eq!(doc.get("records").and_then(J::as_array).map(<[J]>::len), Some(1));
    }

    #[test]
    fn parse_player_argument_validates() {
        assert!(parse_player_argument(" Notch ").is_ok());
        assert!(parse_player_argument("").is_err());
        assert!(parse_player_argument("bad name").is_err());
    }
}
