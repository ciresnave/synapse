# MCP Stdio Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust MCP stdio server, `synapse-mcp`. Any MCP client can use it to send, poll, list and ack
through synapse, with sender verdicts (slice a) and receiver acknowledgement (slice b).

**Architecture:** `src/mcp_server.rs` (feature `mcp`) holds `McpConfig` (TOML) and `SynapseMcpServer`,
which wraps a `CryptoManager`, a UDP `TransportManager` with a pinned `TrustStore`, an address book, and
two in-memory maps. It exposes four `#[tool]` methods. `src/bin/synapse_mcp.rs` is a thin stdio host.

**Tech Stack:** `rmcp` 3.4.0 (`server`, `macros`, `transport-io`, `schemars`), `schemars` 1.2.2,
`toml` (already a dependency), `tokio`. Dev-only: `rmcp` with `client` and `transport-child-process`.

**Spec:** `docs/superpowers/specs/2026-09-17-mcp-surface-design.md`

## Global Constraints

- Branch `feat/mcp-surface`, stacked on `feat/receiver-ack`. **Draft PR, held** with #37 and #38.
  Its commits must stay self-contained.
- The tool set is exactly `send`, `poll`, `list`, `ack`. No trust-store mutation, anywhere.
- **No private key path or key bytes in any tool result, tool error or log line** (spec §6).
  `McpConfig` must not derive `Debug`.
- `poll` never acknowledges.
- Its tool description contains the spec §4 text **verbatim**.
- New `.rs` files carry the SPDX header.
- `CARGO_TARGET_DIR=C:/Projects/synapse/target`.
- Checks:
  - failing set `{test_transport_error_handling}` under `cargo test --no-fail-fast`;
  - `cargo fmt -- --check` exits 0;
  - `cargo clippy -- -D warnings` exits 0;
  - `cargo clippy --test mcp_surface --test mcp_stdio_process -- -D warnings` exits 0.
- **The `rmcp` 3.4.0 API is verified with the compiler.** Names used below (`Parameters`,
  `CallToolResult`, `Content`, `TokioChildProcess::builder`, `CallToolRequestParams`) come from its docs
  on 2026-09-17. If the compiler disagrees, follow the compiler and record the difference in the PR.
- No version bump.

---

### Task 1: Dependencies, feature, config and startup

**Files:** `Cargo.toml`, `src/lib.rs`, `src/sender_auth.rs` (add `TrustStore::pinned_key_id`),
`src/mcp_server.rs` (new: config and `start`), and `tests/mcp_surface.rs` (new: test 6).

- [ ] **Step 1:** In `Cargo.toml`:
  - Dependencies:
    `rmcp = { version = "3.4.0", features = ["server", "macros", "transport-io", "schemars"], optional = true }`
    and `schemars = { version = "1.2.2", optional = true }`.
  - Feature `mcp = ["dep:rmcp", "dep:schemars", "dep:toml", "dep:tokio"]`, and add `"mcp"` to `native`.
  - Dev-dependencies: `rmcp = { version = "3.4.0", features = ["client", "transport-child-process"] }`.
  - Target: `[[bin]] name = "synapse-mcp"`, `path = "src/bin/synapse_mcp.rs"`,
    `required-features = ["mcp"]`.
- [ ] **Step 2:** Write test 6, a failing test. A config whose `private_key_pem_path` points to a
  missing file makes `SynapseMcpServer::start` return `Err`. The error's `to_string()` must not
  contain the path string. **Control:** the TOML text does contain it.
- [ ] **Step 3:** Implement the following.
  - `McpConfig`: `serde::Deserialize`, **no `Debug`**.
  - `PeerConfig`.
  - `McpConfig::from_toml`.
  - `SynapseMcpServer::start(config)`:
    - Read the key file and parse it. On failure, return fixed messages with no path.
    - Pin the peers with `pin_pem`. A failure names the peer.
    - Build the address book.
    - Build and start the UDP `TransportManager`, **then confirm that
      `get_transport_status()[Udp]` is `Running`**, because `start()` only warns when a transport
      fails.
    - Compute `reply_address`, defaulting to `127.0.0.1:<udp_bind_port>`.
  - `TrustStore::pinned_key_id(&self, global_id) -> Option<String>`.
