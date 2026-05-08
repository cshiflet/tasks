//! Persistent OAuth token + account-password store.
//!
//! Three tiers, picked at runtime:
//!
//! 1. [`KeychainTokenStore`] — wraps the `keyring` crate.
//!    Backends: libsecret (Linux), Keychain (macOS), Credential
//!    Manager (Windows). Tokens never touch our filesystem.
//!
//! 2. [`EncryptedFileTokenStore`] — AES-256-GCM frames in a
//!    file at `<config_dir>/tasks-desktop/tokens.dat`. The key
//!    is HKDF-derived from a stable per-machine identifier
//!    (`/etc/machine-id` on Linux, IORegistry UUID on macOS,
//!    MachineGuid on Windows) plus an app salt. Anyone who can
//!    read that file *and* the machine-id source can decrypt;
//!    we surface that limit explicitly to the user.
//!
//! 3. `tasks_sync::InMemoryTokenStore` — falls through here
//!    when both above are unavailable (typically: a Linux box
//!    with no D-Bus session AND no machine-id file). Tokens
//!    don't survive a restart.
//!
//! Both [`TokenStore`] (OAuth tokens) and [`SecretStore`]
//! (CalDAV / EteSync passwords) live in the same storage tier.
//! [`StorageTier::probe`] returns the trio (tier name, token
//! store, secret store), all backed by the same medium so the
//! pre-flight dialog can talk about "credentials" generically.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tasks_sync::{OAuthTokens, ProviderKind, TokenStore, TokenStoreError};

/// Service identifier used in the OS-keychain `Entry::new` calls
/// and in the salt prefix on the Tier-2 HKDF. Keep stable: changing
/// it migrates everyone to a fresh slot and forces re-sign-in.
const SERVICE: &str = "org.tasks.desktop.oauth";
/// Key prefix for password (CalDAV / EteSync) entries inside
/// the same store the OAuth tokens use. Distinct from the
/// provider-coded prefix tokens use so a single account can
/// have both a token AND a password slot without collision.
const PASSWORD_PREFIX: &str = "pwd:";

/// Generic "store an opaque string secret keyed by an account
/// uuid" surface. Used for CalDAV / EteSync passwords; their
/// sister `cda_password` column on `caldav_accounts` becomes a
/// sentinel (empty) once a row is migrated into the secret
/// store.
pub trait SecretStore: Send + Sync {
    fn put_secret(&self, account_uuid: &str, secret: &str) -> Result<(), TokenStoreError>;
    fn get_secret(&self, account_uuid: &str) -> Option<String>;
    fn delete_secret(&self, account_uuid: &str) -> Result<(), TokenStoreError>;
}

/// Which storage tier the running process is using. Surfaced to
/// QML via a bridge invokable so the pre-flight dialog can
/// explain the trade-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageTier {
    /// Tier 1 — OS-native keychain.
    Keychain,
    /// Tier 2 — encrypted file under the config dir.
    EncryptedFile,
    /// Tier 3 — in-memory only; tokens vanish on restart.
    InMemory,
}

impl StorageTier {
    /// Stable string identifier used over the QML bridge.
    pub fn as_str(self) -> &'static str {
        match self {
            StorageTier::Keychain => "keychain",
            StorageTier::EncryptedFile => "encrypted_file",
            StorageTier::InMemory => "in_memory",
        }
    }

    /// Probe each tier in order and return the first that's
    /// healthy. Always returns `Ok` — at worst we fall back to
    /// in-memory.
    pub fn probe() -> (Self, Arc<dyn TokenStore>, Arc<dyn SecretStore>) {
        match KeychainTokenStore::probe() {
            Ok(s) => {
                let arc: Arc<KeychainTokenStore> = Arc::new(s);
                return (
                    StorageTier::Keychain,
                    arc.clone() as Arc<dyn TokenStore>,
                    arc as Arc<dyn SecretStore>,
                );
            }
            Err(e) => tracing::info!("token store: keychain unavailable: {e}"),
        }
        match EncryptedFileTokenStore::probe() {
            Ok(s) => {
                let arc: Arc<EncryptedFileTokenStore> = Arc::new(s);
                return (
                    StorageTier::EncryptedFile,
                    arc.clone() as Arc<dyn TokenStore>,
                    arc as Arc<dyn SecretStore>,
                );
            }
            Err(e) => tracing::warn!("token store: encrypted-file fallback unavailable: {e}"),
        }
        tracing::warn!("token store: degraded to in-memory; credentials will not survive restart");
        let mem = Arc::new(InMemorySecretStore::new());
        (
            StorageTier::InMemory,
            Arc::new(tasks_sync::InMemoryTokenStore::new()),
            mem,
        )
    }
}

