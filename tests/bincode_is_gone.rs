// SPDX-License-Identifier: MIT OR Apache-2.0
//! Removing bincode (RUSTSEC-2025-0141), step B1: the core no longer uses it. Plan:
//! docs/superpowers/plans/2026-10-07-remove-bincode.md.
//!
//! Sources are enumerated from the git index, never by walking the disk. B1 scopes the scan to the
//! core; B2 widens it to the whole tree once the legacy trees are deleted or ported.

use std::path::Path;
use std::process::Command;

use synapse::types::{DateTimeWrapper, SecureMessage, SecurityLevel, UuidWrapper};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn tracked(pathspecs: &[&str]) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root())
        .arg("ls-files")
        .arg("--")
        .args(pathspecs)
        .output()
        .expect("git must be on PATH: this test enumerates from the index");
    assert!(
        out.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("git ls-files printed non-UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn files_containing(files: &[String], needle: &str) -> Vec<String> {
    files
        .iter()
        .filter(|f| {
            std::fs::read_to_string(root().join(f))
                .is_ok_and(|text| text.to_lowercase().contains(&needle.to_lowercase()))
        })
        .cloned()
        .collect()
}

/// The core, for B1: everything except the legacy trees and `error.rs`, which B2 handles, and this
/// guard itself, which names what it looks for.
fn core_files() -> Vec<String> {
    let files = tracked(&[
        "src",
        "crates",
        "tests",
        ":(exclude)src/synapse/blockchain",
        ":(exclude)src/synapse/models/trust",
        ":(exclude)src/error.rs",
        ":(exclude)tests/bincode_is_gone.rs",
    ]);
    assert!(
        files.len() > 100,
        "only {} files from git ls-files: enumeration is broken",
        files.len()
    );
    files
}

#[test]
fn no_core_source_mentions_bincode() {
    let files = core_files();
    // Positive control: the same scan finds the wire format where it is used.
    assert!(
        files_containing(&files, "serde_json").contains(&"src/transport/email_unified.rs".into()),
        "the scan did not find serde_json in email_unified.rs, so it is not reading files"
    );
    let hits = files_containing(&files, "bincode");
    assert!(hits.is_empty(), "the core still mentions bincode: {hits:?}");
}

#[test]
fn removed_byte_codecs_are_gone() {
    // (i) The four pairs are no longer defined.
    let types = std::fs::read_to_string(root().join("src/types.rs")).unwrap();
    assert!(
        !types.contains("fn to_bytes") && !types.contains("fn from_bytes"),
        "src/types.rs still defines a bincode to_bytes/from_bytes"
    );

    // (ii) Nothing calls them by type path.
    let all = tracked(&[
        "src",
        "crates",
        "tests",
        "examples",
        ":(exclude)tests/bincode_is_gone.rs",
    ]);
    // Control: the same `Type::from_bytes` shape with another type is found, so the scan works.
    assert!(
        !files_containing(&all, "Signature::from_bytes").is_empty(),
        "control: Signature::from_bytes should be found"
    );
    for ty in [
        "SimpleMessage",
        "SecureMessage",
        "StreamChunk",
        "StreamMetadata",
    ] {
        for f in ["to_bytes", "from_bytes"] {
            let needle = format!("{ty}::{f}");
            let hits = files_containing(&all, &needle);
            assert!(hits.is_empty(), "{needle} is still called in {hits:?}");
        }
    }
}

/// Captured at `80e4e66`, before any of B1 moved: moving the wrappers must not change the wire.
const SECURE_MESSAGE_JSON: &str = r#"{"message_id":"6f0e8a3c-2b1d-4c5e-9a7f-0123456789ab","to_global_id":"bob@synapse.test","from_global_id":"alice@synapse.test","encrypted_content":[112,97,121,108,111,97,100],"sender_proof":{"alg":"none","key_id":"","sig":[]},"timestamp":"2026-10-07T12:00:00.123456789Z","security_level":"authenticated","routing_path":["relay-1"],"metadata":{"reply_to":"127.0.0.1:1"},"protocol_version":1}"#;

fn fixed_message() -> SecureMessage {
    let mut message = SecureMessage::new(
        "bob@synapse.test",
        "alice@synapse.test",
        b"payload".to_vec(),
        SecurityLevel::Authenticated,
    );
    message.message_id =
        UuidWrapper::new(uuid::Uuid::parse_str("6f0e8a3c-2b1d-4c5e-9a7f-0123456789ab").unwrap());
    message.timestamp = DateTimeWrapper::new(
        chrono::DateTime::parse_from_rfc3339("2026-10-07T12:00:00.123456789Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    );
    message.add_routing_hop("relay-1");
    message.add_metadata("reply_to", "127.0.0.1:1");
    message
}

#[test]
fn secure_message_json_is_unchanged() {
    let json = serde_json::to_string(&fixed_message()).unwrap();
    assert_eq!(json, SECURE_MESSAGE_JSON, "the wire format changed");
    // Negative control: a one-character change to the literal compares unequal.
    let mutated = SECURE_MESSAGE_JSON.replacen("relay-1", "relay-2", 1);
    assert_ne!(json, mutated);
    // And it still round-trips.
    let back: SecureMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), json);
}
