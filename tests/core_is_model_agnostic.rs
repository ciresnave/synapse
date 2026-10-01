// SPDX-License-Identifier: MIT OR Apache-2.0
//! **The core crate names no LLM vendor and no MCP** (CIRESNAVE-EXPECTATIONS §5.1d).
//!
//! CireSnave, verbatim: *"Make sure Synapse isn't building things claude-specific because it is a
//! communication system for \*any LLM\*."* Claude-specific and MCP code lives in adapter crates
//! under `crates/`. This test holds the core (`src/` and `Cargo.toml`, from `git ls-files`) to that.
//!
//! The needle is assembled at runtime, so this file never matches itself if the scope widens.
//! Each allowlist entry must still match, so the list cannot rot into a blanket exception.

use std::path::Path;
use std::process::Command;

/// One entry, approved by the PM 2026-10-01 (spec §4 Q1): a list of eight vendors' and
/// protocols' mDNS service types. The list treats every vendor alike; it is not Claude-specific.
const ALLOWED: &[(&str, &str)] = &[(
    "src/transport/llm_discovery.rs",
    r#""_anthropic._tcp.local.".to_string(),  // Anthropic Claude"#,
)];

fn needles() -> Vec<String> {
    vec![
        format!("{}{}", "clau", "de"),
        format!("{}{}", "anthro", "pic"),
        format!("{}{}", "mc", "p"),
        format!("{}{}", "rm", "cp"),
    ]
}

fn tracked(root: &Path, pathspecs: &[&str]) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
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

/// Every (file, trimmed line) that contains a needle as a whole word, case-insensitive.
/// "Whole word" means not preceded or followed by an ASCII letter, so `mcp_server` matches
/// `mcp`, and `rmcp` is caught by its own needle rather than hiding behind the `r`.
fn hits(root: &Path, files: &[String]) -> Vec<(String, String)> {
    let needles = needles();
    let mut found = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        for line in text.lines() {
            let lower = line.to_ascii_lowercase();
            if needles.iter().any(|n| contains_word(&lower, n)) {
                found.push((f.clone(), line.trim().to_owned()));
            }
        }
    }
    found
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(i) = haystack[start..].find(needle) {
        let at = start + i;
        let letter = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_alphabetic());
        if !letter(at.checked_sub(1).and_then(|j| bytes.get(j))) && !letter(bytes.get(at + needle.len())) {
            return true;
        }
        start = at + needle.len();
    }
    false
}

#[test]
fn the_core_names_no_vendor_and_no_mcp() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = tracked(root, &["src/", "Cargo.toml"]);
    assert!(
        files.len() > 50,
        "only {} core files from git ls-files: enumeration is broken",
        files.len()
    );

    let found = hits(root, &files);
    let (allowed, forbidden): (Vec<_>, Vec<_>) = found
        .iter()
        .partition(|(f, l)| ALLOWED.iter().any(|(af, al)| af == f && al == l));

    for (af, al) in ALLOWED {
        assert!(
            allowed.iter().any(|(f, l)| f == af && l == al),
            "STALE ALLOWLIST ENTRY: {af}: {al:?} no longer matches; delete it"
        );
    }
    assert!(
        forbidden.is_empty(),
        "the core names an LLM vendor or MCP ({} lines); move it to an adapter crate or make it neutral:\n{}",
        forbidden.len(),
        forbidden
            .iter()
            .map(|(f, l)| format!("  {f}: {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Positive control: the same scan finds hits in the MCP adapter. When `crates/` is absent (the
/// core-only CI job deletes it), the control cannot run, and it says so rather than passing.
#[test]
fn the_scan_finds_mcp_in_the_adapter() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if !root.join("crates/synapse-mcp").exists() {
        eprintln!(
            "CONTROL NOT RUN: crates/synapse-mcp is absent (core-only job); the full job runs it"
        );
        return;
    }
    let files = tracked(root, &["crates/synapse-mcp/"]);
    assert!(
        !files.is_empty(),
        "crates/synapse-mcp exists but git ls-files lists nothing in it"
    );
    assert!(
        !hits(root, &files).is_empty(),
        "the scan finds nothing even in the MCP adapter: it is broken"
    );
}
