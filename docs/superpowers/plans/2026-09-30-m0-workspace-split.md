# M0: workspace split and model-agnostic guard (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move all MCP code out of the core `synapse` crate into `crates/synapse-mcp`, and add a CI job
plus a test that keep the core free of MCP and vendor-specific code.

**Architecture:**
- The root stays the `synapse` package and also becomes a workspace with `members = ["crates/*"]`.
  Deleting `crates/` therefore yields a core-only build with no manifest edits.
- `synapse-mcp` depends on `synapse` by path and uses only its public API.

**Tech stack:** cargo workspaces, rmcp 3.x, GitHub Actions (`ubuntu-latest`), Python-free (the guard
test is Rust, std only).

**Spec:** `docs/superpowers/specs/2026-09-30-m0-workspace-split-design.md`. The PM approved §4 on
2026-10-01: Q1 is (a), a one-entry self-checking allowlist; Q2 scans `src/` plus `Cargo.toml` only.

## Global constraints

- **Toolchain:** run cargo from PowerShell with
  `$env:PATH='C:\Users\cires\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin;'+$env:PATH`
  (`~/.cargo/bin` is being rewritten; see HANDOFF). Print `cargo --version` in every captured log.
- **One version number:** `synapse-mcp` uses `version.workspace = true`, and
  `[workspace.package] version` is the single source. Do NOT bump the number; the PM allocates it at
  the gate.
- **Licence header:** every new `.rs` file starts with `// SPDX-License-Identifier: MIT OR Apache-2.0`
  (the SPDX gate enforces it).
- **Enumerate from the index:** `git ls-files`, never a disk walk (portfolio §5b).
- **No behaviour change** to `synapse-mcp`: same tool names, descriptions, config and secret hygiene.

## Review focus

1. **The moved MCP tests must still run in CI.**
   - The risk: a root package with members makes plain `cargo test` cover only the root, so the MCP
     tests would silently stop running.
   - Task 1 switches CI to `--workspace` and `cargo fmt --all`, and the final comparison checks the two
     test targets by name.
2. **`rm -rf crates/` must leave a resolvable workspace.** If cargo rejects a member glob that matches
   nothing, the guard job dies early and asserts nothing (CLAUDE.md §5). Task 1 Step 6 measures this.
3. **The guard must not pass vacuously.**
   - An empty file list, a broken needle, or a missing adapter directory all fail loudly.
   - Task 2 includes sabotage runs for each.
4. **The guard test must not match itself:** the needle is assembled at runtime.
5. **The enhanced-auth guard's error codes must not drift.** `enhanced_auth_guard.py` runs
   `cargo check --all-features --all-targets` at the root. Before M0, `--all-features` compiled
   `mcp_server.rs`; after M0 it doesn't. Task 1 Step 8 runs the guard locally.

---

### Task 1: Create the workspace and move the MCP code

**Files:**
- Modify: `Cargo.toml`. Add `[workspace]` + `[workspace.package]`; remove `rmcp`/`schemars` deps (lines
  80–82), the `rmcp` dev-dep (185–186), `[[bin]] synapse-mcp` (280–283), feature `mcp` (389–390), and
  `"mcp"` in `native` (424).
- Modify: `src/lib.rs:209-210`. Delete `#[cfg(feature = "mcp")] pub mod mcp_server;`.
- Move: `src/mcp_server.rs` → `crates/synapse-mcp/src/lib.rs` (`git mv`).
- Move: `src/bin/synapse_mcp.rs` → `crates/synapse-mcp/src/main.rs` (`git mv`).
- Move: `tests/mcp_surface.rs`, `tests/mcp_stdio_process.rs` → `crates/synapse-mcp/tests/` (`git mv`).
- Create: `crates/synapse-mcp/tests/common/mod.rs`, a copy of `tests/common/mod.rs`. The core's other
  tests still use the original, so it stays.
- Create: `crates/synapse-mcp/Cargo.toml`.
- Modify: `.github/workflows/ci_cd.yml:165,171,177,186`.
- Modify: `CAPABILITY_INVENTORY.md:854`. Update the binary's location; keep the sentence's claim.

**Interfaces:**
- Produces: crate `synapse_mcp` (lib) exporting `McpConfig`, `SynapseMcpServer`, `SendArgs` and `AckArgs`,
  the same items as `synapse::mcp_server` today; and binary `synapse-mcp`.
- Consumes: `synapse::{crypto, error, replay, sealing, sender_auth, transport, types}`, all `pub mod`
  today.

