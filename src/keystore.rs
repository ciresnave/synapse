// SPDX-License-Identifier: MIT OR Apache-2.0
//! The persistent keystore (M2): a stable `role@account` identity on disk.
//!
//! One account key per user, created once and never overwritten. Per-role signing and sealing keys.
//! A role certificate (f1's [`AgentCertificate`]) signed locally by the account key. A process that
//! names the same role on the same account gets the same keys and the same `global_id`.
//! Design: `docs/superpowers/specs/2026-10-01-m2-keystore-design.md`.
//!
//! **Secret hygiene:** no key bytes, PEM bodies or file paths appear in any `Display`, `Debug` or
//! error. Errors name the *item* ("account key"), never where it lives, and an [`std::io::Error`]
//! is reduced to its [`std::io::ErrorKind`] because its own `Display` can carry the path.
//!
//! **Crash safety:** every file is written to a `.tmp-` sibling, synced, then published.
//! Create-only files are published with a hard link, which fails if the name exists, so two racing
//! creators never overwrite each other and the loser reads the winner's key. Only the certificate is
//! replaced (by rename), because any certificate the account key signed for the same keys is valid.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey, spki::der::pem::LineEnding};

/// What went wrong, naming only the item involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeystoreError {
    /// A role or account name is not `[A-Za-z0-9_-]{1,64}`.
    InvalidName,
    /// No home directory could be determined from the environment.
    NoHome,
    /// `init_account` found an account already there; it never overwrites one.
    AccountExists,
    /// `open` found no account; run `init_account` first.
    NoAccount,
    /// The item is readable by more than its owner.
    PermissionsTooOpen(&'static str),
    /// The item exists but does not parse.
    Corrupt(&'static str),
    /// The item could not be read or written.
    Io(&'static str, io::ErrorKind),
}

impl fmt::Display for KeystoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeystoreError::InvalidName => {
                write!(f, "names may contain only letters, digits, '-' and '_' (1 to 64)")
            }
            KeystoreError::NoHome => write!(f, "no keystore home: set SYNAPSE_HOME"),
            KeystoreError::AccountExists => write!(f, "an account already exists"),
            KeystoreError::NoAccount => write!(f, "no account: run `synapse id init` first"),
            KeystoreError::PermissionsTooOpen(item) => {
                write!(f, "the {item} is readable by more than its owner")
            }
            KeystoreError::Corrupt(item) => write!(f, "the {item} is corrupt"),
            KeystoreError::Io(item, kind) => write!(f, "cannot access the {item}: {kind}"),
        }
    }
}

impl std::error::Error for KeystoreError {}

type Result<T> = std::result::Result<T, KeystoreError>;

/// `[A-Za-z0-9_-]{1,64}`: the label rule of [`crate::certificate::identity_narrows`], which also
/// makes a name safe as a single path component.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// `$SYNAPSE_HOME`, else `%LOCALAPPDATA%\synapse` (Windows), else `$XDG_DATA_HOME/synapse`, else
/// `~/.local/share/synapse`. Reads the environment only.
pub fn default_home() -> Result<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(home) = var("SYNAPSE_HOME") {
        return Ok(PathBuf::from(home));
    }
    if cfg!(windows) {
        if let Some(local) = var("LOCALAPPDATA") {
            return Ok(PathBuf::from(local).join("synapse"));
        }
    }
    if let Some(data) = var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data).join("synapse"));
    }
    var("HOME")
        .map(|home| PathBuf::from(home).join(".local").join("share").join("synapse"))
        .ok_or(KeystoreError::NoHome)
}

/// The account, as it may be shown: its name and the `key_id` of its public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    pub account: String,
    pub key_id: String,
}

/// An opened keystore: the account and its key, loaded.
pub struct Keystore {
    home: PathBuf,
    account: String,
    account_key: SigningKey,
}

impl fmt::Debug for Keystore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keystore")
            .field("account", &self.account)
            .field("key_id", &self.account().key_id)
            .finish_non_exhaustive()
    }
}

const ACCOUNT_DIR: &str = "account";
const ACCOUNT_KEY: &str = "account.key.pem";
const ACCOUNT_NAME: &str = "name";

