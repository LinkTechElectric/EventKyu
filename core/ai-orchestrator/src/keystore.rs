use crate::Result;
use std::collections::HashMap;

/// Application-provided storage for BYOK credentials.
///
/// Production implementations should use operating-system protected storage,
/// such as macOS Keychain or Windows DPAPI. This crate deliberately contains
/// no plaintext-file credential persistence.
pub trait KeyStore {
    /// Retrieves a provider key, or `None` when no key is stored.
    fn get(&self, provider: &str) -> Result<Option<String>>;

    /// Stores or replaces a provider key in protected application storage.
    fn set(&mut self, provider: &str, key: &str) -> Result<()>;

    /// Deletes a provider key and returns whether one was present.
    fn delete(&mut self, provider: &str) -> Result<bool>;
}

/// Volatile in-memory key storage intended for tests and ephemeral sessions.
///
/// Its custom `Debug` implementation reports only the number of entries, not
/// provider names or secret values.
#[derive(Default)]
pub struct InMemoryKeyStore {
    keys: HashMap<String, String>,
}

impl InMemoryKeyStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for InMemoryKeyStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InMemoryKeyStore")
            .field("entries", &self.keys.len())
            .finish_non_exhaustive()
    }
}

impl KeyStore for InMemoryKeyStore {
    fn get(&self, provider: &str) -> Result<Option<String>> {
        Ok(self.keys.get(provider).cloned())
    }

    fn set(&mut self, provider: &str, key: &str) -> Result<()> {
        self.keys.insert(provider.to_owned(), key.to_owned());
        Ok(())
    }

    fn delete(&mut self, provider: &str) -> Result<bool> {
        Ok(self.keys.remove(provider).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_supports_get_set_delete_without_debugging_secrets() {
        let mut store = InMemoryKeyStore::new();
        assert_eq!(store.get("openai").unwrap(), None);
        store.set("openai", "secret-value").unwrap();
        assert_eq!(
            store.get("openai").unwrap(),
            Some("secret-value".to_owned())
        );
        assert!(!format!("{store:?}").contains("secret-value"));
        assert!(store.delete("openai").unwrap());
        assert!(!store.delete("openai").unwrap());
        assert_eq!(store.get("openai").unwrap(), None);
    }
}