- [ ] **Step 1: Baseline.** On `a45685f` (a detached worktree), capture
  `cargo --version; cargo test --no-fail-fast` to `scratchpad/m0-base-test.txt`. Expected: exit 0.
- [ ] **Step 2: The failing check first.** The test file in Step 4 exists only after the move, so the
  red step is the build itself. Run the move from Step 3 *without* the manifest, then
  `cargo build -p synapse-mcp`. Expected: an error that the package `synapse-mcp` is not found. That
  proves the new target doesn't exist yet.
- [ ] **Step 3: Move.** Use `git mv` for the four files. In the moved `lib.rs`, replace every `crate::`
  with `synapse::`. Its own `pub(crate)` items stay `pub(crate)`, because they are internal to the new
  crate. In `main.rs` and the two tests, replace `synapse::mcp_server::` with `synapse_mcp::`.
- [ ] **Step 4: Manifests.**
  - Root `Cargo.toml`, before `[package]`:
    ```toml
    [workspace]
    members = ["crates/*"]
    resolver = "3"

    [workspace.package]
    version = "4.0.0"
    ```
    In `[package]`, change `version = "4.0.0"` to `version.workspace = true`.
  - `crates/synapse-mcp/Cargo.toml`:
    ```toml
    [package]
    name = "synapse-mcp"
    version.workspace = true
    edition = "2024"
    rust-version = "1.88"
    authors = ["Eric Evans <ciresnave@gmail.com>"]
    description = "MCP stdio adapter for Synapse: lets any MCP client send, poll, list and ack."
    license = "MIT OR Apache-2.0"
    repository = "https://github.com/ciresnave/synapse"

    [lib]
    name = "synapse_mcp"
    path = "src/lib.rs"

    [[bin]]
    name = "synapse-mcp"
    path = "src/main.rs"

    [dependencies]
    synapse = { path = "../..", version = "4.0.0" }
    rmcp = { version = "3.4.0", features = ["server", "macros", "transport-io", "schemars"] }
    schemars = "1.2.2"
    serde = { version = "1.0", features = ["derive"] }
    serde_json = "1.0"
    toml = "0.9.2"
    tokio = { version = "1.46.1", features = ["full"] }
    tracing = "0.1"
    tracing-subscriber = "0.3"

    [dev-dependencies]
    rmcp = { version = "3.4.0", features = ["client", "transport-child-process"] }
    tempfile = "3.0"
    ```
  - Trim this dependency list to what the moved files actually `use` (read the `use` lines). Add any
    that the compiler names as missing. Don't keep unused ones: cargo's `unused dependency` warnings
    are already a backlog item.
- [ ] **Step 5: Build both.** Run `cargo build --workspace`, then
  `cargo test -p synapse-mcp --no-fail-fast`. Expected: `mcp_surface` reports 13 passed and
  `mcp_stdio_process` 1 passed, the same names as in the baseline log. If an item is reported private,
  STOP and report it. Making core items `pub` is an API change the spec didn't approve.
- [ ] **Step 6: Measure Review focus 2.** Copy the worktree with `crates/` deleted to a scratch
  directory (`git worktree add` + `rm -rf crates`), then run `cargo test -p synapse --no-fail-fast`
  there.
  - Expected: it resolves and passes.
  - If cargo rejects the empty glob, record the exact error. The CI job in Task 2 then rewrites
    `members = []` with a single `sed` that asserts exactly one match.
- [ ] **Step 7: CI.** In `ci_cd.yml`:
  - `cargo build --verbose` becomes `cargo build --workspace --verbose`;
  - `cargo test --verbose --no-fail-fast` becomes `cargo test --workspace --verbose --no-fail-fast`;
  - `cargo fmt -- --check` becomes `cargo fmt --all -- --check`;
  - `cargo clippy --all-targets -- -D warnings` becomes
    `cargo clippy --workspace --all-targets -- -D warnings`.

  Add one comment line above the test step: `# --workspace: a root package with members tests only
  itself by default, which would silently drop crates/synapse-mcp's tests.`
- [ ] **Step 8: Run the enhanced-auth guard locally** with `python .github/enhanced_auth_guard.py`.
  Expected: the same verdict as on `a45685f`. Run it there too, and compare the printed error-code
  sets.
