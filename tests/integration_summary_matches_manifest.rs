// SPDX-License-Identifier: MIT OR Apache-2.0
//! **A document claiming which dependency features were added must agree with the manifest.**
//!
//! `AUTH_FRAMEWORK_INTEGRATION_SUMMARY.md` reports, under "What We've
//! Implemented", that three `auth-framework` feature flags were added:
//! `config-management`, `enterprise-features`, `token-to-profile`.
//!
//! ⚠️ **None of the three exists in any published version of `auth-framework`**
//! — measured across all nine, implicit optional-dependency features included.
//! Requesting them made this crate unresolvable: `cargo check` on **default**
//! features failed at version selection, before the compiler ran. That is why
//! the repository did not build. They were removed in #10.
//!
//! ⚠️ **THE SENTENCE IS LITERALLY ACCURATE AND THE FRAME IS FALSE.** The flags
//! really were typed into `Cargo.toml`. What is untrue is "Integration
//! Complete" and "what we've accomplished" — a change described faithfully and
//! presented as a success. **That is harder to catch than a wrong description,
//! because every detail checks out.**
//!
//! # What this pins, and why it is not the sentence
//!
//! The original claim is deliberately KEPT in the document — it is the record of
//! what was asserted and when. **So "the sentence is present" cannot be the
//! failure condition, or correcting the document would redden this guard
//! forever.** What the correction adds is an explicit statement of the CURRENT
//! requested set, and this test holds that statement to `Cargo.toml`.
//!
//! **Add or remove an `auth-framework` feature and this reddens until the
//! document is updated.** The document cannot silently drift from the manifest
//! the way the original claim did.

use std::fs;
use std::path::Path;

/// The features `Cargo.toml` actually requests of `auth-framework`.
fn manifest_features(manifest: &str) -> Vec<String> {
    let start = match manifest.find("auth-framework = {") {
        Some(i) => i,
        None => return Vec::new(),
    };
    let rest = &manifest[start..];
    let end = match rest.find("] }") {
        Some(i) => i,
        None => return Vec::new(),
    };
    rest[..end]
        .split('"')
        .filter(|s| s.contains('-') && !s.contains('=') && !s.contains('.'))
        .map(|s| s.trim().to_string())
        .filter(|s| s != "auth-framework")
        .collect()
}

/// The set the correction states as current, from its "the requested set is
/// exactly:" line. Anchored on prose, not a line number.
fn documented_features(doc: &str) -> Option<Vec<String>> {
    let marker = "the requested set is exactly:";
    let idx = doc.find(marker)?;
    let line: String = doc[idx + marker.len()..]
        .chars()
        .take_while(|c| *c != '\n')
        .collect();
    let feats: Vec<String> = line
        .split('`')
        .filter(|s| s.contains('-') && !s.contains(' '))
        .map(|s| s.trim().to_string())
        .collect();
    if feats.is_empty() {
        None
    } else {
        Some(feats)
    }
}

#[test]
fn the_integration_summary_agrees_with_cargo_toml() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("control: Cargo.toml");
    let doc = fs::read_to_string(root.join("AUTH_FRAMEWORK_INTEGRATION_SUMMARY.md")).expect(
        "control: AUTH_FRAMEWORK_INTEGRATION_SUMMARY.md is missing — repoint, do not delete",
    );

    let mut actual = manifest_features(&manifest);
    actual.sort();

    // Non-vacuity: if the manifest parser stops finding features, every
    // comparison below is between two empty sets and passes having read nothing.
    assert!(
        !actual.is_empty(),
        "no auth-framework features parsed from Cargo.toml — the parser is broken, not the manifest"
    );

    let mut stated = documented_features(&doc).unwrap_or_else(|| {
        panic!(
            "AUTH_FRAMEWORK_INTEGRATION_SUMMARY.md no longer states the current requested set.\n\
             Cargo.toml requests {actual:?}.\n\
             The document claims three feature flags were ADDED that no published auth-framework \
             version declares. If you removed the correction, restore a statement of the current \
             set — the claim above it is kept deliberately as the record and cannot be the thing \
             this test keys on."
        )
    });
    stated.sort();

    assert_eq!(
        stated, actual,
        "THE INTEGRATION SUMMARY AND Cargo.toml DISAGREE ABOUT auth-framework's FEATURES.\n\n  \
         document states : {stated:?}\n  \
         Cargo.toml has  : {actual:?}\n\n\
         Update the correction block in AUTH_FRAMEWORK_INTEGRATION_SUMMARY.md, or revert the \
         manifest change.\n\n\
         ⚠️ This guard exists because that document once reported adding three features that \
         exist in NO published version of auth-framework, which made this crate unresolvable. \
         A document describing dependency features must not be able to drift from the manifest \
         silently.\n\n\
         ⚠️ KNOWN LIMIT: it compares the auth-framework feature list only, and keys on one \
         phrase in the document. It is not a general doc/manifest checker."
    );
}
