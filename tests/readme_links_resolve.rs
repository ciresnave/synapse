// SPDX-License-Identifier: MIT OR Apache-2.0
//! **Every relative link in the README must point at something that exists.**
//!
//! Ten did not. Two had been deleted by `7b80ca4` (`PRODUCTION_READINESS_REPORT.md`,
//! `TRANSPORT_STATUS.md`); eight had never existed at all — `CONTRIBUTING.md`,
//! `RENAMING_RECOMMENDATIONS.md`, a bare `LICENSE` in a dual-licensed repo that
//! ships `LICENSE-MIT` and `LICENSE-APACHE`, and five example files
//! (`basic_messaging.rs`, `ai_collaboration.rs`, `file_transfer.rs`,
//! `real_time_chat.rs`, `ai_assistant.rs`) in a directory holding 33 real ones.
//!
//! ⚠️ **THE FIRST AUDIT OF THIS FOUND FOUR, NOT TEN, BECAUSE IT SCANNED ONLY
//! `.md`.** The five dead example links and the dead `LICENSE` are `.rs` and
//! extensionless, and an under-counted denominator is a claim that looks better
//! than the truth. **This guard therefore checks EVERY relative link regardless
//! of extension** — the scoping choice is the defect it exists to prevent.
//!
//! ⚠️ **AND IT CANNOT RUN UNDER `cargo test` AT THIS COMMIT.** The crate does not
//! resolve its dependencies — `Cargo.toml` requests three `auth-framework`
//! features no published version declares (#2) — so CI never reaches a compiler.
//! It was verified standalone (`std` only) and forced in both directions before
//! being committed:
//!
//! ```text
//! CARGO_MANIFEST_DIR=<repo> rustc --test --edition 2021 \
//!     tests/readme_links_resolve.rs -o t && ./t
//! ```
//!
//! **Until the build is repaired it is verified but unwired, and must not be
//! counted as coverage.** A detector shipped and never connected is worse than
//! one never written, because the apparatus reads as evidence.
//!
//! # Why this matters more than tidiness right now
//!
//! The README is what a newcomer reads to learn how to connect — and the
//! newcomers this project is about to acquire are agents on other providers.
//! **A dead "Basic Messaging" link is an onboarding failure, not a typo.**

use std::fs;
use std::path::Path;

/// Extract every link target from `](...)` pairs.
fn link_targets(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = md.chars().collect();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == ']' && bytes[i + 1] == '(' {
            let mut j = i + 2;
            let mut buf = String::new();
            while j < bytes.len() && bytes[j] != ')' {
                buf.push(bytes[j]);
                j += 1;
            }
            if j < bytes.len() {
                out.push(buf);
                i = j;
            }
        }
        i += 1;
    }
    out
}

/// Relative file references only: not URLs, not in-page anchors, not mailto.
fn is_relative_file(target: &str) -> bool {
    let t = target.trim();
    !(t.is_empty()
        || t.starts_with("http://")
        || t.starts_with("https://")
        || t.starts_with('#')
        || t.starts_with("mailto:"))
}

#[test]
fn every_relative_readme_link_resolves() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md"))
        .expect("control: README.md is missing — repoint this test rather than deleting it");

    let all = link_targets(&readme);
    // Non-vacuity: a parser that stops finding links passes this test having
    // examined nothing, and is indistinguishable from a README with no dead links.
    assert!(
        all.len() > 30,
        "only {} links parsed from README.md — the extractor is broken, not the README",
        all.len()
    );

    let mut relative: Vec<String> = all
        .into_iter()
        .filter(|t| is_relative_file(t))
        .map(|t| t.split('#').next().unwrap_or("").trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    relative.sort();
    relative.dedup();

    // Non-vacuity, second arm: the filter must not remove everything.
    assert!(
        relative.len() > 15,
        "only {} RELATIVE links after filtering — the filter is too aggressive",
        relative.len()
    );

    let dead: Vec<&String> = relative
        .iter()
        .filter(|t| !root.join(t.as_str()).exists())
        .collect();

    assert!(
        dead.is_empty(),
        "THE README LINKS {} PATHS THAT DO NOT EXIST:\n  {}\n\n\
         Checked {} distinct relative links.\n\n\
         ⚠️ Before removing a link, establish WHICH KIND it is — the remedies differ:\n  \
         - the target was DELETED  -> the claim it backed is now unbacked; say so, or restore it\n  \
         - the target NEVER EXISTED -> the README has always promised something absent\n\
         `git log --diff-filter=AD -- <path>` separates them, and an empty history means the\n\
         second. ⚠️ Do not repoint a dead link at a file with a similar name without checking\n\
         it is the same thing — sending a reader to the wrong file is the same defect.",
        dead.len(),
        dead.iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
        relative.len()
    );
}
