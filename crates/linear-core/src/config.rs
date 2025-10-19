use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use thiserror::Error;

/// Application-specific configuration helpers.
#[derive(Debug, Clone)]
pub struct ConfigLocator {
    root: PathBuf,
}

impl ConfigLocator {
    /// Attempt to discover the persistent configuration directory, creating it if needed.
    pub fn new() -> Result<Self, ConfigError> {
        let dirs = ProjectDirs::from("app", "linear", "linear-rs")
            .ok_or(ConfigError::MissingProjectDirs)?;
        let config_dir = dirs.config_dir();
        fs::create_dir_all(config_dir).map_err(ConfigError::CreateDir)?;
        set_user_only_permissions(config_dir)?;
        Ok(Self {
            root: config_dir.to_path_buf(),
        })
    }

    /// Path to the credentials file for the given profile.
    pub fn credentials_file(&self, profile: &str) -> PathBuf {
        self.root.join(format!("credentials-{profile}.json"))
    }

    #[cfg(test)]
    pub(crate) fn from_root_for_tests(root: PathBuf) -> Self {
        Self { root }
    }
}

fn set_user_only_permissions(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        let metadata = fs::metadata(path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Errors that can occur when working with configuration directories.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("unable to determine configuration directory for linear-rs")]
    MissingProjectDirs,
    #[error("failed to create configuration directory: {0}")]
    CreateDir(#[source] std::io::Error),
    #[error("filesystem error: {0}")]
    Io(#[source] std::io::Error),
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn credentials_file_appends_profile() {
        let temp_dir = TempDir::new().unwrap();
        let locator = ConfigLocator {
            root: temp_dir.path().to_path_buf(),
        };
        let path = locator.credentials_file("default");
        assert!(path.ends_with("credentials-default.json"));
    }
}
