use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(crate) fn write(path: &Path, contents: &[u8]) -> Result<()> {
    write_with(path, |file| file.write_all(contents))
}

fn write_with(
    path: &Path,
    write_contents: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<()> {
    let parent = parent_dir(path);
    std::fs::create_dir_all(&parent)
        .with_context(|| format!("Failed to create {}", parent.display()))?;

    let mut temporary = tempfile::NamedTempFile::new_in(&parent)
        .with_context(|| format!("Failed to create a temporary file in {}", parent.display()))?;
    write_contents(temporary.as_file_mut())
        .with_context(|| format!("Failed to write temporary file for {}", path.display()))?;
    temporary
        .as_file_mut()
        .flush()
        .with_context(|| format!("Failed to flush temporary file for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary file for {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "Failed to atomically replace {} with its completed temporary file",
                path.display()
            )
        })?;

    sync_parent_directory(&parent)?;
    Ok(())
}

fn parent_dir(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> Result<()> {
    std::fs::File::open(parent)
        .with_context(|| format!("Failed to open {} for directory sync", parent.display()))?
        .sync_all()
        .with_context(|| format!("Failed to sync directory {}", parent.display()))
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"old").unwrap();

        write(&path, b"new").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn failed_temporary_write_preserves_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"old and valid").unwrap();

        let result = write_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "simulated disk failure",
            ))
        });

        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old and valid");
        let entries = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(entries, 1, "temporary file should be cleaned up");
    }
}