fn key_for(provider: ProviderKind, account_uuid: &str) -> String {
    format!("{}:{account_uuid}", provider.code())
}

fn password_key_for(account_uuid: &str) -> String {
    format!("{PASSWORD_PREFIX}{account_uuid}")
}

/// Helper trait so we don't have to repeat the same enum→code
/// mapping in three places.
trait ProviderCode {
    fn code(&self) -> &'static str;
}
impl ProviderCode for ProviderKind {
    fn code(&self) -> &'static str {
        match self {
            ProviderKind::CalDav => "caldav",
            ProviderKind::EteSync => "etesync",
            ProviderKind::GoogleTasks => "google",
            ProviderKind::MicrosoftToDo => "microsoft",
        }
    }
}

// ---------- Tier 1: OS keychain ----------

pub struct KeychainTokenStore {
    /// Cache of in-flight reads. The `keyring` crate hits the OS
    /// for every `get`, which can be slow on first call (libsecret
    /// startup, prompting); cache to keep the per-account load
    /// snappy after the first hit.
    cache: Mutex<std::collections::HashMap<String, OAuthTokens>>,
}

impl KeychainTokenStore {
    /// Probe whether the backend is functional. Writes + reads a
    /// throwaway entry under a probe key; on success the entry is
    /// deleted. Surfacing setup errors here keeps them out of the
    /// hot OAuth path.
    pub fn probe() -> Result<Self, String> {
        let entry =
            keyring::Entry::new(SERVICE, "__probe__").map_err(|e| format!("Entry::new: {e}"))?;
        // Some backends (libsecret) require the secret to be set
        // before get_password works; round-trip a sentinel value.
        entry
            .set_password("ok")
            .map_err(|e| format!("set_password: {e}"))?;
        let v = entry
            .get_password()
            .map_err(|e| format!("get_password: {e}"))?;
        if v != "ok" {
            return Err(format!("probe round-trip mismatch ({v})"));
        }
        let _ = entry.delete_credential();
        Ok(Self {
            cache: Mutex::new(std::collections::HashMap::new()),
        })
    }
}

impl TokenStore for KeychainTokenStore {
    fn put(
        &self,
        provider: ProviderKind,
        account_uuid: &str,
        tokens: &OAuthTokens,
    ) -> Result<(), TokenStoreError> {
        let key = key_for(provider, account_uuid);
        let json = serde_json::to_string(&PersistedTokens::from(tokens))
            .map_err(|e| TokenStoreError::Backend(format!("serialise: {e}")))?;
        let entry = keyring::Entry::new(SERVICE, &key)
            .map_err(|e| TokenStoreError::Backend(format!("Entry::new: {e}")))?;
        entry
            .set_password(&json)
            .map_err(|e| TokenStoreError::Backend(format!("set_password: {e}")))?;
        if let Ok(mut g) = self.cache.lock() {
            g.insert(key, tokens.clone());
        }
        Ok(())
    }

    fn get(&self, provider: ProviderKind, account_uuid: &str) -> Option<OAuthTokens> {
        let key = key_for(provider, account_uuid);
        if let Ok(g) = self.cache.lock() {
            if let Some(v) = g.get(&key) {
                return Some(v.clone());
            }
        }
        let entry = keyring::Entry::new(SERVICE, &key).ok()?;
        let s = entry.get_password().ok()?;
        let pt: PersistedTokens = serde_json::from_str(&s).ok()?;
        let tokens: OAuthTokens = pt.into();
        if let Ok(mut g) = self.cache.lock() {
            g.insert(key, tokens.clone());
        }
        Some(tokens)
    }

