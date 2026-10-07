// SPDX-License-Identifier: MIT OR Apache-2.0
//! Brute-force hardening P2 (`docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md`,
//! rows 15 and 16): `synapse::auth`'s password login and JWT code are deleted, not hardened. It had
//! an unsalted SHA-256 password check with a user-existence oracle, a JWT verify secret that fell
//! back to a public constant, and a hardcoded auth-framework secret key. Only `utils` (the key
//! manager) had users outside the module, so only `utils` remains.

use std::path::Path;
use std::process::Command;

const DELETED: &[&str] = &[
    "src/synapse/auth/api.rs",
    "src/synapse/auth/example.rs",
    "src/synapse/auth/middleware.rs",
    "src/synapse/auth/trust_bridge.rs",
    "src/synapse/auth/README.md",
];

/// Secrets that were written into the source. None may appear in any tracked file under `src/`.
const SECRETS: &[&str] = &[
    "default_jwt_secret_change_in_production",
    "synapse-secret-key",
];

fn tracked_sources(root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "src"])
        .current_dir(root)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git ls-files failed: {out:?}");
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn the_password_and_jwt_modules_are_deleted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in DELETED {
        assert!(!root.join(path).exists(), "{path} should be deleted");
    }
    // Control: the kept key manager is still there, so the check is looking in the right place.
    assert!(root.join("src/synapse/auth/utils.rs").exists());
}

#[test]
fn no_hardcoded_auth_secret_remains_in_the_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = tracked_sources(root);
    assert!(
        files.len() > 50,
        "git ls-files src listed only {} files",
        files.len()
    );
    let mut found = Vec::new();
    let mut control = false;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        for secret in SECRETS {
            if text.contains(secret) {
                found.push(format!("{file}: {secret}"));
            }
        }
        // Control: the same scan finds a string known to be in the kept key manager.
        control |= file == "src/synapse/auth/utils.rs" && text.contains("SynapseKeyManager");
    }
    assert!(
        control,
        "control: the scan did not read src/synapse/auth/utils.rs"
    );
    assert!(found.is_empty(), "hardcoded auth secrets remain: {found:?}");
}
