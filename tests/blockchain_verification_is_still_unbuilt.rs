// SPDX-License-Identifier: MIT OR Apache-2.0
//! **`verify_block_signature` has never existed. This reddens when it does.**
//!
//! `tests/security_test.rs` once carried a test called
//! `test_blockchain_signature_verification` that signed a block and never
//! verified one. The name was what `cargo test` printed on every green run, so
//! anyone asking *"does this project test that block signatures are verified?"*
//! got a yes from a test that only signs. It is now named
//! `test_blockchain_block_signing`, for what it asserts.
//!
//! ⚠️ **THE DEFERRAL IS NOT "SOMEONE FORGOT TO CALL IT". THE FUNCTION DOES NOT
//! EXIST.** Measured at `f0f570c`, before this file was added: the identifier
//! appeared exactly ONCE in the repository, inside the comment deferring it.
//! Not renamed, not private, not behind a feature: never written.
//!
//! *(This file necessarily adds prose mentions of the name, which is why the
//! search below is for the DEFINITION form and not the bare identifier — a
//! guard whose own documentation defeats it is the vacuity this file exists to
//! avoid.)*
//!
//! **That distinction decides the remedy.** *"Never called"* sends someone
//! looking for a call site. *"Never existed"* means something has to be built
//! first. ⚠️ **A grep returns zero for both**, so the two findings are
//! indistinguishable by the query that produces them, and only the count of
//! DEFINITIONS separates them.
//!
//! # Polarity: green while the deferral stands, red when it becomes closeable
//!
//! This is a tripwire, not an assertion about correctness. It is GREEN while
//! there is no verifier, and RED the moment one appears — at which point the
//! real test becomes writable and this file has done its job. **Finishing the
//! work forces the paperwork, rather than leaving a stale "not built yet"
//! behind.**
//!
//! ⚠️ **THIS FILE CANNOT RUN UNDER `cargo test` AT THIS COMMIT.** The crate does
//! not resolve its dependencies — `Cargo.toml` requests three `auth-framework`
//! features that no published version declares — so CI never reaches a
//! compiler. It was verified standalone (`std` only) with
//! `rustc --test --edition 2021`, forced in both directions, before being
//! committed. **Until PR #10 repairs the build it is verified but unwired, and
//! must not be counted as coverage.**

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

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
fn no_block_signature_verifier_exists_yet() {
    let root = repo_root();
    let mut files = Vec::new();
    source_files(&root.join("src"), &mut files);

    // Non-vacuity: an empty walk makes "no definition found" trivially true.
    assert!(
        files.len() > 50,
        "only {} .rs files under src/ — the walk is broken, not the tree",
        files.len()
    );

    // ⚠️ ASSEMBLED, AND THE SUBJECT IS THE DEFINITION FORM. The bare
    // identifier appears twice in this file's own documentation above, so a
    // search for the NAME would match this test and pass forever if the scan
    // were ever widened past `src/`. A search for `fn <name>` cannot: measured,
    // this file contains zero occurrences of that form. The assembly keeps even
    // the definition form from appearing literally. A sibling project's gate
    // shipped the un-assembled version of this defect this week.
    let name = format!("verify_block{}signature", "_");
    let definition = format!("fn {name}");

    let mut defined_in = Vec::new();
    for f in &files {
        if let Ok(text) = fs::read_to_string(f) {
            if text.contains(definition.as_str()) {
                defined_in.push(f.strip_prefix(&root).unwrap_or(f).display().to_string());
            }
        }
    }

    assert!(
        defined_in.is_empty(),
        "A BLOCK-SIGNATURE VERIFIER NOW EXISTS, in {defined_in:?}. That is the deferred \
         work, and this tripwire has done its job.\n\n  \
         WRITE: the assertion tests/security_test.rs defers — that the verifier ACCEPTS a \
         signed block and REJECTS the tampered one built beside it.\n  \
         THEN DELETE: this file, and the deferral note in test_blockchain_block_signing.\n\n\
         ⚠️ Do not simply rename the signing test back. The old name promised verification \
         and printed green while performing none; restoring it would restore that."
    );
}
