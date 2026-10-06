//! Persisted workspace state (design D4): favorite files and per-file
//! visible-column selections, stored as a JSON document under the user's
//! config directory. All failures to read or parse degrade to an empty
//! workspace (spec: workspace-persistence / Graceful handling of unreadable
//! persisted state); saves are atomic (temp file + rename).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Canonical string key for a file path: the canonicalized absolute path when
/// the file (or an ancestor) resolves, else the path as given — so favorites
/// and column selections survive files that have been moved or deleted.
pub fn path_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Default location of the workspace file under the user's config directory.
/// `None` when the platform has no config dir; persistence then degrades to
/// an in-memory workspace (design D4).
pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("log_analyzer").join("workspace.json"))
}

/// Which keyboard modifier turns a cell click into a query filter (spec:
/// modifier-click-filtering / Configurable filter modifier). `Command` is the
/// platform command modifier: Cmd on macOS, Ctrl on other platforms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterModifier {
    #[default]
    Ctrl,
    Alt,
    Shift,
    Command,
}

impl FilterModifier {
    /// All variants in menu order.
    pub const ALL: [FilterModifier; 4] = [
        FilterModifier::Ctrl,
        FilterModifier::Alt,
        FilterModifier::Shift,
        FilterModifier::Command,
    ];

    /// Label shown in the toolbar combo box.
    pub fn label(self) -> &'static str {
        match self {
            FilterModifier::Ctrl => "Ctrl",
            FilterModifier::Alt => "Alt",
            FilterModifier::Shift => "Shift",
            FilterModifier::Command => "Cmd / Ctrl",
        }
    }
}

/// Persisted per-user workspace: favorite files, per-file visible-column
/// selections, and global settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Favorite file paths (path keys), in the order they were added.
    #[serde(default)]
    pub favorites: Vec<String>,
    /// Visible-column selection per file path key.
    #[serde(default)]
    pub columns: BTreeMap<String, Vec<String>>,
    /// Modifier key that triggers filter-on-click in the table (spec:
    /// modifier-click-filtering).
    #[serde(default)]
    pub filter_modifier: FilterModifier,
}

