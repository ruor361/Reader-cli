use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReaderConfig {
    pub show_progress: bool,
    pub discreet_mode: bool,
    pub mouse_support: bool,
    pub text_width: Option<u16>,
    pub library_dirs: Vec<PathBuf>,
    /// Bookshelf directory. Files added via `import` (or its `add` alias) are copied into
    /// this directory; the original is left in place. Each book gets a
    /// sibling `*.json` sidecar. `reader shelf refresh` reconciles the
    /// directory contents, sidecars, and `library.json`.
    pub bookshelf_dir: Option<PathBuf>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            discreet_mode: false,
            mouse_support: false,
            text_width: None,
            library_dirs: Vec::new(),
            bookshelf_dir: None,
            path: None,
        }
    }
}

impl ReaderConfig {
    pub fn open_default() -> Result<Self> {
        // Resolution strategy (XDG-first, with fallback) lives in `paths`.
        Self::open(crate::paths::configuration_path()?)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let mut config = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            match serde_json::from_str(&text) {
                Ok(cfg) => cfg,
                Err(err) => {
                    // JSON is unparseable. Move the bad file aside and start
                    // fresh so the user is not stuck with a binary that
                    // refuses to launch. The original is preserved at
                    // `<path>.corrupt` for inspection.
                    eprintln!(
                        "warning: {} is corrupt ({err}); initializing a fresh configuration. \
                         The old file was preserved as {}.corrupt.",
                        path.display(),
                        path.display(),
                    );
                    let backup = path.with_extension("json.corrupt");
                    let _ = std::fs::rename(&path, &backup);
                    Self::default()
                }
            }
        } else {
            Self::default()
        };
        config.path = Some(path);
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        let text = serde_json::to_string_pretty(self)?;
        crate::atomic_file::write(path, text.as_bytes())
            .with_context(|| format!("Failed to save {}", path.display()))?;
        Ok(())
    }

    pub fn toggle_progress(&mut self) {
        self.show_progress = !self.show_progress;
    }

    pub fn toggle_discreet_mode(&mut self) {
        self.discreet_mode = !self.discreet_mode;
    }
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct ReaderConfigWire {
    show_progress: bool,
    discreet_mode: Option<bool>,
    privacy_mode: Option<bool>,
    mouse_support: bool,
    text_width: Option<u16>,
    library_dirs: Vec<PathBuf>,
    bookshelf_dir: Option<PathBuf>,
}

impl Default for ReaderConfigWire {
    fn default() -> Self {
        let config = ReaderConfig::default();
        Self {
            show_progress: config.show_progress,
            discreet_mode: None,
            privacy_mode: None,
            mouse_support: config.mouse_support,
            text_width: config.text_width,
            library_dirs: config.library_dirs,
            bookshelf_dir: config.bookshelf_dir,
        }
    }
}

impl<'de> serde::Deserialize<'de> for ReaderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ReaderConfigWire::deserialize(deserializer)?;
        Ok(Self {
            show_progress: wire.show_progress,
            discreet_mode: wire.discreet_mode.or(wire.privacy_mode).unwrap_or(false),
            mouse_support: wire.mouse_support,
            text_width: wire.text_width,
            library_dirs: wire.library_dirs,
            bookshelf_dir: wire.bookshelf_dir,
            path: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_reading_friendly_display_settings() {
        let config = ReaderConfig::default();

        assert!(config.show_progress);
        assert!(!config.discreet_mode);
        assert!(!config.mouse_support);
        assert_eq!(config.text_width, None);
        assert!(config.bookshelf_dir.is_none());
    }

    #[test]
    fn ignores_the_removed_theme_field_and_loads_missing_fields_from_defaults() {
        let config: ReaderConfig = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();

        assert!(config.show_progress);
        assert!(!config.discreet_mode);
        assert!(!config.mouse_support);
        assert_eq!(config.text_width, None);
    }

    #[test]
    fn saves_and_reloads_toggles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("configuration.json");
        let mut config = ReaderConfig::open(path.clone()).unwrap();

        config.toggle_progress();
        config.toggle_discreet_mode();
        config.save().unwrap();

        let loaded = ReaderConfig::open(path).unwrap();
        assert!(!loaded.show_progress);
        assert!(loaded.discreet_mode);
    }

    #[test]
    fn recovers_from_corrupt_configuration_json_by_initializing_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("configuration.json");
        std::fs::write(&path, "not even close to json").unwrap();

        let config = ReaderConfig::open(path.clone()).unwrap();

        // The bad file was moved aside; the original path is gone.
        assert!(!path.exists());
        let backup = path.with_extension("json.corrupt");
        assert!(backup.exists());
        // Defaults are restored.
        assert!(config.show_progress);
        assert!(!config.discreet_mode);
        // And a subsequent save produces a valid file at the original path.
        config.save().unwrap();
        assert!(path.exists());
    }

    #[test]
    fn migrates_legacy_privacy_mode_and_serializes_only_discreet_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("configuration.json");
        std::fs::write(&path, r#"{"privacy_mode":true}"#).unwrap();
        let config = ReaderConfig::open(path.clone()).unwrap();

        assert!(config.discreet_mode);
        config.save().unwrap();
        let serialized = std::fs::read_to_string(path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(value["discreet_mode"], true);
        assert!(value.get("privacy_mode").is_none());
    }

    #[test]
    fn discreet_mode_wins_when_both_config_keys_exist() {
        let config: ReaderConfig =
            serde_json::from_str(r#"{"privacy_mode":true,"discreet_mode":false}"#).unwrap();

        assert!(!config.discreet_mode);
    }

    #[test]
    fn removed_theme_is_not_serialized() {
        let config: ReaderConfig = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        let serialized = serde_json::to_value(config).unwrap();

        assert!(serialized.get("theme").is_none());
    }
}
