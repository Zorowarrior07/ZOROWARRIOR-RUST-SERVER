//! Server-list (status) response construction.
//!
//! The JSON shape is the one the vanilla client has used since 1.7:
//!
//! ```json
//! {
//!   "version": { "name": "1.21.8", "protocol": 772 },
//!   "players": { "max": 20, "online": 0, "sample": [] },
//!   "description": { "text": "MOTD" }
//! }
//! ```
//!
//! The `online` count and `sample` come from the real session registry, and the
//! `protocol` field is the version ZWRS actually speaks, never a guess.

use zwrs_core::json::Json;

/// Values needed to build a status response.
#[derive(Debug, Clone)]
pub struct StatusInfo {
    /// Version string shown to the client (e.g. `1.21.8`).
    pub version_name: String,
    /// Newest protocol version the server accepts.
    pub protocol: i32,
    /// MOTD text.
    pub motd: String,
    /// Maximum players.
    pub max_players: u32,
    /// Players currently online.
    pub online: u32,
    /// Up to 12 online player names to show in the hover sample.
    pub sample: Vec<String>,
    /// Optional second MOTD line.
    pub second_line: Option<String>,
}

impl StatusInfo {
    /// Renders the JSON document, pretty-printed for readability in tests.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut version = Json::object();
        version.insert("name".into(), Json::String(self.version_name.clone()));
        version.insert("protocol".into(), Json::Number(f64::from(self.protocol)));

        let mut players = Json::object();
        players.insert("max".into(), Json::Number(f64::from(self.max_players)));
        players.insert("online".into(), Json::Number(f64::from(self.online)));
        let sample: Vec<Json> = self
            .sample
            .iter()
            .take(12)
            .map(|name| {
                let mut entry = Json::object();
                entry.insert("name".into(), Json::String(name.clone()));
                entry.insert("id".into(), Json::String(String::new()));
                Json::Object(entry)
            })
            .collect();
        players.insert("sample".into(), Json::Array(sample));

        let description = match &self.second_line {
            Some(second) => {
                let mut extra = Json::object();
                extra.insert("text".into(), Json::String(format!("\n{second}")));
                Json::Array(vec![Json::String(self.motd.clone()), Json::Object(extra)])
            }
            None => Json::String(self.motd.clone()),
        };

        let mut root = Json::object();
        root.insert("version".into(), Json::Object(version));
        root.insert("players".into(), Json::Object(players));
        root.insert("description".into(), description);
        Json::Object(root).to_pretty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> StatusInfo {
        StatusInfo {
            version_name: "1.21.8".into(),
            protocol: 772,
            motd: "A ZOROWARRIOR server".into(),
            max_players: 20,
            online: 2,
            sample: vec!["Alpha".into(), "Bravo".into()],
            second_line: None,
        }
    }

    #[test]
    fn status_json_is_valid_and_complete() {
        let text = info().to_json();
        let doc = Json::parse(&text).expect("status json must parse");
        let version = doc.get("version").unwrap();
        assert_eq!(version.get("protocol").and_then(Json::as_i64), Some(772));
        assert_eq!(
            version.get("name").and_then(Json::as_str),
            Some("1.21.8")
        );
        let players = doc.get("players").unwrap();
        assert_eq!(players.get("max").and_then(Json::as_u64), Some(20));
        assert_eq!(players.get("online").and_then(Json::as_u64), Some(2));
        assert_eq!(
            players.get("sample").and_then(Json::as_array).map(<[Json]>::len),
            Some(2)
        );
        assert_eq!(
            doc.get("description")
                .and_then(Json::as_str)
                .map(str::to_string),
            Some("A ZOROWARRIOR server".to_string())
        );
    }

    #[test]
    fn second_line_becomes_a_second_component() {
        let mut i = info();
        i.second_line = Some("line two".into());
        let doc = Json::parse(&i.to_json()).unwrap();
        let desc = doc.get("description").and_then(Json::as_array).unwrap();
        assert_eq!(desc.len(), 2);
        assert_eq!(desc[0].as_str(), Some("A ZOROWARRIOR server"));
        assert_eq!(
            desc[1].get("text").and_then(Json::as_str),
            Some("\nline two")
        );
    }

    #[test]
    fn motd_with_special_characters_is_escaped() {
        let mut i = info();
        i.motd = "quote \" and \\ and §a colour".into();
        let text = i.to_json();
        let doc = Json::parse(&text).expect("escaping must keep the json valid");
        assert_eq!(
            doc.get("description").and_then(Json::as_str),
            Some("quote \" and \\ and §a colour")
        );
    }

    #[test]
    fn sample_is_capped_at_twelve() {
        let mut i = info();
        i.sample = (0..30).map(|n| format!("Player{n}")).collect();
        let doc = Json::parse(&i.to_json()).unwrap();
        assert_eq!(
            doc.get("players")
                .and_then(|p| p.get("sample"))
                .and_then(Json::as_array)
                .map(<[Json]>::len),
            Some(12)
        );
    }
}