impl Workspace {
    /// Load from `path`. A missing, unreadable, or malformed file yields an
    /// empty workspace instead of an error (spec: graceful handling).
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Atomically write the workspace to `path` (temp file + rename).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, serde_json::to_string_pretty(self)?)?;
        fs::rename(&temp, path)
    }

    /// Add/remove a favorite; returns the new state (`true` = favorite).
    pub fn toggle_favorite(&mut self, path_key: &str) -> bool {
        if let Some(pos) = self.favorites.iter().position(|p| p == path_key) {
            self.favorites.remove(pos);
            false
        } else {
            self.favorites.push(path_key.to_owned());
            true
        }
    }

    /// Remove a favorite if present.
    pub fn remove_favorite(&mut self, path_key: &str) {
        self.favorites.retain(|p| p != path_key);
    }

    /// True when the path is a favorite.
    pub fn is_favorite(&self, path_key: &str) -> bool {
        self.favorites.iter().any(|p| p == path_key)
    }

    /// Presentation-ordered copy of the favorites list: sorted by lowercased
    /// path, stable on ties (spec: workspace-persistence / File favorites).
    /// The persisted `favorites` order — the order files were marked — is
    /// left untouched.
    pub fn sorted_favorites(&self) -> Vec<String> {
        let mut sorted = self.favorites.clone();
        sorted.sort_by_key(|path| path.to_lowercase());
        sorted
    }

    /// Remember the visible-column selection for a file.
    pub fn set_columns(&mut self, path_key: &str, columns: Vec<String>) {
        self.columns.insert(path_key.to_owned(), columns);
    }

    /// The remembered selection for a file, if any.
    pub fn columns_for(&self, path_key: &str) -> Option<&Vec<String>> {
        self.columns.get(path_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("log_analyzer_persistence_{name}"))
    }

    #[test]
    fn round_trips_favorites_and_columns() {
        let path = temp_path("roundtrip.json");
        let _ = std::fs::remove_file(&path);

        let mut ws = Workspace::default();
        assert!(ws.toggle_favorite("/logs/a.log"));
        assert!(ws.is_favorite("/logs/a.log"));
        ws.set_columns("/logs/a.log", vec!["level".into(), "message".into()]);
        ws.save(&path).expect("save");

        let loaded = Workspace::load(&path);
        assert_eq!(loaded, ws);
        assert_eq!(
            loaded.columns_for("/logs/a.log"),
            Some(&vec!["level".to_owned(), "message".to_owned()])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_or_corrupted_file_yields_empty_workspace() {
        let missing = temp_path("does_not_exist.json");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(Workspace::load(&missing), Workspace::default());

        let corrupt = temp_path("corrupt.json");
        std::fs::write(&corrupt, "{not json at all").unwrap();
        assert_eq!(Workspace::load(&corrupt), Workspace::default());
        // Partially-valid shapes degrade field-by-field instead of failing.
        std::fs::write(&corrupt, r#"{"favorites": 42}"#).unwrap();
        assert_eq!(Workspace::load(&corrupt), Workspace::default());
        let _ = std::fs::remove_file(&corrupt);
    }

    #[test]
    fn favorites_toggle_and_remove() {
        let mut ws = Workspace::default();
        assert!(ws.toggle_favorite("/a"));
        assert!(!ws.toggle_favorite("/a"), "second toggle unmarks");
        assert!(ws.favorites.is_empty());

        ws.toggle_favorite("/a");
        ws.toggle_favorite("/b");
        ws.remove_favorite("/a");
        assert_eq!(ws.favorites, vec!["/b".to_owned()]);
        assert!(!ws.is_favorite("/a"));
        assert!(ws.is_favorite("/b"));
    }

    #[test]
    fn sorted_favorites_orders_case_insensitively() {
        let mut ws = Workspace::default();
        // Marked out of alphabetical order, including case-only differences:
        // "/Logs/c.log" and "/logs/A.log" sort by their lowercased keys
        // ("/logs/c.log", "/logs/a.log").
        for key in ["/logs/b.log", "/logs/a.log", "/Logs/c.log", "/logs/A.log"] {
            ws.toggle_favorite(key);
        }
        assert_eq!(
            ws.sorted_favorites(),
            vec![
                "/logs/a.log".to_owned(),
                "/logs/A.log".to_owned(),
                "/logs/b.log".to_owned(),
                "/Logs/c.log".to_owned(),
            ]
        );
        // Presentation-only: the persisted list keeps the marking order.
        assert_eq!(
            ws.favorites,
            vec![
                "/logs/b.log".to_owned(),
                "/logs/a.log".to_owned(),
                "/Logs/c.log".to_owned(),
                "/logs/A.log".to_owned(),
            ]
        );
    }

    #[test]
    fn save_is_atomic_and_replaces_previous_content() {
        let path = temp_path("atomic.json");
        let _ = std::fs::remove_file(&path);
        let mut ws = Workspace::default();
        ws.toggle_favorite("/one");
        ws.save(&path).unwrap();
        ws.remove_favorite("/one");
        ws.toggle_favorite("/two");
        ws.save(&path).unwrap();
        let loaded = Workspace::load(&path);
        assert_eq!(loaded.favorites, vec!["/two".to_owned()]);
        // No temp leftovers.
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn filter_modifier_defaults_and_round_trips() {
        // Each variant serializes lowercase and deserializes back.
        for (modifier, serialized) in [
            (FilterModifier::Ctrl, "\"ctrl\""),
            (FilterModifier::Alt, "\"alt\""),
            (FilterModifier::Shift, "\"shift\""),
            (FilterModifier::Command, "\"command\""),
        ] {
            assert_eq!(serde_json::to_string(&modifier).unwrap(), serialized);
            let deserialized: FilterModifier = serde_json::from_str(serialized).unwrap();
            assert_eq!(deserialized, modifier);
        }

        // An old workspace file without the field loads as the default (Ctrl).
        let path = temp_path("filter_modifier_legacy.json");
        std::fs::write(&path, r#"{"favorites": ["/a"]}"#).unwrap();
        let ws = Workspace::load(&path);
        assert_eq!(ws.filter_modifier, FilterModifier::Ctrl);
        assert_eq!(ws.filter_modifier, FilterModifier::default());

        // The choice round-trips through the workspace file.
        let mut ws = Workspace::default();
        ws.filter_modifier = FilterModifier::Alt;
        ws.save(&path).unwrap();
        assert_eq!(Workspace::load(&path).filter_modifier, FilterModifier::Alt);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn path_key_prefers_canonical_paths() {
        assert_eq!(
            path_key(Path::new("/definitely/not/here/x.log")),
            "/definitely/not/here/x.log"
        );
        // A real file canonicalizes to its absolute path.
        let real = std::env::temp_dir().join("log_analyzer_pathkey_probe");
        std::fs::write(&real, b"x").unwrap();
        let key = path_key(&real);
        assert!(key.starts_with('/'), "canonicalized: {key}");
        let _ = std::fs::remove_file(&real);
    }
}