    fn delete(&self, provider: ProviderKind, account_uuid: &str) -> Result<(), TokenStoreError> {
        let key = key_for(provider, account_uuid);
        let entry = keyring::Entry::new(SERVICE, &key)
            .map_err(|e| TokenStoreError::Backend(format!("Entry::new: {e}")))?;
        // delete_credential returns NoEntry when nothing's there;
        // treat as a no-op success rather than an error.
        let _ = entry.delete_credential();
        if let Ok(mut g) = self.cache.lock() {
            g.remove(&key);
        }
        Ok(())
    }
}

// ---------- Tier 2: encrypted file ----------

/// File format version. Bump only when on-disk layout changes
/// in an incompatible way. v1 keeps the master-password slot
/// open via the [`KdfMarker`] byte; older readers refuse to
/// load v2+ blobs.
const FILE_FORMAT_VERSION: u8 = 1;

/// One byte at the start of every file identifying which key
/// derivation produced the AES-256-GCM key.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KdfMarker {
    /// Key = HKDF-SHA256(machine-id, app-salt). No prompt.
    MachineId = 0,
    /// Key = Argon2id(master-password, file-salt). Prompted on
    /// each launch. Reserved; not implemented in this batch.
    #[allow(dead_code)]
    MasterPassword = 1,
}

impl KdfMarker {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(KdfMarker::MachineId),
            1 => Some(KdfMarker::MasterPassword),
            _ => None,
        }
    }
}

/// File layout (concatenated bytes), v1:
///   [1 byte: file-format version (1)]
///   [1 byte: kdf marker (0=machine-id, 1=master-password)]
///   [16 bytes: per-file salt — random on first write; mixed
///              into the KDF on every read]
///   [12 bytes: AES-GCM nonce]
///   [N bytes:  AES-GCM ciphertext + 16-byte tag]
///
/// We rewrite the whole file on every put/delete; the blob is
/// tiny (a handful of accounts × a few hundred bytes each).
pub struct EncryptedFileTokenStore {
    path: PathBuf,
    cipher: Aes256Gcm,
    /// Salt baked into the file at first write. Persisted between
    /// launches. Re-keying (e.g. enabling master password later)
    /// generates a fresh salt and re-encrypts.
    salt: [u8; 16],
    /// In-memory cache for both OAuth tokens and arbitrary
    /// secrets. Keys are namespaced (`<provider>:<uuid>` for
    /// tokens, `pwd:<uuid>` for password secrets) so a single
    /// HashMap covers both surfaces.
    tokens: Mutex<std::collections::HashMap<String, OAuthTokens>>,
    secrets: Mutex<std::collections::HashMap<String, String>>,
}

impl EncryptedFileTokenStore {
    pub fn probe() -> Result<Self, String> {
        let path = file_path().ok_or_else(|| "no config dir".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
        }
        // Read existing file (if any) to recover its salt. A
        // fresh install gets a random salt + empty maps.
        let (salt, tokens, secrets) = match std::fs::read(&path) {
            Ok(bytes) if !bytes.is_empty() => {
                decode_file(&bytes).map_err(|e| format!("decrypt {}: {e}", path.display()))?
            }
            _ => {
                let mut salt = [0u8; 16];
                getrandom::getrandom(&mut salt).map_err(|e| format!("getrandom: {e}"))?;
                (salt, Default::default(), Default::default())
            }
        };
        let key = derive_machine_id_key(&salt)?;
        let cipher = Aes256Gcm::new(&key);
        Ok(Self {
            path,
            cipher,
            salt,
            tokens: Mutex::new(tokens),
            secrets: Mutex::new(secrets),
        })
    }

    fn flush(&self) -> std::io::Result<()> {
        let tokens = self
            .tokens
            .lock()
            .map_err(|e| std::io::Error::other(format!("lock: {e}")))?
            .clone();
        let secrets = self
            .secrets
            .lock()
            .map_err(|e| std::io::Error::other(format!("lock: {e}")))?
            .clone();
        let bytes = encode_file(
            &self.cipher,
            KdfMarker::MachineId,
            &self.salt,
            &tokens,
            &secrets,
        )
        .map_err(|e| std::io::Error::other(format!("encrypt: {e}")))?;
        write_atomic(&self.path, &bytes)
    }
}