- [ ] **Step 9: Run everything:** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace --no-fail-fast`.
- [ ] **Step 10: Commit.**
  `refactor!: move the MCP surface into crates/synapse-mcp; core drops the mcp feature`.

### Task 2: The guard (test plus CI job), born red

**Files:**
- Create: `tests/core_is_model_agnostic.rs`.
- Modify: `.github/workflows/ci_cd.yml`. Add a job after `Build and Test`.

**Interfaces:**
- Consumes: Task 1's layout (`crates/synapse-mcp/` exists, and the core has no `mcp` feature).
- Produces: the test name `the_core_names_no_vendor_and_no_mcp` and the CI job name
  `core is model-agnostic`.

- [ ] **Step 1: Write the test.**

```rust
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
    assert!(out.status.success(), "git ls-files failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect()
}

/// Every (file, trimmed line) that contains a needle as a whole word, case-insensitive.
/// "Whole word" = not preceded or followed by an ASCII letter, so `mcp` matches `mcp_server`
/// and `rmcp`'s `mcp` does not hide behind the `r` (it is checked by its own needle below).
fn hits(root: &Path, files: &[String]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(root.join(f)) else { continue };
        for line in text.lines() {
            let lower = line.to_ascii_lowercase();
            let hit = needles().iter().any(|n| contains_word(&lower, n))
                || contains_word(&lower, &format!("r{}", "mcp"));
            if hit {
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
        let before = at.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(at + needle.len()).copied();
        let letter = |b: Option<u8>| b.is_some_and(|b| b.is_ascii_alphabetic());
        if !letter(before) && !letter(after) {
            return true;
        }
        start = at + 1;
    }
    false
}

#[test]
fn the_core_names_no_vendor_and_no_mcp() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = tracked(root, &["src/", "Cargo.toml"]);
    assert!(files.len() > 50, "only {} core files from git ls-files: enumeration is broken", files.len());

    let found = hits(root, &files);
    let (allowed, forbidden): (Vec<_>, Vec<_>) =
        found.iter().partition(|(f, l)| ALLOWED.iter().any(|(af, al)| af == f && al == l));

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
        forbidden.iter().map(|(f, l)| format!("  {f}: {l}")).collect::<Vec<_>>().join("\n")
    );
}

/// Positive control: the same scan finds hits in the MCP adapter. When `crates/` is absent (the
/// core-only CI job deletes it), the control cannot run, and it says so rather than passing.
#[test]
fn the_scan_finds_mcp_in_the_adapter() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = tracked(root, &["crates/synapse-mcp/"]);
    if !root.join("crates/synapse-mcp").exists() {
        eprintln!("CONTROL NOT RUN: crates/synapse-mcp is absent (core-only job); the full job runs it");
        return;
    }
    assert!(!files.is_empty(), "crates/synapse-mcp exists but git ls-files lists nothing in it");
    assert!(!hits(root, &files).is_empty(), "the scan finds nothing even in the MCP adapter: it is broken");
}
```

- [ ] **Step 2: Run it, and see it red for the stated reason.**
  `cargo test --test core_is_model_agnostic`. Expected:
  - `the_core_names_no_vendor_and_no_mcp` FAILS. The list it prints must include the hits measured in
    spec §2: `identity.rs`, `types.rs`, `lib.rs`, `email.rs`, `config.rs`, `auth_enterprise.rs`,
    `auth_v4_example.rs`, `llm_discovery.rs:90`, `sender_auth.rs:559`, `transport/manager.rs:1477`,
    and `transport/email_unified.rs:94`.
  - It must NOT include `llm_discovery.rs:176`, because the allowlist covers that line.
  - `the_scan_finds_mcp_in_the_adapter` PASSES.
  - Save the output to `scratchpad/m0-guard-red.txt`.
- [ ] **Step 3: Add the CI job** after `build_and_test`:

```yaml
  model_agnostic_core:
    name: core is model-agnostic
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install stable toolchain
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          override: true
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y libssl-dev pkg-config
      - name: Positive control -- the adapter does depend on rmcp
        run: |
          set -o pipefail
          n=$(cargo tree -p synapse-mcp -e normal --prefix none | grep -c '^rmcp ')
          echo "rmcp in synapse-mcp's tree: $n"
          test "$n" -ge 1
      - name: The core's dependency tree has no rmcp
        run: |
          set -o pipefail
          n=$(cargo tree -p synapse --all-features -e normal,build --prefix none | { grep -c '^rmcp ' || true; })
          echo "rmcp in synapse's tree (all features): $n"
          test "$n" -eq 0
      - name: Delete every adapter crate
        run: rm -rf crates/ && test ! -e crates
      - name: The core builds and passes with no adapter present
        run: cargo test -p synapse --no-fail-fast
