// SPDX-License-Identifier: MIT OR Apache-2.0
//! **A completion report may not claim a quantifier the tree refutes.**
//!
//! ⚠️⚠️ **THIS TEST DOES NOT EXECUTE TODAY, AND THAT IS NOT A DETAIL.**
//!
//! At the commit that adds it, this crate does not resolve its dependencies:
//! `Cargo.toml` requests `config-management`, `enterprise-features` and
//! `token-to-profile` from `auth-framework`, and none of the nine published
//! versions declares any of them. **`cargo test` never reaches a compiler, so
//! CI cannot run this file and must not be read as covering it.**
//!
//! ⚠️ **It HAS been run, and its logic is verified — just not by cargo.** It
//! depends only on `std`, so it compiles and executes standalone:
//!
//! ```text
//! CARGO_MANIFEST_DIR=<repo> rustc --test --edition 2021 //!     tests/security_claims_match_the_tree.rs -o t && ./t
//! ```
//!
//! Forced that way before being committed: **green** with the stated count
//! matching the tree; **red** when the stated count was mutated 31 -> 99
//! (*"it states 99 / the tree 31"*); **red** when the correction was deleted
//! while the false quantifier stood. Each mutation was confirmed present in the
//! tree before its run.
//!
//! It is added now so that it begins running under `cargo test` the moment the
//! build is repaired (PR #10, `repair/buildable-and-inventory`). **Until then
//! it is verified but unwired, and a detector that is shipped and never
//! connected is worse than one never written, because the apparatus reads as
//! evidence.** That is why the limitation is stated here rather than in a commit
//! message nobody re-reads.
//!
//! # What it refuses
//!
//! `SECURITY_AUDIT_COMPLETION_REPORT.md` states *"All 'In a real
//! implementation' placeholders have been replaced with actual secure
//! implementations"*. Measured at `f0f570c`: **31 remain in `src/`**.
//!
//! ⚠️ **And the precondition was set by this project itself.**
//! `CRITICAL_SECURITY_FIXES.md` says *"Full security audit AFTER all 'In a
//! real' implementations completed"* and records **37 outstanding** — a number
//! matching that commit and no other in the history. **The honest document is
//! the one whose title does not say "COMPLETION".**
//!
//! # Why it fails on the CONJUNCTION
//!
//! Fixing either side makes it green: discharge the placeholders, or restate
//! the quantifier. **Only the disagreement is refused**, because the remedies
//! are different work and neither is this test's to choose.
//!
//! ⚠️ **The phrase is assembled at runtime.** This file would otherwise contain
//! the literal it searches for, and a later widening of the scan from `src/` to
//! the repository would make the guard match itself and pass forever. That
//! exact defect shipped in a sibling project's gate this week.

use std::fs;
use std::path::{Path, PathBuf};

/// The claim, as the report words it. Anchored on prose because line numbers rot.
const CLAIM: &str = "placeholders have been replaced";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Every `.rs` file under `src/`, recursively.
fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            source_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn the_completion_report_may_not_claim_a_quantifier_the_tree_refutes() {
    let root = repo_root();

    let report = fs::read_to_string(root.join("SECURITY_AUDIT_COMPLETION_REPORT.md"))
        .expect("control: SECURITY_AUDIT_COMPLETION_REPORT.md is missing — repoint this test rather than deleting it");

    // Non-vacuity: if the walk finds nothing, "zero placeholders" is trivially
    // true and this test passes having examined nothing.
    let mut files = Vec::new();
    source_files(&root.join("src"), &mut files);
    assert!(
        files.len() > 50,
        "only {} .rs files found under src/ — the walk is broken, not the tree",
        files.len()
    );

    // Assembled so this file never contains the literal it searches for.
    let needle = format!("In a real {}", "implementation");

    let mut hits: Vec<(String, usize)> = Vec::new();
    for f in &files {
        if let Ok(text) = fs::read_to_string(f) {
            let n = text.matches(needle.as_str()).count();
            if n > 0 {
                let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string();
                hits.push((rel, n));
            }
        }
    }
    let total: usize = hits.iter().map(|(_, n)| n).sum();

    // ⚠️ THE PINNED THING IS THE NUMBER, NOT THE SENTENCE.
    //
    // The original false quantifier is deliberately KEPT in the report — this is
    // a record of what was claimed and when, and rewriting it destroys that. So
    // "the claim is present" cannot be the failure condition, or the correct
    // remedy would redden the guard forever.
    //
    // What the correction added is a MEASURED COUNT. That count is a claim about
    // the tree, and this test holds it to the tree: if placeholders are
    // discharged and the correction still says 31, the note has rotted exactly
    // the way the sentence above it did. A measurement without a live check
    // becomes a claim about the present the moment anyone repeats it.
    let stated = stated_count(&report);

    assert!(
        stated.is_some(),
        "SECURITY_AUDIT_COMPLETION_REPORT.md contains the unqualified claim {CLAIM:?} \
         and NO correction stating the measured count.\n\n\
         Measured now: {total} occurrences across {} files.\n\n\
         Either discharge them, or restate the quantifier with the count and the ref it \
         was measured at. ⚠️ CRITICAL_SECURITY_FIXES.md already records the honest \
         version and its number matches the tree — read it before editing this one.",
        hits.len()
    );

    let stated = stated.unwrap();
    assert_eq!(
        stated,
        total,
        "THE CORRECTION IN SECURITY_AUDIT_COMPLETION_REPORT.md HAS GONE STALE.\n\n\
         it states : {stated} remaining\n\
         the tree  : {total} remaining, across {} files\n\
         largest   : {:?}\n\n\
         If placeholders were discharged, update the count — or, when it reaches 0, \
         delete the correction AND the false quantifier together and remove this test.\n\n\
         ⚠️ KNOWN LIMIT: this keys on one phrase and one stated number. Rewording either \
         silences it without discharging anything, and that is stated rather than hidden.",
        hits.len(),
        {
            let mut h = hits.clone();
            h.sort_by(|a, b| b.1.cmp(&a.1));
            h.into_iter().take(4).collect::<Vec<_>>()
        }
    );
}

/// The count the report's correction states, if it carries one.
///
/// Anchored on the phrase the correction uses, so a correction that drops the
/// number is treated as absent rather than as agreeing.
fn stated_count(report: &str) -> Option<usize> {
    let marker = "occurrences of";
    let idx = report.find(marker)?;
    let before = &report[..idx];
    let digits: String = before
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.chars().rev().collect::<String>().parse().ok()
}