impl TokenStore for EncryptedFileTokenStore {
    fn put(
        &self,
        provider: ProviderKind,
        account_uuid: &str,
        tokens: &OAuthTokens,
    ) -> Result<(), TokenStoreError> {
        let key = key_for(provider, account_uuid);
        {
            let mut g = self
                .tokens
                .lock()
                .map_err(|e| TokenStoreError::Backend(format!("lock poisoned: {e}")))?;
            g.insert(key, tokens.clone());
        }
        self.flush()
            .map_err(|e| TokenStoreError::Backend(format!("write {}: {e}", self.path.display())))
    }

    fn get(&self, provider: ProviderKind, account_uuid: &str) -> Option<OAuthTokens> {
        let key = key_for(provider, account_uuid);
        self.tokens.lock().ok()?.get(&key).cloned()
    }

    fn delete(&self, provider: ProviderKind, account_uuid: &str) -> Result<(), TokenStoreError> {
        let key = key_for(provider, account_uuid);
        let removed = {
            let mut g = self
                .tokens
                .lock()
                .map_err(|e| TokenStoreError::Backend(format!("lock poisoned: {e}")))?;
            g.remove(&key).is_some()
        };
        if removed {
            self.flush().map_err(|e| {
                TokenStoreError::Backend(format!("write {}: {e}", self.path.display()))
            })?;
        }
        Ok(())
    }
}

impl SecretStore for EncryptedFileTokenStore {
    fn put_secret(&self, account_uuid: &str, secret: &str) -> Result<(), TokenStoreError> {
        let key = password_key_for(account_uuid);
        {
            let mut g = self
                .secrets
                .lock()
                .map_err(|e| TokenStoreError::Backend(format!("lock poisoned: {e}")))?;
            g.insert(key, secret.to_string());
        }
        self.flush()
            .map_err(|e| TokenStoreError::Backend(format!("write {}: {e}", self.path.display())))
    }

    fn get_secret(&self, account_uuid: &str) -> Option<String> {
        let key = password_key_for(account_uuid);
        self.secrets.lock().ok()?.get(&key).cloned()
    }

    fn delete_secret(&self, account_uuid: &str) -> Result<(), TokenStoreError> {
        let key = password_key_for(account_uuid);
        let removed = {
            let mut g = self
                .secrets
                .lock()
                .map_err(|e| TokenStoreError::Backend(format!("lock poisoned: {e}")))?;
            g.remove(&key).is_some()
        };
        if removed {
            self.flush().map_err(|e| {
                TokenStoreError::Backend(format!("write {}: {e}", self.path.display()))
            })?;
        }
        Ok(())
    }
}

impl SecretStore for KeychainTokenStore {
    fn put_secret(&self, account_uuid: &str, secret: &str) -> Result<(), TokenStoreError> {
        let key = password_key_for(account_uuid);
        let entry = keyring::Entry::new(SERVICE, &key)
            .map_err(|e| TokenStoreError::Backend(format!("Entry::new: {e}")))?;
        entry
            .set_password(secret)
            .map_err(|e| TokenStoreError::Backend(format!("set_password: {e}")))?;
        Ok(())
    }

    fn get_secret(&self, account_uuid: &str) -> Option<String> {
        let key = password_key_for(account_uuid);
        let entry = keyring::Entry::new(SERVICE, &key).ok()?;
        entry.get_password().ok()
    }

    fn delete_secret(&self, account_uuid: &str) -> Result<(), TokenStoreError> {
        let key = password_key_for(account_uuid);
        let entry = keyring::Entry::new(SERVICE, &key)
            .map_err(|e| TokenStoreError::Backend(format!("Entry::new: {e}")))?;
        let _ = entry.delete_credential();
        Ok(())
    }
}

