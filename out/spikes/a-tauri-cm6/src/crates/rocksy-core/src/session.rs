//! Last session (active vault, open tabs), persisted as `session.json` in the
//! application data directory.

use crate::atomic::write_data_file;
use crate::error::{ApiError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SESSION_FILE: &str = "session.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(default)]
    pub active_vault: Option<String>,
    #[serde(default)]
    pub tabs: Vec<String>,
    #[serde(default)]
    pub active_tab: Option<String>,
}

fn session_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SESSION_FILE)
}

/// Last saved session, or `None` if there is none or the file is unreadable
/// or corrupted (a broken session must never prevent startup).
pub fn load_session(data_dir: &Path) -> Option<Session> {
    let bytes = fs::read(session_path(data_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Persists the session atomically.
pub fn save_session(data_dir: &Path, session: &Session) -> Result<()> {
    let json = serde_json::to_vec_pretty(session)
        .map_err(|e| ApiError::io(format!("cannot serialize session: {e}")))?;
    write_data_file(&session_path(data_dir), &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_files, TempDir};

    #[test]
    fn round_trip_and_camel_case() {
        let dir = TempDir::new("session");
        let data = dir.path().join("data");
        assert_eq!(load_session(&data), None);

        let s = Session {
            active_vault: Some("/v".into()),
            tabs: vec!["/v/a.md".into(), "/tmp/b.md".into()],
            active_tab: Some("/tmp/b.md".into()),
        };
        save_session(&data, &s).unwrap();
        assert_eq!(load_session(&data), Some(s.clone()));
        assert_eq!(all_files(&data), vec![SESSION_FILE]);

        let json = fs::read_to_string(data.join(SESSION_FILE)).unwrap();
        assert!(json.contains("\"activeVault\""));
        assert!(json.contains("\"activeTab\""));
        assert!(!json.contains("active_vault"));

        // The contract's JSON shape deserializes, nulls included.
        let parsed: Session =
            serde_json::from_str(r#"{"activeVault":null,"tabs":[],"activeTab":null}"#).unwrap();
        assert_eq!(parsed, Session::default());
    }

    #[test]
    fn corrupted_file_is_ignored_then_overwritten() {
        let dir = TempDir::new("session-bad");
        dir.write(SESSION_FILE, "{ not json");
        assert_eq!(load_session(dir.path()), None);
        save_session(dir.path(), &Session::default()).unwrap();
        assert_eq!(load_session(dir.path()), Some(Session::default()));
    }
}
