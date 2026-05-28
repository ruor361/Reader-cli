use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ReaderConfig {
    pub theme: Theme,
    pub show_progress: bool,
    pub mouse_support: bool,
    pub text_width: Option<u16>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            show_progress: true,
            mouse_support: false,
            text_width: None,
            path: None,
        }
    }
}

impl ReaderConfig {
    pub fn open_default() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .with_context(|| "Could not locate a user config directory")?
            .join("reader");
        Self::open(config_dir.join("configuration.json"))
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        let mut config = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            serde_json::from_str(&text).with_context(|| format!("Invalid {}", path.display()))?
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

    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };
    }

    pub fn toggle_progress(&mut self) {
        self.show_progress = !self.show_progress;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_reading_friendly_display_settings() {
        let config = ReaderConfig::default();

        assert_eq!(config.theme, Theme::Dark);
        assert!(config.show_progress);
        assert!(!config.mouse_support);
        assert_eq!(config.text_width, None);
    }

    #[test]
    fn loads_missing_fields_from_defaults() {
        let config: ReaderConfig = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();

        assert_eq!(config.theme, Theme::Light);
        assert!(config.show_progress);
        assert!(!config.mouse_support);
        assert_eq!(config.text_width, None);
    }

    #[test]
    fn saves_and_reloads_toggles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("configuration.json");
        let mut config = ReaderConfig::open(path.clone()).unwrap();

        config.toggle_theme();
        config.toggle_progress();
        config.save().unwrap();

        let loaded = ReaderConfig::open(path).unwrap();
        assert_eq!(loaded.theme, Theme::Light);
        assert!(!loaded.show_progress);
    }
}
