# M0: workspace split and model-agnostic guard (design)

**Status:** DRAFT for PM approval. Nothing is built.
**Plan:** `docs/superpowers/plans/2026-09-29-claude-peers-replacement.md`, milestone M0. The PM cleared M0 to start on
2026-10-01 (board 82).
**Measured at:** `origin/main@a45685f` (4.0.0).
**Rule (CIRESNAVE-EXPECTATIONS §5.1d, verbatim):** *"Make sure Synapse isn't building things claude-specific
because it is a communication system for \*any LLM\*."*

## 1. Goal

After M0, the core crate `synapse` contains no MCP code and no Claude-specific code, and it builds and
passes its tests with no adapter crate present. `synapse-mcp` behaves exactly as it does today, but it
lives in its own crate. A CI job fails if either property regresses.

## 2. What exists today (measured)

- **The MCP code lives in the core crate:**
  - `src/mcp_server.rs` (506 lines), behind `#[cfg(feature = "mcp")]` (lib.rs:209);
  - `src/bin/synapse_mcp.rs`;
  - feature `mcp = ["dep:rmcp", "dep:schemars", "dep:toml", "dep:tokio"]`, which is included in
    `native`, which is `default`;
  - dev-dependency `rmcp` with `client` and `transport-child-process`;
  - tests `tests/mcp_surface.rs` and `tests/mcp_stdio_process.rs`, both using `tests/common::free_udp_port`.
- **The MCP code needs only public core API.** Every `crate::` path in `mcp_server.rs` is a `pub mod`
  item (`crypto`, `error`, `replay`, `sealing`, `sender_auth`, `transport`, `types`). Its own
  `pub(crate)` items are internal to it. So the move is a path rewrite, not an API change. The build
  confirms this or refutes it in Task 1.
- **A scan of core `src/` for `claude|anthropic|mcp`** (case-insensitive, `git grep`) hits 13 files.
  The same scan over `tests/` finds `mcp_*.rs`, which is the control that the scan finds things.
  Outside the two MCP files, every hit is one of these:
  - **Example names in docs and tests** (about 30 hits): `"Claude"` and `claude@anthropic.com` used as
    sample entities in `identity.rs`, `types.rs`, `lib.rs`, `email.rs`, `config.rs`,
    `auth_enterprise.rs`, `auth_v4_example.rs` and `llm_discovery.rs:90`.
  - **Comments naming `synapse-mcp`/MCP**: `sender_auth.rs:559` and `transport/manager.rs:1477`.
  - **A comment citing the portfolio's `CLAUDE.md §7`**: `transport/email_unified.rs:94`.
  - **One functional, vendor-neutral list**: `transport/llm_discovery.rs:176`, the mDNS service types
    `_llm`, `_openai`, `_anthropic`, `_ollama`, `_llamacpp`, `_textgen`, `_vllm` and `_synapse-ai`.

## 3. Design

### 3.1 Layout

```
Cargo.toml                 [package] synapse (unchanged location) + [workspace] members = ["crates/*"]
src/                       core, MCP-free
crates/synapse-mcp/
  Cargo.toml               depends on synapse = { path = "../.." }, rmcp, schemars, toml, tokio
  src/lib.rs               was src/mcp_server.rs (crate:: -> synapse::)
  src/main.rs              was src/bin/synapse_mcp.rs; bin name stays `synapse-mcp`
  tests/mcp_surface.rs, tests/mcp_stdio_process.rs, tests/common/mod.rs (copy of free_udp_port)
```

- **The core stays at the repo root.** Moving it into `crates/` would break every test that reads repo
  files through `CARGO_MANIFEST_DIR` (for example `security_claims_match_the_tree` reads
  `SECURITY_AUDIT_COMPLETION_REPORT.md`), and that churn buys nothing.
- **`members = ["crates/*"]` is a glob.** Deleting `crates/` therefore removes every adapter without
  editing any manifest, which is how the guard job (§3.3) runs. Cargo has to accept a glob that matches
  nothing. Task 1 verifies that. If cargo rejects it, the job rewrites `members` instead, and the PR
  says so.
- **The crate is `synapse-mcp`, and its version tracks the workspace's one version** (the §9 rule: one
  version number per project). Use `version.workspace = true`, with `[workspace.package] version` as
  the single source of truth.

### 3.2 Core changes