impl Keystore {
    /// Create the account. Refuses if one exists: overwriting an account key would silently lose
    /// the identity of every role it vouches for.
    pub fn init_account(home: &Path, account: &str) -> Result<AccountSummary> {
        if !valid_name(account) {
            return Err(KeystoreError::InvalidName);
        }
        let dir = home.join(ACCOUNT_DIR);
        ensure_dir(&dir, "account directory")?;
        let key_path = dir.join(ACCOUNT_KEY);
        if exists(&key_path, "account key")? {
            return Err(KeystoreError::AccountExists);
        }
        let key = SigningKey::from_bytes(&random_bytes::<32>()?);
        let pem = key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|_| KeystoreError::Corrupt("account key"))?;
        if !write_new(&key_path, pem.as_bytes(), "account key")? {
            return Err(KeystoreError::AccountExists);
        }
        replace(&dir.join(ACCOUNT_NAME), account.as_bytes(), "account name")?;
        Ok(summary(account, &key))
    }

    /// Open the keystore at `home`. Never creates an account.
    pub fn open(home: &Path) -> Result<Keystore> {
        let dir = home.join(ACCOUNT_DIR);
        let key_path = dir.join(ACCOUNT_KEY);
        if !exists(&key_path, "account key")? {
            return Err(KeystoreError::NoAccount);
        }
        check_dir(&dir, "account directory")?;
        let pem = read_checked(&key_path, "account key")?;
        let pem = std::str::from_utf8(&pem).map_err(|_| KeystoreError::Corrupt("account key"))?;
        let account_key =
            SigningKey::from_pkcs8_pem(pem).map_err(|_| KeystoreError::Corrupt("account key"))?;
        let name = read_checked(&dir.join(ACCOUNT_NAME), "account name")?;
        let account = String::from_utf8(name)
            .ok()
            .filter(|n| valid_name(n))
            .ok_or(KeystoreError::Corrupt("account name"))?;
        Ok(Keystore {
            home: home.to_path_buf(),
            account,
            account_key,
        })
    }

    #[must_use]
    pub fn account(&self) -> AccountSummary {
        summary(&self.account, &self.account_key)
    }

    /// The account's public key: what a receiver pins to trust this account's roles.
    #[must_use]
    pub fn account_public_key(&self) -> [u8; 32] {
        self.account_key.verifying_key().to_bytes()
    }
}

fn summary(account: &str, key: &SigningKey) -> AccountSummary {
    AccountSummary {
        account: account.to_string(),
        key_id: crate::sender_auth::key_id(&key.verifying_key().to_bytes()),
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; N];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| KeystoreError::Io("random source", io::ErrorKind::Other))?;
    Ok(bytes)
}

fn io_err(item: &'static str) -> impl Fn(io::Error) -> KeystoreError {
    move |e| KeystoreError::Io(item, e.kind())
}

fn exists(path: &Path, item: &'static str) -> Result<bool> {
    path.try_exists().map_err(io_err(item))
}

/// Create `dir` (and parents) owner-only, then check it.
fn ensure_dir(dir: &Path, item: &'static str) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(io_err(item))?;
    check_dir(dir, item)
}

fn check_dir(dir: &Path, item: &'static str) -> Result<()> {
    check_owner_only(dir, item)
}

/// Refuse anything readable by more than its owner.
fn check_owner_only(path: &Path, item: &'static str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path).map_err(io_err(item))?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(KeystoreError::PermissionsTooOpen(item));
        }
    }
    #[cfg(not(unix))]
    let _ = (path, item);
    Ok(())
}

/// Check permissions, then read.
fn read_checked(path: &Path, item: &'static str) -> Result<Vec<u8>> {
    check_owner_only(path, item)?;
    fs::read(path).map_err(io_err(item))
}

/// Write `bytes` to a fresh owner-only `.tmp-` sibling of `path` and sync it.
fn write_temp(path: &Path, bytes: &[u8], item: &'static str) -> Result<PathBuf> {
    let dir = path.parent().ok_or(KeystoreError::Io(item, io::ErrorKind::NotFound))?;
    let nonce = u64::from_le_bytes(random_bytes::<8>()?);
    let temp = dir.join(format!(".tmp-{}-{nonce:016x}", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(io_err(item))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = fs::remove_file(&temp);
        return Err(KeystoreError::Io(item, e.kind()));
    }
    Ok(temp)
}

/// Publish `bytes` at `path` only if nothing is there. `Ok(false)` means another writer got there
/// first and its file was left untouched.
fn write_new(path: &Path, bytes: &[u8], item: &'static str) -> Result<bool> {
    let temp = write_temp(path, bytes, item)?;
    let linked = fs::hard_link(&temp, path);
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(KeystoreError::Io(item, e.kind())),
    }
}

/// Publish `bytes` at `path`, replacing whatever is there, atomically.
fn replace(path: &Path, bytes: &[u8], item: &'static str) -> Result<()> {
    let temp = write_temp(path, bytes, item)?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        KeystoreError::Io(item, e.kind())
    })
}
