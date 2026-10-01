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

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey, spki::der::pem::LineEnding};

use crate::certificate::{AgentCertificate, Permission, chain_from_pem, chain_to_pem};
use crate::crypto::CryptoManager;
use crate::sealing::SealingKeyPair;

/// How long a role certificate is valid (PM-approved, spec §4 Q3).
pub const CERT_VALIDITY: Duration = Duration::hours(24);
/// A certificate with less than this left is reissued on load.
pub const RENEW_BELOW: Duration = Duration::hours(12);

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
                write!(
                    f,
                    "names may contain only letters, digits, '-' and '_' (1 to 64)"
                )
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
    if cfg!(windows)
        && let Some(local) = var("LOCALAPPDATA")
    {
        return Ok(PathBuf::from(local).join("synapse"));
    }
    if let Some(data) = var("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data).join("synapse"));
    }
    var("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("synapse")
        })
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

/// A role, loaded: its identity, its keys in a ready [`CryptoManager`], and its certificate.
pub struct RoleIdentity {
    /// `<role>@<account>`.
    pub global_id: String,
    /// The signing key, the sealing key and the certificate chain, all loaded.
    pub crypto: CryptoManager,
    pub signing_key_id: String,
    pub sealing_key_id: String,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub permissions: Vec<Permission>,
    /// Leaf first; one link, signed by the account key.
    pub chain: Vec<AgentCertificate>,
}

impl fmt::Debug for RoleIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoleIdentity")
            .field("global_id", &self.global_id)
            .field("signing_key_id", &self.signing_key_id)
            .field("sealing_key_id", &self.sealing_key_id)
            .field("not_before", &self.not_before)
            .field("not_after", &self.not_after)
            .field("permissions", &self.permissions)
            .finish_non_exhaustive()
    }
}

