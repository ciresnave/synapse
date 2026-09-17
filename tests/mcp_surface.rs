// SPDX-License-Identifier: MIT OR Apache-2.0
//! MCP stdio surface, in-process — spec: docs/superpowers/specs/2026-09-17-mcp-surface-design.md
//! Test numbers refer to the spec's §7.

use synapse::mcp_server::{McpConfig, SynapseMcpServer};

// §7 test 6 — startup refuses without a key, and says so without naming the path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_refuses_a_missing_key_without_naming_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let key_path = dir.path().join("no-such-key-7f3a.pem");
    let key_path_text = key_path.to_string_lossy().replace('\\', "/");
    let toml = format!(
        "global_id = \"alice@synapse.test\"\n\
         private_key_pem_path = \"{key_path_text}\"\n\
         udp_bind_port = 0\n"
    );
    // Control: the path is in the config, so a leak would be findable.
    assert!(toml.contains(&key_path_text));

    let config = McpConfig::from_toml(&toml).expect("the TOML itself is valid");
    let err = match SynapseMcpServer::start(config).await {
        Ok(_) => panic!("a server started without its key"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("private key"),
        "the error should say what is wrong: {err}"
    );
    assert!(
        !err.contains(&key_path_text) && !err.contains("no-such-key-7f3a"),
        "the error names the key path: {err}"
    );
}
