// SPDX-License-Identifier: MIT OR Apache-2.0
//! The names the CLI and the MCP adapters build on stay exported from `synapse_client`.

use serde_json::json;
use synapse_client::{Daemon, MailError, encode_body, events, inbox_line, printable};

#[test]
fn the_shared_client_surface_is_exported() {
    let _connect: fn(&std::path::Path) -> Result<Daemon, MailError> = Daemon::connect;
    assert_eq!(events::EVENTS_FILE, "cli-security-events.jsonl");
    assert_eq!(printable("a\u{1b}b"), "a\\u{1b}b");
    let encoded = encode_body(b"hi");
    let line = inbox_line(&json!({ "body_b64": encoded }));
    assert_eq!(line["body"], "hi");
}
