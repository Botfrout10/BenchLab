//! Secret storage. Values never enter SQLite, snapshots, logs, or exports.
//!
//! The keychain service is `dev.benchlab.app`; accounts are
//! `secret:<secret_ref_id>` (SPEC.md §11.6). Minimal WSL/Linux environments may
//! not expose a Secret Service; in that case BenchLab falls back to memory for
//! the current session and says so in the UI.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use benchlab_core::Id;
use secrecy::SecretString;
use sha2::{Digest, Sha256};

pub const KEYRING_SERVICE: &str = "dev.benchlab.app";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretPersistence {
    Keychain,
    SessionOnly,
}

impl SecretPersistence {
    pub fn label(self) -> &'static str {
        match self {
            SecretPersistence::Keychain => "OS keychain",
            SecretPersistence::SessionOnly => "Session only",
        }
    }

    pub fn is_persistent(self) -> bool {
        matches!(self, SecretPersistence::Keychain)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("the OS keychain is unavailable: {0}")]
    Keychain(String),
    #[error("secret was not found")]
    NotFound,
}

pub trait SecretStore: Send + Sync {
    fn persistence(&self) -> SecretPersistence;
    fn set(&self, id: Id, value: SecretString) -> Result<(), SecretError>;
    fn get(&self, id: Id) -> Result<Option<SecretString>, SecretError>;
    fn delete(&self, id: Id) -> Result<(), SecretError>;
}

/// Use the platform keychain when it initializes, otherwise keep secrets in
/// memory for this session only.
pub fn default_secret_store() -> Arc<dyn SecretStore> {
    if KeyringSecretStore::available() {
        Arc::new(KeyringSecretStore)
    } else {
        tracing::warn!("OS keychain unavailable; using session-only secret storage");
        Arc::new(SessionSecretStore::default())
    }
}

pub struct KeyringSecretStore;

impl KeyringSecretStore {
    pub fn available() -> bool {
        keyring::Entry::store_status().is_ok()
    }

    fn entry(id: Id) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(KEYRING_SERVICE, &format!("secret:{id}"))
            .map_err(|error| SecretError::Keychain(error.to_string()))
    }
}

impl SecretStore for KeyringSecretStore {
    fn persistence(&self) -> SecretPersistence {
        SecretPersistence::Keychain
    }

    fn set(&self, id: Id, value: SecretString) -> Result<(), SecretError> {
        use secrecy::ExposeSecret;
        Self::entry(id)?
            .set_password(value.expose_secret())
            .map_err(|error| SecretError::Keychain(error.to_string()))
    }

    fn get(&self, id: Id) -> Result<Option<SecretString>, SecretError> {
        match Self::entry(id)?.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretError::Keychain(error.to_string())),
        }
    }

    fn delete(&self, id: Id) -> Result<(), SecretError> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretError::Keychain(error.to_string())),
        }
    }
}

#[derive(Default)]
pub struct SessionSecretStore {
    values: Mutex<HashMap<Id, SecretString>>,
}

impl SecretStore for SessionSecretStore {
    fn persistence(&self) -> SecretPersistence {
        SecretPersistence::SessionOnly
    }

    fn set(&self, id: Id, value: SecretString) -> Result<(), SecretError> {
        self.values.lock().unwrap().insert(id, value);
        Ok(())
    }

    fn get(&self, id: Id) -> Result<Option<SecretString>, SecretError> {
        Ok(self.values.lock().unwrap().get(&id).cloned())
    }

    fn delete(&self, id: Id) -> Result<(), SecretError> {
        self.values.lock().unwrap().remove(&id);
        Ok(())
    }
}

/// SHA-256 of the value, first 8 lowercase hex characters.
pub fn fingerprint(value: &SecretString) -> String {
    use secrecy::ExposeSecret;
    let digest = Sha256::digest(value.expose_secret().as_bytes());
    format!("{digest:x}").chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn session_store_round_trips_and_deletes() {
        let store = SessionSecretStore::default();
        let id = Id::now_v7();
        store
            .set(id, SecretString::from("hunter2".to_string()))
            .unwrap();
        assert_eq!(store.get(id).unwrap().unwrap().expose_secret(), "hunter2");
        store.delete(id).unwrap();
        assert!(store.get(id).unwrap().is_none());
    }

    #[test]
    fn keyring_store_round_trips_when_available() {
        if !KeyringSecretStore::available() {
            return;
        }
        let store = KeyringSecretStore;
        let id = Id::now_v7();
        let value = SecretString::from("benchlab-keyring-test".to_string());
        store.set(id, value.clone()).unwrap();
        assert_eq!(
            store.get(id).unwrap().unwrap().expose_secret(),
            value.expose_secret()
        );
        store.delete(id).unwrap();
        assert!(store.get(id).unwrap().is_none());
    }

    #[test]
    fn fingerprints_are_stable_and_short() {
        let value = SecretString::from("hunter2".to_string());
        let first = fingerprint(&value);
        assert_eq!(first.len(), 8);
        assert_eq!(first, fingerprint(&value));
        assert_ne!(
            first,
            fingerprint(&SecretString::from("different".to_string()))
        );
    }
}
