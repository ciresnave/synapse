// SPDX-License-Identifier: MIT OR Apache-2.0
//! Guards against re-introducing specific fabricated security-capability claims that were
//! removed from this crate's docs when `src/router_enhanced.rs` was deleted (its "Security
//! Features" module doc advertised PGP encryption, TLS, S/MIME, DNS-based identity validation,
//! replay protection, and rate limiting -- none of which this crate implements).
//!
//! ⚠️ KNOWN LIMIT: each entry below anchors on one exact phrasing. A claim reworded to say the
//! same false thing in different words will not be caught by this test -- this is a per-phrasing
//! detector, not a semantic one. When you find a new instance of this pattern (a doc claiming an
//! unimplemented capability), add its exact wording as a new entry here rather than assuming
//! this test already covers it.
//!
//! This is a separate mechanism from `tests/security_claims_match_the_tree.rs`, which compares a
//! *stated count* of "In a real implementation" placeholders against a live count in `src/` -- a
//! number-vs-tree check with no fixed-phrase anchors. This file adds the fixed-phrase check that
//! was previously only run as one-off greps during review.

use std::fs;
use std::path::{Path, PathBuf};

/// (phrase, why it's false)
const FORBIDDEN_CLAIMS: &[(&str, &str)] = &[
    (
        "Timestamps and nonces prevent replay attacks",
        "no anti-replay mechanism exists on any router-level receive path in this crate",
    ),
    (
        "PGP-encrypted with recipient's public key",
        "this crate does not implement PGP or any message-content encryption scheme",
    ),
    (
        "Messages encrypted with recipient's public key",
        "this crate signs messages but does not encrypt their content -- Private/Secure security levels are refused, not honoured",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scan_files(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan_files(&p, exts, out);
        } else if p.extension().is_some_and(|x| exts.iter().any(|e| x == *e)) {
            out.push(p);
        }
    }
}

#[test]
fn fabricated_security_claims_do_not_reappear() {
    let root = repo_root();

    let mut files = Vec::new();
    scan_files(&root.join("src"), &["rs"], &mut files);
    scan_files(&root.join("tests"), &["rs"], &mut files);
    scan_files(&root.join("examples"), &["rs"], &mut files);
    // Top-level *.md only -- not the whole tree, to avoid matching this test's own doc comment
    // or historical process records (e.g. SDD ledgers) that quote a claim while documenting it.
    // Note: this walk never reaches `.superpowers/` (gitignored SDD scratch at the repo root),
    // since only `src/`, `tests/`, `examples/`, and top-level `*.md` are scanned.
    if let Ok(entries) = fs::read_dir(&root) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "md") {
                files.push(p);
            }
        }
    }

    assert!(
        files.len() > 50,
        "only {} files scanned -- the walk is broken, not the tree",
        files.len()
    );

    let mut violations = Vec::new();
    for f in &files {
        // Never scan this test's own source -- it necessarily contains the forbidden phrases.
        if f.file_name()
            .is_some_and(|n| n == "fabricated_security_claims_are_absent.rs")
        {
            continue;
        }
        if let Ok(text) = fs::read_to_string(f) {
            // Case-insensitive: a claim reworded with different capitalization (e.g. mid-sentence
            // vs. sentence-start) is the same false claim and must still be caught.
            let text_lower = text.to_lowercase();
            for (claim, reason) in FORBIDDEN_CLAIMS {
                if text_lower.contains(&claim.to_lowercase()) {
                    let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string();
                    violations.push(format!("{rel}: {claim:?} ({reason})"));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Fabricated security-capability claim(s) found:\n{}\n\n\
         These phrases describe capabilities this crate does not implement. If the capability \
         was genuinely added, delete the entry from FORBIDDEN_CLAIMS with a note on what changed \
         and where it's tested. If this is a re-introduced false claim, remove it or replace it \
         with an accurate statement instead.",
        violations.join("\n")
    );
}
