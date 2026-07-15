use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::ui::Theme;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ReaderConfig {
    pub theme: Theme,
    pub show_progress: bool,
    pub privacy_mode: bool,
    pub mouse_support: bool,
    pub text_width: Option<u16>,
    pub library_dirs: Vec<PathBuf>,
    /// Bookshelf directory. When set, files added via `add` are copied into
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
            theme: Theme::Dark,
            show_progress: true,
            privacy_mode: false,
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn toggle_progress(&mut self) {
        self.show_progress = !self.show_progress;
    }

    pub fn toggle_privacy_mode(&mut self) {
        self.privacy_mode = !self.privacy_mode;
    }

    pub fn toggle_theme(&mut self) {
        self.theme = self.theme.toggled();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_reading_friendly_display_settings() {
        let config = ReaderConfig::default();

        assert!(config.show_progress);
        assert_eq!(config.theme, Theme::Dark);
        assert!(!config.privacy_mode);
        assert!(!config.mouse_support);
        assert_eq!(config.text_width, None);
        assert!(config.bookshelf_dir.is_none());
    }

    #[test]
    fn loads_missing_fields_from_defaults() {
        let config: ReaderConfig = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();

        assert!(config.show_progress);
        assert_eq!(config.theme, Theme::Light);
        assert!(!config.privacy_mode);
        assert!(!config.mouse_support);
        assert_eq!(config.text_width, None);
    }

    #[test]
    fn saves_and_reloads_toggles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("configuration.json");
        let mut config = ReaderConfig::open(path.clone()).unwrap();

        config.toggle_progress();
        config.toggle_privacy_mode();
        config.toggle_theme();
        config.save().unwrap();

        let loaded = ReaderConfig::open(path).unwrap();
        assert!(!loaded.show_progress);
        assert!(loaded.privacy_mode);
        assert_eq!(loaded.theme, Theme::Light);
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
        assert!(!config.privacy_mode);
        // And a subsequent save produces a valid file at the original path.
        config.save().unwrap();
        assert!(path.exists());
    }
}
