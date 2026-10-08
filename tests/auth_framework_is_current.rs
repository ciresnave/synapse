// SPDX-License-Identifier: MIT OR Apache-2.0
//! auth-framework 0.3.0 pulled reqwest 0.11 -> hyper 0.14 -> h2 0.3.27 (RUSTSEC-2026-0258, no fix on
//! the 0.3 line) and rustls-pemfile 1.0.4 (RUSTSEC-2025-0134). The manifest must ask for 0.5 or later.
//! Cargo.lock is not tracked, so the manifest is what this can pin; the Security Audit job reads the
//! resolved graph.

/// The minor version of the `auth-framework` requirement in `manifest`, e.g. 5 for "0.5.0-rc26".
fn auth_framework_minor(manifest: &str) -> u64 {
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("auth-framework = "))
        .expect("the manifest declares auth-framework");
    let version = line
        .split("version = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the auth-framework line has a version");
    assert!(version.starts_with("0."), "unexpected major in {version}");
    version
        .split('.')
        .nth(1)
        .and_then(|m| m.parse().ok())
        .expect("a numeric minor")
}

#[test]
fn the_reader_sees_the_old_requirement() {
    let old = r#"auth-framework = { version = "0.3.0", optional = true, features = ["x"] }"#;
    assert_eq!(auth_framework_minor(old), 3);
}

#[test]
fn the_manifest_asks_for_auth_framework_0_5_or_later() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read Cargo.toml");
    assert!(
        auth_framework_minor(&manifest) >= 5,
        "auth-framework below 0.5 drags in h2 0.3 and rustls-pemfile 1 (RUSTSEC-2026-0258, 2025-0134)"
    );
}
