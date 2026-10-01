// SPDX-License-Identifier: MIT OR Apache-2.0
//! **The core crate names no LLM vendor and no MCP** (CIRESNAVE-EXPECTATIONS §5.1d).
//!
//! CireSnave, verbatim: *"Make sure Synapse isn't building things claude-specific because it is a
//! communication system for \*any LLM\*."* Claude-specific and MCP code lives in adapter crates
//! under `crates/`. This test holds the core (`src/` and `Cargo.toml`, from `git ls-files`) to that.
//!
//! ⚠️ This file names the words it forbids (in this documentation, the quote and its messages),
//! so it must stay OUT of the scanned scope. Widening the scan to `tests/` or `.` requires
//! excluding this file by path. Only the needles are assembled at runtime.
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

/// Every (file, trimmed line) that contains a needle anywhere, case-insensitive. Plain substrings
/// on purpose: `McpTransport` or `ClaudeAdapter` is exactly what a misplaced adapter type looks like
/// (review finding I1). A false positive gets an allowlist entry, which is self-checking.
fn hits(root: &Path, files: &[String]) -> Vec<(String, String)> {
    let needles = needles();
    let mut found = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        for line in text.lines() {
            if line_hits_with(line, &needles) {
                found.push((f.clone(), line.trim().to_owned()));
            }
        }
    }
    found
}

fn line_hits(line: &str) -> bool {
    line_hits_with(line, &needles())
}

fn line_hits_with(line: &str, needles: &[String]) -> bool {
    let lower = line.to_ascii_lowercase();
    needles.iter().any(|n| lower.contains(n.as_str()))
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

/// Review finding I1: a vendor or MCP name glued to more letters (`McpTransport`,
/// `ClaudeAdapter`, `AnthropicClient`) is exactly what an adapter type in the core would look like,
/// so the scan matches substrings, not whole words.
#[test]
fn the_scan_catches_names_inside_identifiers() {
    for line in [
        "pub struct McpTransport;",
        "pub struct ClaudeAdapter;",
        "pub struct AnthropicClient;",
        "use rmcp::model;",
    ] {
        assert!(line_hits(line), "the scan missed {line:?}");
    }
    assert!(
        !line_hits("pub struct NeutralTransport;"),
        "control: a neutral line must not hit"
    );
}
