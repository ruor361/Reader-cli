use std::path::PathBuf;

use anyhow::{Context, Result};

/// Resolve the reader config directory.
///
/// Prefers `$HOME/.config/reader/` when `$HOME/.config` exists (XDG-style,
/// consistent across macOS and Linux). Falls back to
/// `dirs::config_dir()/reader/` when `HOME` is unset or `~/.config` cannot
/// be resolved (e.g. on Windows).
pub fn config_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        let xdg_dir = PathBuf::from(home).join(".config").join("reader");
        if xdg_dir.parent().is_some_and(|p| p.exists()) {
            return Ok(xdg_dir);
        }
    }
    let dir = dirs::config_dir()
        .with_context(|| "Could not locate a user config directory")?
        .join("reader");
    Ok(dir)
}

pub fn library_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("library.json"))
}

pub fn configuration_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("configuration.json"))
}

pub fn reading_data_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("sync"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_path_ends_with_library_json() {
        let path = library_path().unwrap();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("library.json")
        );
    }

    #[test]
    fn configuration_path_ends_with_configuration_json() {
        let path = configuration_path().unwrap();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("configuration.json")
        );
    }

    #[test]
    fn library_and_configuration_share_parent_dir() {
        let lib = library_path().unwrap();
        let conf = configuration_path().unwrap();
        assert_eq!(lib.parent(), conf.parent());
    }

    #[test]
    fn reading_data_shares_the_config_parent() {
        let data = reading_data_dir().unwrap();
        let conf = configuration_path().unwrap();
        assert_eq!(data.parent(), conf.parent());
    }
}