/// In-memory secret store paired with `tasks_sync::InMemoryTokenStore`
/// when both Tier 1 and Tier 2 are unavailable.
pub struct InMemorySecretStore {
    inner: Mutex<std::collections::HashMap<String, String>>,
}

impl InMemorySecretStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl SecretStore for InMemorySecretStore {
    fn put_secret(&self, account_uuid: &str, secret: &str) -> Result<(), TokenStoreError> {
        self.inner
            .lock()
            .map_err(|e| TokenStoreError::Backend(format!("lock poisoned: {e}")))?
            .insert(account_uuid.to_string(), secret.to_string());
        Ok(())
    }

    fn get_secret(&self, account_uuid: &str) -> Option<String> {
        self.inner.lock().ok()?.get(account_uuid).cloned()
    }

    fn delete_secret(&self, account_uuid: &str) -> Result<(), TokenStoreError> {
        self.inner
            .lock()
            .map_err(|e| TokenStoreError::Backend(format!("lock poisoned: {e}")))?
            .remove(account_uuid);
        Ok(())
    }
}

fn file_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("tasks-desktop");
    p.push("tokens.dat");
    Some(p)
}

fn derive_machine_id_key(per_file_salt: &[u8; 16]) -> Result<Key<Aes256Gcm>, String> {
    let mid = machine_uid::get().map_err(|e| format!("machine-uid: {e}"))?;
    // Salt = per-file random bytes ‖ app constant. Mixing both
    // means rotating the file's salt rotates the key (lets a
    // future "re-key" path generate a fresh nonce salt without
    // needing a new machine id).
    let mut salt = Vec::with_capacity(per_file_salt.len() + 32);
    salt.extend_from_slice(per_file_salt);
    salt.extend_from_slice(b"org.tasks.desktop.oauth/v1");
    let hk = Hkdf::<Sha256>::new(Some(&salt), mid.as_bytes());
    let mut out = [0u8; 32];
    hk.expand(b"token-store-key", &mut out)
        .map_err(|e| format!("hkdf expand: {e}"))?;
    Ok(Key::<Aes256Gcm>::from_slice(&out).to_owned())
}

#[derive(Serialize, Deserialize)]
struct PersistedTokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_ms: i64,
}

impl From<&OAuthTokens> for PersistedTokens {
    fn from(t: &OAuthTokens) -> Self {
        use secrecy::ExposeSecret;
        Self {
            access_token: t.access_token.expose_secret().to_string(),
            refresh_token: t
                .refresh_token
                .as_ref()
                .map(|s| s.expose_secret().to_string()),
            expires_at_ms: t.expires_at_ms,
        }
    }
}

impl From<PersistedTokens> for OAuthTokens {
    fn from(p: PersistedTokens) -> Self {
        use secrecy::SecretString;
        OAuthTokens {
            access_token: SecretString::from(p.access_token),
            refresh_token: p.refresh_token.map(SecretString::from),
            expires_at_ms: p.expires_at_ms,
        }
    }
}

/// Plaintext payload serialised inside the AES-GCM frame.
/// Splits OAuth tokens from password secrets so a single
/// decode call rebuilds both maps.
#[derive(Default, Serialize, Deserialize)]
struct StorePayload {
    #[serde(default)]
    tokens: std::collections::HashMap<String, PersistedTokens>,
    #[serde(default)]
    secrets: std::collections::HashMap<String, String>,
}

