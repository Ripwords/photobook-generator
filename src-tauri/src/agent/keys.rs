//! API keys for the model providers. Keys live in the macOS keychain and are
//! read only by Rust: no command returns one, so the webview can learn whether
//! a key is set but never its value.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    DeepSeek,
    Jev,
}

impl Provider {
    /// The keychain account name under [`KEYCHAIN_SERVICE`].
    pub fn account(self) -> &'static str {
        match self {
            Provider::DeepSeek => "deepseek",
            Provider::Jev => "jev",
        }
    }

    /// Read only by debug builds, when the keychain has no key.
    pub fn env_var(self) -> &'static str {
        match self {
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::Jev => "TYPESAFE_API_KEY",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::DeepSeek => "DeepSeek",
            Provider::Jev => "Jev",
        }
    }
}

/// The app's bundle identifier, so the entries sit under the app's name in
/// Keychain Access.
pub const KEYCHAIN_SERVICE: &str = "com.jiajingteoh.photobook";

pub trait KeyStore {
    fn get(&self, provider: Provider) -> Result<Option<String>, String>;
    fn set(&self, provider: Provider, key: &str) -> Result<(), String>;
    /// Succeeds when there was nothing to clear.
    fn clear(&self, provider: Provider) -> Result<(), String>;
}

pub struct KeychainStore;

impl KeychainStore {
    fn entry(provider: Provider) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, provider.account()).map_err(|e| e.to_string())
    }
}

impl KeyStore for KeychainStore {
    fn get(&self, provider: Provider) -> Result<Option<String>, String> {
        match Self::entry(provider)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, provider: Provider, key: &str) -> Result<(), String> {
        Self::entry(provider)?.set_password(key).map_err(|e| e.to_string())
    }

    fn clear(&self, provider: Provider) -> Result<(), String> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<Provider, String>>);

impl MemoryStore {
    fn map(&self) -> Result<std::sync::MutexGuard<'_, HashMap<Provider, String>>, String> {
        self.0.lock().map_err(|e| e.to_string())
    }
}

impl KeyStore for MemoryStore {
    fn get(&self, provider: Provider) -> Result<Option<String>, String> {
        Ok(self.map()?.get(&provider).cloned())
    }

    fn set(&self, provider: Provider, key: &str) -> Result<(), String> {
        self.map()?.insert(provider, key.to_string());
        Ok(())
    }

    fn clear(&self, provider: Provider) -> Result<(), String> {
        self.map()?.remove(&provider);
        Ok(())
    }
}

/// What `api_key_status` sends to the webview. Bools only, by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct KeyStatus {
    pub deepseek: bool,
    pub jev: bool,
}

fn non_blank(key: &str) -> Option<&str> {
    Some(key.trim()).filter(|k| !k.is_empty())
}

pub fn set_key(store: &impl KeyStore, provider: Provider, key: &str) -> Result<(), String> {
    let key = non_blank(key).ok_or_else(|| format!("The {} key is empty.", provider.label()))?;
    store.set(provider, key)
}

/// True for a provider when `api_key` would find a key, so a debug build
/// running on the environment fallback does not report "not set".
pub fn status(
    store: &impl KeyStore,
    env: impl Fn(Provider) -> Option<String>,
) -> Result<KeyStatus, String> {
    let has = |provider| lookup(store, provider, &env).map(|key| key.is_some());
    Ok(KeyStatus {
        deepseek: has(Provider::DeepSeek)?,
        jev: has(Provider::Jev)?,
    })
}

fn lookup(
    store: &impl KeyStore,
    provider: Provider,
    env: impl Fn(Provider) -> Option<String>,
) -> Result<Option<String>, String> {
    if let Some(key) = store.get(provider)? {
        return Ok(Some(key));
    }
    Ok(env(provider).and_then(|v| non_blank(&v).map(str::to_string)))
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("No {} API key is set. Add it in Settings.", .0.label())]
    Missing(Provider),
    #[error("{0}")]
    Store(String),
}

pub(crate) fn resolve_key(
    store: &impl KeyStore,
    provider: Provider,
    env: impl Fn(Provider) -> Option<String>,
) -> Result<String, KeyError> {
    lookup(store, provider, env)
        .map_err(KeyError::Store)?
        .ok_or(KeyError::Missing(provider))
}

#[cfg(debug_assertions)]
pub(crate) fn env_fallback(provider: Provider) -> Option<String> {
    std::env::var(provider.env_var()).ok()
}

#[cfg(not(debug_assertions))]
pub(crate) fn env_fallback(_provider: Provider) -> Option<String> {
    None
}