const ACCOUNT_DIR: &str = "account";
const ROLES_DIR: &str = "roles";
const SIGNING_KEY: &str = "signing.key.pem";
const SEALING_KEY: &str = "sealing.key.pem";
const CERT: &str = "cert.pem";
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

    /// Load `role`, creating its keys on first use, and (re)issuing its certificate when there is
    /// none, it does not match the keys, or less than [`RENEW_BELOW`] of it remains at `now`.
    pub fn role(&self, role: &str, now: DateTime<Utc>) -> Result<RoleIdentity> {
        if !valid_name(role) {
            return Err(KeystoreError::InvalidName);
        }
        let roles = self.home.join(ROLES_DIR);
        ensure_dir(&roles, "roles directory")?;
        let dir = roles.join(role);
        ensure_dir(&dir, "role directory")?;

        let signing_pem = load_or_create(&dir.join(SIGNING_KEY), "role signing key", || {
            CryptoManager::new()
                .generate_keypair()
                .map(|(private, _public)| private)
                .map_err(|_| KeystoreError::Io("role signing key", io::ErrorKind::Other))
        })?;
        let sealing_pem = load_or_create(&dir.join(SEALING_KEY), "role sealing key", || {
            Ok(SealingKeyPair::generate().to_pkcs8_pem())
        })?;

        let mut crypto = CryptoManager::new();
        crypto
            .load_private_key(&signing_pem)
            .map_err(|_| KeystoreError::Corrupt("role signing key"))?;
        crypto
            .load_sealing_key_pem(&sealing_pem)
            .map_err(|_| KeystoreError::Corrupt("role sealing key"))?;
        let signing_public = crypto
            .public_key_bytes()
            .map_err(|_| KeystoreError::Corrupt("role signing key"))?;
        let sealing_public = *crypto
            .sealing_key()
            .ok_or(KeystoreError::Corrupt("role sealing key"))?
            .public_key()
            .as_bytes();

        let global_id = format!("{role}@{}", self.account);
        let cert_path = dir.join(CERT);
        let current = if exists(&cert_path, "role certificate")? {
            read_checked(&cert_path, "role certificate")?
        } else {
            Vec::new()
        };
        let reusable = std::str::from_utf8(&current)
            .ok()
            .and_then(|pem| chain_from_pem(pem).ok())
            .filter(|chain| {
                chain.len() == 1
                    && chain[0].subject_global_id == global_id
                    && chain[0].subject_signing_key == signing_public
                    && chain[0].subject_sealing_key == sealing_public
                    && chain[0].issuer_key_id == self.account().key_id
                    && chain[0].not_before <= now
                    && chain[0].not_after - now >= RENEW_BELOW
            });
        let chain = match reusable {
            Some(chain) => chain,
            None => {
                let unsigned = AgentCertificate {
                    version: 1,
                    serial: random_bytes::<16>()?,
                    issuer_key_id: self.account().key_id,
                    subject_label: role.to_string(),
                    subject_global_id: global_id.clone(),
                    subject_signing_key: signing_public,
                    subject_sealing_key: sealing_public,
                    not_before: now,
                    not_after: now + CERT_VALIDITY,
                    permissions: vec![Permission::Send, Permission::RequestAck, Permission::Ack],
                    may_delegate: 0,
                    signature: [0; 64],
                };
                let chain = vec![AgentCertificate::sign(unsigned, &self.account_key)];
                replace(
                    &cert_path,
                    chain_to_pem(&chain).as_bytes(),
                    "role certificate",
                )?;
                chain
            }
        };

        let leaf = &chain[0];
        let identity = RoleIdentity {
            global_id,
            signing_key_id: crate::sender_auth::key_id(&signing_public),
            sealing_key_id: crate::sender_auth::key_id(&sealing_public),
            not_before: leaf.not_before,
            not_after: leaf.not_after,
            permissions: leaf.permissions.clone(),
            chain: chain.clone(),
            crypto: {
                crypto.set_certificate_chain(chain);
                crypto
            },
        };
        Ok(identity)
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
        let mode = fs::metadata(path)
            .map_err(io_err(item))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(KeystoreError::PermissionsTooOpen(item));
        }
    }
    #[cfg(windows)]
    {
        if windows_acl::readable_by_broad_group(path).map_err(io_err(item))? {
            return Err(KeystoreError::PermissionsTooOpen(item));
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = (path, item);
    Ok(())
}

/// The Windows half of "owner-only" (PM-approved, spec §4 Q2(a)): files live under the user's
/// `%LOCALAPPDATA%`, whose inherited ACL admits the user, SYSTEM and Administrators. On every load
/// we refuse a file or directory that `Everyone`, `Users` or `Authenticated Users` can read, the
/// same rule OpenSSH for Windows applies to private keys. A NULL DACL grants everyone everything, so
/// it is refused too.
#[cfg(windows)]
mod windows_acl {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GENERIC_ALL, GENERIC_READ, LocalFree};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, CreateWellKnownSid, DACL_SECURITY_INFORMATION,
        EqualSid, GetAce, PSECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE, WELL_KNOWN_SID_TYPE,
        WinAuthenticatedUserSid, WinBuiltinUsersSid, WinWorldSid,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_READ_DATA;

    /// `ACCESS_ALLOWED_ACE_TYPE` (winnt.h), a fixed ABI value; defined here rather than enabling
    /// windows-sys's `System_SystemServices` feature for one constant.
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const READ_BITS: u32 = FILE_READ_DATA | GENERIC_READ | GENERIC_ALL;
    const BROAD: [WELL_KNOWN_SID_TYPE; 3] =
        [WinWorldSid, WinBuiltinUsersSid, WinAuthenticatedUserSid];

    /// Frees the security descriptor `GetNamedSecurityInfoW` allocated, on every path.
    struct Descriptor(PSECURITY_DESCRIPTOR);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: allocated by GetNamedSecurityInfoW with LocalAlloc; freed exactly once.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    pub(super) fn readable_by_broad_group(path: &Path) -> io::Result<bool> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; the out-pointers are valid for writes; owner, group and
        // SACL are not requested, so their out-pointers may be null.
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        let _owned = Descriptor(descriptor);
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        if dacl.is_null() {
            return Ok(true);
        }

        let mut broad = [[0u8; SECURITY_MAX_SID_SIZE as usize]; 3];
        for (kind, buffer) in BROAD.iter().zip(broad.iter_mut()) {
            let mut size = SECURITY_MAX_SID_SIZE;
            // SAFETY: `buffer` holds SECURITY_MAX_SID_SIZE bytes, as `size` says.
            let ok = unsafe {
                CreateWellKnownSid(
                    *kind,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
        }

        // SAFETY: `dacl` is non-null and points into `descriptor`, which `_owned` keeps alive.
        let count = unsafe { (*dacl).AceCount };
        for index in 0..u32::from(count) {
            let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
            // SAFETY: index < AceCount; `ace` receives a pointer into the DACL.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every ACE starts with an ACE_HEADER.
            let header = unsafe { &*(ace as *const ACE_HEADER) };
            if header.AceType != ACCESS_ALLOWED_ACE_TYPE {
                continue;
            }
            // SAFETY: AceType says this ACE is an ACCESS_ALLOWED_ACE; its SID starts at SidStart.
            let allowed = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            if allowed.Mask & READ_BITS == 0 {
                continue;
            }
            let sid = std::ptr::addr_of!(allowed.SidStart) as *mut core::ffi::c_void;
            for buffer in &mut broad {
                // SAFETY: both point at valid SIDs for the duration of the call.
                if unsafe { EqualSid(sid, buffer.as_mut_ptr().cast()) } != 0 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

/// Check permissions, then read.
fn read_checked(path: &Path, item: &'static str) -> Result<Vec<u8>> {
    check_owner_only(path, item)?;
    fs::read(path).map_err(io_err(item))
}

/// Read the key at `path`, or create it with `make` if absent. If another process creates it
/// between our check and our write, its key wins and is the one returned.
fn load_or_create(
    path: &Path,
    item: &'static str,
    make: impl FnOnce() -> Result<String>,
) -> Result<String> {
    if !exists(path, item)? {
        write_new(path, make()?.as_bytes(), item)?;
    }
    String::from_utf8(read_checked(path, item)?).map_err(|_| KeystoreError::Corrupt(item))
}

/// Write `bytes` to a fresh owner-only `.tmp-` sibling of `path` and sync it.
fn write_temp(path: &Path, bytes: &[u8], item: &'static str) -> Result<PathBuf> {
    let dir = path
        .parent()
        .ok_or(KeystoreError::Io(item, io::ErrorKind::NotFound))?;
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