- [ ] **Step 4:** Run `cargo test --test mcp_surface` and check it passes. Build with default features.
- [ ] **Step 5:** Commit: `feat(mcp): config and startup for synapse-mcp (P2c)`.

### Task 2: The four tools

**Files:** `src/mcp_server.rs`, `tests/mcp_surface.rs` (tests 1–5 and 7).

- [ ] **Step 1:** Write tests 1–5 and 7 from spec §7, calling the tool methods directly on two or three
  in-process servers. The negative delivery check in test 2 uses a canary: an unsigned
  `SecureMessage` sent raw to alice's port, then polled until it appears.
- [ ] **Step 2:** Run them and check that they fail: the tool methods don't exist yet.
- [ ] **Step 3:** Implement `#[tool_router(server_handler)] impl SynapseMcpServer`:
  - `send(Parameters<SendArgs>)`
  - `poll()`
  - `list()`
  - `ack(Parameters<AckArgs>)`

  Each returns `Result<CallToolResult, ErrorData>`. Tool-level refusals are `CallToolResult::error`
  with a text message; results are `CallToolResult::success` with one JSON text block. The JSON shapes
  and descriptions are exactly spec §4's, and `poll`'s description is verbatim.
- [ ] **Step 4:** Run the tests and check that they pass, then run them 3 more times.
- [ ] **Step 5:** Commit: `feat(mcp): send, poll, list and ack tools (P2c)`.

### Task 3: The `synapse-mcp` binary and the process test

**Files:** `src/bin/synapse_mcp.rs`, `tests/mcp_stdio_process.rs`.

- [ ] **Step 1:** Write test 8. It spawns two processes with `env!("CARGO_BIN_EXE_synapse-mcp")` and
  `--config`, with stderr piped and drained by a task.
  - Check that `list_all_tools` gives exactly the four tools.
  - Run send → poll → ack → poll until `Acknowledged`.
  - Collect every tool response text, and each process's whole stderr after shutdown.
  - Assert that neither contains either key path or `PRIVATE KEY`.
  - **Control:** each config file's text does contain its key path.
- [ ] **Step 2:** Run it and check that it fails: there is no binary yet.
- [ ] **Step 3:** Implement the binary.
  - It takes `--config <path>`, or the `SYNAPSE_MCP_CONFIG` environment variable.
  - Logging is `tracing_subscriber` to stderr, with no ANSI colours.
  - Startup errors go to stderr as fixed messages, followed by a nonzero exit.
  - Otherwise it runs `server.serve(stdio()).await?.waiting().await`.
- [ ] **Step 4:** Run it and check that it passes.
- [ ] **Step 5:** Commit: `feat(mcp): synapse-mcp stdio binary (P2c)`.

### Task 4: Mutation, full verification, docs, branch, stop

- [ ] **Step 1: Mutation (spec §7 test 9).** Predict: **only** test 2 fails. Make `poll` call
  `acknowledge` for every message it returns, ignoring errors. Run `cargo test --test mcp_surface`,
  record the output, restore the code, and check that `git status` is clean.
- [ ] **Step 2: Full verification** with the Global Constraints commands. Report the named `ok` count
  and the doc-test count.
- [ ] **Step 3: Inventory.** Add a dated note that an MCP stdio surface is built on
  `feat/mcp-surface` (held), and that `main` still has none. It goes where inventory §2.2 discusses a
  non-Rust agent runtime.
- [ ] **Step 4: Push and report.** Push, then open a draft PR against `feat/receiver-ack`. Send the PM
  a `[READY]`, then **stop and send the summary** the PM asked for: slice d waits for #37, and e and f
  come after it.