/// The key for `provider`: keychain first, then (debug builds only) the
/// environment.
pub(crate) fn api_key(provider: Provider) -> Result<String, KeyError> {
    resolve_key(&KeychainStore, provider, env_fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: Provider) -> Option<String> {
        None
    }

    #[test]
    fn provider_wire_names() {
        assert_eq!(serde_json::to_string(&Provider::DeepSeek).unwrap(), "\"deepseek\"");
        assert_eq!(serde_json::to_string(&Provider::Jev).unwrap(), "\"jev\"");
        assert_eq!(
            serde_json::from_str::<Provider>("\"deepseek\"").unwrap(),
            Provider::DeepSeek
        );
        assert_eq!(serde_json::from_str::<Provider>("\"jev\"").unwrap(), Provider::Jev);
    }

    #[test]
    fn provider_names_are_distinct() {
        assert_ne!(Provider::DeepSeek.account(), Provider::Jev.account());
        assert_eq!(Provider::DeepSeek.env_var(), "DEEPSEEK_API_KEY");
        assert_eq!(Provider::Jev.env_var(), "TYPESAFE_API_KEY");
    }

    #[test]
    fn set_status_clear_round_trip() {
        let store = MemoryStore::default();
        assert_eq!(
            status(&store, no_env).unwrap(),
            KeyStatus { deepseek: false, jev: false }
        );

        set_key(&store, Provider::Jev, "jev-secret").unwrap();
        assert_eq!(
            status(&store, no_env).unwrap(),
            KeyStatus { deepseek: false, jev: true }
        );
        assert_eq!(resolve_key(&store, Provider::Jev, no_env).unwrap(), "jev-secret");

        set_key(&store, Provider::DeepSeek, "ds-secret").unwrap();
        store.clear(Provider::Jev).unwrap();
        assert_eq!(
            status(&store, no_env).unwrap(),
            KeyStatus { deepseek: true, jev: false }
        );
        assert_eq!(resolve_key(&store, Provider::DeepSeek, no_env).unwrap(), "ds-secret");
    }

    #[test]
    fn clearing_an_absent_key_succeeds() {
        let store = MemoryStore::default();
        store.clear(Provider::DeepSeek).unwrap();
        store.clear(Provider::DeepSeek).unwrap();
    }

    #[test]
    fn empty_or_whitespace_key_is_refused_and_stores_nothing() {
        let store = MemoryStore::default();
        for bad in ["", "   ", "\t\n "] {
            assert!(set_key(&store, Provider::DeepSeek, bad).is_err(), "{bad:?} accepted");
        }
        assert_eq!(store.get(Provider::DeepSeek).unwrap(), None);
    }

    #[test]
    fn refusing_a_blank_key_keeps_the_old_one() {
        let store = MemoryStore::default();
        set_key(&store, Provider::DeepSeek, "ds-secret").unwrap();
        assert!(set_key(&store, Provider::DeepSeek, "  ").is_err());
        assert_eq!(store.get(Provider::DeepSeek).unwrap().as_deref(), Some("ds-secret"));
    }

    #[test]
    fn key_is_trimmed_before_storing() {
        let store = MemoryStore::default();
        set_key(&store, Provider::DeepSeek, "  sk-abc\n").unwrap();
        assert_eq!(store.get(Provider::DeepSeek).unwrap().as_deref(), Some("sk-abc"));
    }

    #[test]
    fn status_serialises_to_bools_only() {
        let store = MemoryStore::default();
        set_key(&store, Provider::DeepSeek, "sk-must-not-leak").unwrap();
        let json = serde_json::to_value(status(&store, no_env).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({ "deepseek": true, "jev": false }));
    }

    #[test]
    fn keychain_wins_over_env() {
        let store = MemoryStore::default();
        set_key(&store, Provider::DeepSeek, "from-keychain").unwrap();
        let env = |_| Some("from-env".to_string());
        assert_eq!(resolve_key(&store, Provider::DeepSeek, env).unwrap(), "from-keychain");
    }

    #[test]
    fn env_fills_in_when_keychain_is_empty() {
        let store = MemoryStore::default();
        let env = |p| (p == Provider::Jev).then(|| " from-env \n".to_string());
        assert_eq!(resolve_key(&store, Provider::Jev, env).unwrap(), "from-env");
        assert!(resolve_key(&store, Provider::DeepSeek, env).is_err());
        assert_eq!(
            status(&store, env).unwrap(),
            KeyStatus { deepseek: false, jev: true }
        );
    }

    #[test]
    fn blank_env_value_counts_as_unset() {
        let store = MemoryStore::default();
        let env = |_| Some("   ".to_string());
        assert!(resolve_key(&store, Provider::Jev, env).is_err());
        assert_eq!(
            status(&store, env).unwrap(),
            KeyStatus { deepseek: false, jev: false }
        );
    }

    #[test]
    fn missing_key_error_names_the_provider() {
        let store = MemoryStore::default();
        let err = resolve_key(&store, Provider::DeepSeek, no_env).unwrap_err();
        assert_eq!(err, KeyError::Missing(Provider::DeepSeek));
        assert!(err.to_string().contains("DeepSeek"), "{err}");
        let err = resolve_key(&store, Provider::Jev, no_env).unwrap_err();
        assert!(err.to_string().contains("Jev"), "{err}");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_builds_read_the_env_fallback() {
        // No other test reads the real environment, so setting this variable
        // cannot race with the parallel harness.
        let var = Provider::Jev.env_var();
        let before = std::env::var(var).ok();
        std::env::set_var(var, "  jev-from-env ");
        let read = env_fallback(Provider::Jev);
        match before {
            Some(v) => std::env::set_var(var, v),
            None => std::env::remove_var(var),
        }
        assert_eq!(read.as_deref(), Some("  jev-from-env "));
    }
}