```

  Install the same dependencies `Build and Test` installs: copy its `Install dependencies` step
  verbatim (`ci_cd.yml:130-134`) instead of the guess above. If Task 1 Step 6 found that the empty
  glob is rejected, add before `cargo test` a step that runs
  `sed -i 's/^members = \["crates\/\*"\]$/members = []/' Cargo.toml && grep -c '^members = \[\]$' Cargo.toml`.
  The grep must print 1.
- [ ] **Step 4: Sabotage, each one predicted, confirmed present and then run.** Revert each before the
  next.
  - (a) Change the allowlist line's text by one character. Expected: a STALE ALLOWLIST ENTRY failure.
  - (b) Point `tracked` at `&["nonexistent/"]`. Expected: the enumeration-is-broken failure.
  - (c) Change `needles()` to return only an impossible string. Expected:
    `the_scan_finds_mcp_in_the_adapter` FAILS. That proves the control catches a dead scan.
  - (d) On the CI-step logic, run locally in bash: temporarily add
    `rmcp = { version = "3.4.0", optional = true }` plus `x = ["dep:rmcp"]` to the core. Expected: the
    `cargo tree --all-features` count becomes ≥ 1, so the step fails.
  - Record each mutation's diff and the observed output in `scratchpad/m0-sabotage.txt`.
- [ ] **Step 5: Commit** (with the guard still red):
  `test: the core names no LLM vendor and no MCP (born red)`.

### Task 3: Neutralize the core's vendor names (turns the guard green)

**Files (each line is from spec §2; re-read each before editing):**
- `src/identity.rs:11,23,29,334,340,873,876,877`
- `src/types.rs:16,27,47,78,189,329,405`
- `src/lib.rs:23`
- `src/email.rs:259,266,270`
- `src/config.rs:422,423`
- `src/auth_enterprise.rs:75,1372`
- `src/auth_v4_example.rs:336,375`
- `src/transport/llm_discovery.rs:90`
- `src/sender_auth.rs:559`
- `src/transport/manager.rs:1477`
- `src/transport/email_unified.rs:94`

**Interfaces:** none. These are strings, comments and test data only. No public names change.

- [ ] **Step 1: Replacement table.** Apply it consistently, so tests that compare one string with
  another stay equal:

| from | to |
|---|---|
| `"Claude"` (entity or local name) | `"Assistant"` |
| `claude@anthropic.com`, `claude@anthropic.ai`, `claude-3@anthropic.ai` | `assistant@ai-lab.example.com` |
| `"claude-3"` (agent id) | `"assistant-1"` |
| `"claude-3.5"` (a model_version example) | `"model-1.0"` |
| `create_ai_model("Claude", "anthropic.ai")` | `create_ai_model("Assistant", "ai-lab.example.com")` |
| `"Claude-3"` in the `llm_discovery.rs:90` doc list | `"Llama-3"` (keeps the list a list of model names) |
| `"[Synapse Tool Call] Claude → FileSystem"` | `"[Synapse Tool Call] Assistant → FileSystem"` |
| the `sender_auth.rs:559` comment `synapse-mcp` | `an adapter` |
| the `manager.rs:1477` comment `the MCP config` | `an adapter's config` |
| the `email_unified.rs:94` comment `(CLAUDE.md §7: …` | `(the portfolio's measurement rule: …` |

- [ ] **Step 2: Run** `cargo test --test core_is_model_agnostic`. Expected: both tests PASS. If a line
  is still listed, it was missed; fix it, and don't widen the allowlist.
- [ ] **Step 3: Run the full suite:**
  `cargo test --workspace --no-fail-fast > scratchpad/m0-branch-test.txt`. Compare by name against
  `m0-base-test.txt`, using the same `awk` extraction as for honest-keys. The only allowed differences:
  - `mcp_surface`/`mcp_stdio_process` appear under the new crate's binaries, with the same test names
    and outcomes;
  - the 2 new guard tests appear.
- [ ] **Step 4: Lint:** `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 5: Commit:** `refactor: neutral example names in the core; the agnostic guard is green`.

### After the tasks

- One fresh whole-branch review: a single Sonnet reviewer, per the token-budget memory. It reads the
  spec, this plan and the diff, and re-runs sabotage (b) and (d) itself.
- Push to `m0/workspace-split`, take PR #64 out of draft, and update its body with the by-name
  comparison and the sabotage table. Send `[READY]` to the PM.