- Remove `pub mod mcp_server`, the `mcp` feature, `"mcp"` from `native`, the `rmcp` and `schemars`
  dependencies, the `rmcp` dev-dependency, and the `[[bin]] synapse-mcp`.
- Keep `toml` and `tokio`: other features use them. Task 1 runs a `cargo tree` check that the removal
  leaves both reachable where needed.
- **This is breaking:** the `mcp` feature and `synapse::mcp_server` disappear. The PM allocates the
  version at gate time.

### 3.3 The guard (plan §4)

A new CI job, **`core is model-agnostic`**, runs on GitHub-hosted `ubuntu-latest` like every other job:
1. **The core builds alone.** `rm -rf crates/`, then `cargo test -p synapse --all-features
   --no-fail-fast`. The exclusions are the ones CI already applies for `enhanced-auth`; the job reuses
   whatever feature set "Build and Test" uses, so it doesn't diverge.
2. **No MCP in the core's dependency tree.**
   - `cargo tree -p synapse -e normal,build --all-features` (default features as well, if they differ)
     **must not contain `rmcp`**.
   - Positive control, run *before* step 1 deletes `crates/`: `cargo tree -p synapse-mcp` **must**
     contain `rmcp`.
   - The job prints both counts beside each other.
3. **No vendor names in the core.** A test, `tests/core_is_model_agnostic.rs`, enumerates core files
   with `git ls-files src/` (portfolio §5b: from the index, never a disk walk). It fails on any match of
   `(?i)claude|anthropic|\brmcp\b|\bmcp\b`.
   - **Allowlist:** see Q1.
   - **Each allowlist entry must still match.** A stale entry fails, so the list can't rot into a
     blanket exception.
   - **The needle is assembled at runtime**, as `security_claims_match_the_tree` does, so this file
     never matches itself even if the scope widens.
   - **Positive control:** the same function run over `git ls-files crates/synapse-mcp/` must find more
     than 0 hits. Under step 1's `rm -rf crates/`, that control would be vacuous, so the test detects the
     missing directory and says so instead of passing silently.

### 3.4 Cleanup the name scan forces (mechanical)

- **Example names** become neutral (`"Alice"`, `"ResearchBot"`, `assistant@ai-lab.example.com`),
  keeping each example's meaning.
- **The two MCP comments** are reworded to "adapter" or "a library consumer".
- **The `CLAUDE.md §7` comment** becomes "the portfolio's measurement rule".
- No behaviour changes. The test suite is the check, compared by name against `a45685f`.

## 4. Questions for the PM

- **Q1. What to do with the mDNS list `_anthropic._tcp.local.` (llm_discovery.rs:176).** It sits in a
  list that treats eight vendors and protocols alike, so it is vendor-neutral, not Claude-specific.
  Options:
  - **(a) Recommended:** an allowlist with exactly this one entry (file plus exact line text), its reason
    written beside it, and the stale-entry check from §3.3.
  - **(b)** Delete the entry. This silently drops discovery of one vendor's services.
  - **(c)** Delete every vendor entry, keeping only `_llm` and `_synapse-ai`. That is a behaviour
    change outside M0's scope.
- **Q2. Does the name scan cover core `tests/` too, or only `src/`?** Recommended: `src/` plus
  `Cargo.toml`. Tests may legitimately name a vendor as test data, and the rule is about what the core
  *is*, not what its tests feed it.

## 5. Acceptance

- Run the full `cargo test --no-fail-fast` (stable, with `cargo --version` printed) on the branch and at
  `a45685f`, and compare by test name. The only allowed differences are the two moved MCP test targets
  (same test names, new binary) and the new guard test.
- `cargo clippy --all-targets -D warnings` and `fmt` pass for the whole workspace and for the core
  alone.
- **Sabotage, predicted and then observed** (each mutation is confirmed present before its run):
  - re-adding `rmcp` to core deps turns step 2 red;
  - adding `"claude"` to a core string turns step 3 red;
  - a stale allowlist entry turns step 3 red;
  - deleting `crates/synapse-mcp` turns the positive control red, with its message, not green.
- `synapse-mcp`'s process test passes when run from the new crate, which shows the binary still works
  end to end.

## 6. Size

One PR in three commits: the move, the cleanup, the guard. Estimated at 0.5–1 lane-day, as in the
plan.
