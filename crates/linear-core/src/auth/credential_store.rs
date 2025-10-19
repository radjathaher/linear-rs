use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::ConfigLocator;

use super::{AuthError, AuthSession};

/// Persistence abstraction for authentication credentials.
pub trait CredentialStore {
    fn load(&self, profile: &str) -> Result<Option<AuthSession>, AuthError>;
    fn save(&self, profile: &str, session: &AuthSession) -> Result<(), AuthError>;
    fn delete(&self, profile: &str) -> Result<(), AuthError>;
}

/// Factory trait allowing higher-level components to obtain a credential store.
pub trait CredentialStoreFactory: Send + Sync {
    fn open(&self) -> Result<Box<dyn CredentialStore + Send + Sync>, AuthError>;
}

/// Filesystem-backed credential storage located in the user configuration directory.
pub struct FileCredentialStore {
    locator: ConfigLocator,
}

impl FileCredentialStore {
    pub fn new(locator: ConfigLocator) -> Self {
        Self { locator }
    }

    pub fn with_default_locator() -> Result<Self, AuthError> {
        Ok(Self::new(ConfigLocator::new()?))
    }

    fn write_file(path: &Path, payload: &str) -> Result<(), AuthError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        file.write_all(payload.as_bytes())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = file.metadata()?.permissions();
            perm.set_mode(0o600);
            fs::set_permissions(path, perm)?;
        }

        Ok(())
    }
}

impl CredentialStore for FileCredentialStore {
    fn load(&self, profile: &str) -> Result<Option<AuthSession>, AuthError> {
        let path = self.locator.credentials_file(profile);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path)?;
        let envelope: SessionEnvelope = serde_json::from_str(&raw)?;
        Ok(Some(envelope.session))
    }

    fn save(&self, profile: &str, session: &AuthSession) -> Result<(), AuthError> {
        let path = self.locator.credentials_file(profile);
        let envelope = SessionEnvelope {
            profile: profile.to_owned(),
            session: session.clone(),
            version: 1,
        };
        let payload = serde_json::to_string_pretty(&envelope)?;
        Self::write_file(&path, &payload)
    }

    fn delete(&self, profile: &str) -> Result<(), AuthError> {
        let path = self.locator.credentials_file(profile);
        match fs::remove_file(path) {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionEnvelope {
    version: u32,
    profile: String,
    session: AuthSession,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    fn sample_session() -> AuthSession {
        AuthSession::new_access_token(
            "token".into(),
            Some("refresh".into()),
            Utc::now() + Duration::minutes(5),
            vec!["read".into()],
        )
    }

    #[test]
    fn round_trip_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let locator = ConfigLocator::from_root_for_tests(temp_dir.path().to_path_buf());
        let store = FileCredentialStore::new(locator);
        let session = sample_session();
        store.save("default", &session).unwrap();
        let loaded = store.load("default").unwrap().unwrap();
        assert_eq!(loaded.access_token, session.access_token);
        assert_eq!(loaded.refresh_token, session.refresh_token);
    }

    #[test]
    fn delete_missing_is_ok() {
        let temp_dir = TempDir::new().unwrap();
        let locator = ConfigLocator::from_root_for_tests(temp_dir.path().to_path_buf());
        let store = FileCredentialStore::new(locator);
        store.delete("missing").unwrap();
    }
}