fn encode_file(
    cipher: &Aes256Gcm,
    kdf: KdfMarker,
    salt: &[u8; 16],
    tokens: &std::collections::HashMap<String, OAuthTokens>,
    secrets: &std::collections::HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    let payload = StorePayload {
        tokens: tokens.iter().map(|(k, v)| (k.clone(), v.into())).collect(),
        secrets: secrets.clone(),
    };
    let plain = serde_json::to_vec(&payload).map_err(|e| format!("serialise: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| format!("getrandom: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plain.as_ref())
        .map_err(|e| format!("encrypt: {e}"))?;
    let mut out = Vec::with_capacity(1 + 1 + 16 + 12 + ct.len());
    out.push(FILE_FORMAT_VERSION);
    out.push(kdf as u8);
    out.extend_from_slice(salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Returns `(salt, tokens, secrets)`. The Tier 2 caller hands
/// `salt` back to `derive_machine_id_key` to materialise the
/// cipher; same field is forwarded into the master-password
/// path in a future commit.
type DecodedFile = (
    [u8; 16],
    std::collections::HashMap<String, OAuthTokens>,
    std::collections::HashMap<String, String>,
);

fn decode_file(bytes: &[u8]) -> Result<DecodedFile, String> {
    if bytes.len() < 1 + 1 + 16 + 12 + 16 {
        return Err(format!("frame too short ({} bytes)", bytes.len()));
    }
    let version = bytes[0];
    if version != FILE_FORMAT_VERSION {
        return Err(format!(
            "unsupported file-format version {version} (this build supports {FILE_FORMAT_VERSION})"
        ));
    }
    let kdf =
        KdfMarker::from_byte(bytes[1]).ok_or_else(|| format!("unknown KDF marker {}", bytes[1]))?;
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&bytes[2..18]);
    let nonce_bytes = &bytes[18..30];
    let ct = &bytes[30..];
    let key = match kdf {
        KdfMarker::MachineId => derive_machine_id_key(&salt)?,
        KdfMarker::MasterPassword => {
            return Err("master-password mode is not supported in this build; \
                 a future release will prompt for the passphrase here"
                .into());
        }
    };
    let cipher = Aes256Gcm::new(&key);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ct)
        .map_err(|e| format!("decrypt: {e}"))?;
    let payload: StorePayload =
        serde_json::from_slice(&plain).map_err(|e| format!("deserialise: {e}"))?;
    let tokens = payload
        .tokens
        .into_iter()
        .map(|(k, v)| (k, v.into()))
        .collect();
    Ok((salt, tokens, payload.secrets))
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("dat.tmp");
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&tmp)?.permissions();
        perm.set_mode(0o600);
        std::fs::set_permissions(&tmp, perm)?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    fn sample() -> OAuthTokens {
        OAuthTokens {
            access_token: SecretString::from("acc-123".to_string()),
            refresh_token: Some(SecretString::from("ref-456".to_string())),
            expires_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn file_round_trips_tokens_and_secrets() {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&[7u8; 32]));
        let salt = [3u8; 16];
        let mut tokens = std::collections::HashMap::new();
        tokens.insert("google:abc".to_string(), sample());
        let mut secrets = std::collections::HashMap::new();
        secrets.insert("pwd:caldav-xyz".to_string(), "hunter2".to_string());
        let bytes = encode_file(&cipher, KdfMarker::MachineId, &salt, &tokens, &secrets).unwrap();
        // Only decode_file's machine-id branch can cope without a
        // real machine-id; assert the inner cipher decrypt works
        // by reading nonce + ct manually.
        assert_eq!(bytes[0], FILE_FORMAT_VERSION);
        assert_eq!(bytes[1], KdfMarker::MachineId as u8);
        assert_eq!(&bytes[2..18], &salt);
        let nonce = Nonce::from_slice(&bytes[18..30]);
        let plain = cipher.decrypt(nonce, &bytes[30..]).unwrap();
        let payload: StorePayload = serde_json::from_slice(&plain).unwrap();
        assert!(payload.tokens.contains_key("google:abc"));
        assert_eq!(
            payload.secrets.get("pwd:caldav-xyz"),
            Some(&"hunter2".to_string())
        );
    }

    #[test]
    fn decode_rejects_truncated_input() {
        assert!(decode_file(&[1, 2, 3]).is_err());
    }

    #[test]
    fn decode_rejects_unknown_version() {
        let mut bytes = vec![99u8, 0u8];
        bytes.resize(60, 0);
        let err = decode_file(&bytes).unwrap_err();
        assert!(err.contains("unsupported file-format version 99"));
    }

    #[test]
    fn decode_rejects_master_password_until_supported() {
        let mut bytes = vec![FILE_FORMAT_VERSION, KdfMarker::MasterPassword as u8];
        bytes.resize(60, 0);
        let err = decode_file(&bytes).unwrap_err();
        assert!(err.contains("master-password"));
    }
}
