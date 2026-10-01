// SPDX-License-Identifier: MIT OR Apache-2.0
//! The persistent keystore (M2): a stable `role@account` identity on disk.
//! Design: `docs/superpowers/specs/2026-10-01-m2-keystore-design.md`.

use std::path::Path;

use synapse::keystore::{Keystore, KeystoreError, default_home, valid_name};

fn account_key_path(home: &Path) -> std::path::PathBuf {
    home.join("account").join("account.key.pem")
}

/// The home path in both slash styles, so a message can be checked for either.
fn path_forms(home: &Path) -> [String; 2] {
    let s = home.display().to_string();
    [s.replace('/', "\\"), s.replace('\\', "/")]
}

#[test]
fn init_creates_an_account_that_open_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let created = Keystore::init_account(dir.path(), "acct").expect("init");
    assert_eq!(created.account, "acct");
    assert_eq!(created.key_id.len(), 64);
    assert!(created.key_id.chars().all(|c| c.is_ascii_hexdigit()));

    let store = Keystore::open(dir.path()).expect("open");
    let read = store.account();
    assert_eq!(read.account, created.account);
    assert_eq!(read.key_id, created.key_id);
}

#[test]
fn init_refuses_when_an_account_exists() {
    let dir = tempfile::tempdir().unwrap();
    Keystore::init_account(dir.path(), "acct").expect("init");
    let before = std::fs::read(account_key_path(dir.path())).unwrap();

    let again = Keystore::init_account(dir.path(), "other");
    assert!(matches!(again, Err(KeystoreError::AccountExists)), "{again:?}");
    assert_eq!(std::fs::read(account_key_path(dir.path())).unwrap(), before);
}

#[test]
fn open_without_an_account_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(Keystore::open(dir.path()), Err(KeystoreError::NoAccount)));
}

#[test]
fn names_are_validated() {
    for good in ["synapse", "a-b_1", &"x".repeat(64)] {
        assert!(valid_name(good), "{good:?} should be valid");
    }
    for bad in ["", "a.b", "a@b", "../x", "a b", &"x".repeat(65)] {
        assert!(!valid_name(bad), "{bad:?} should be invalid");
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        Keystore::init_account(dir.path(), "a.b"),
        Err(KeystoreError::InvalidName)
    ));
}

#[test]
fn a_garbage_account_key_is_corrupt_and_unnamed() {
    let dir = tempfile::tempdir().unwrap();
    Keystore::init_account(dir.path(), "acct").expect("init");
    let key = account_key_path(dir.path());
    let mut perms = std::fs::metadata(&key).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&key, perms).unwrap();
    std::fs::write(&key, b"junk").unwrap();

    let err = Keystore::open(dir.path()).expect_err("garbage must not load");
    assert!(matches!(err, KeystoreError::Corrupt("account key")), "{err:?}");
    let msg = err.to_string();
    for form in path_forms(dir.path()) {
        assert!(!msg.contains(&form), "message names the path: {msg}");
    }
    assert!(!msg.contains("junk"), "message carries file bytes: {msg}");
}

#[cfg(unix)]
#[test]
fn keys_are_owner_only_and_widened_keys_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    Keystore::init_account(dir.path(), "acct").expect("init");
    let key = account_key_path(dir.path());
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&key), 0o600);
    assert_eq!(mode(&dir.path().join("account")), 0o700);

    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Keystore::open(dir.path()),
        Err(KeystoreError::PermissionsTooOpen(_))
    ));
}

#[test]
fn a_leftover_temp_file_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("account")).unwrap();
    std::fs::write(dir.path().join("account").join(".tmp-1-1"), b"junk").unwrap();
    Keystore::init_account(dir.path(), "acct").expect("init despite a leftover temp file");
    Keystore::open(dir.path()).expect("open despite a leftover temp file");
}

#[test]
fn default_home_prefers_synapse_home() {
    // The only test in this binary that touches the environment.
    let chosen = std::env::temp_dir().join("synapse-home-test");
    // SAFETY: no other test in this binary reads or writes these variables.
    unsafe { std::env::set_var("SYNAPSE_HOME", &chosen) };
    assert_eq!(default_home().unwrap(), chosen);
    unsafe { std::env::remove_var("SYNAPSE_HOME") };
    let fallback = default_home().unwrap();
    assert!(fallback.ends_with("synapse"), "{}", fallback.display());
    #[cfg(windows)]
    assert!(fallback.starts_with(std::env::var("LOCALAPPDATA").unwrap()));
}
