// SPDX-License-Identifier: MIT OR Apache-2.0
//! MCP stdio surface over the real protocol: spec §7 test 8.
//!
//! Two `synapse-mcp` processes are driven by an `rmcp` client. Every tool response and each
//! process's whole stderr are searched for its private key path and for `PRIVATE KEY` (PM
//! requirement). Positive controls: each config file does contain its key path, and each stderr
//! does contain the server's own startup log, so the search is looking at real output.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};
use synapse::CryptoManager;
use tokio::io::AsyncReadExt;

const ALICE: &str = "alice@synapse.test";
const BOB: &str = "bob@synapse.test";

struct Process {
    client: RunningService<RoleClient, ()>,
    stderr: tokio::task::JoinHandle<String>,
    key_path: String,
    sealing_key_path: String,
    key_file_marker: String,
    config_text: String,
}

mod common;
use common::free_udp_port;

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

async fn spawn(
    dir: &Path,
    id: &str,
    port: u16,
    private_pem: &str,
    sealing_pem: &str,
    peer: (&str, &str, &str, u16),
) -> Process {
    let key_file_marker = format!("{}-secret-key-9c2e", id.split('@').next().unwrap());
    let key_path = dir.join(format!("{key_file_marker}.pem"));
    std::fs::write(&key_path, private_pem).unwrap();
    let sealing_path = dir.join(format!("{key_file_marker}-sealing.pem"));
    std::fs::write(&sealing_path, sealing_pem).unwrap();
    let (peer_id, peer_pem, peer_sealing_pem, peer_port) = peer;
    let config_text = format!(
        "global_id = \"{id}\"\nprivate_key_pem_path = \"{}\"\nsealing_key_path = \"{}\"\nudp_bind_port = {port}\n\n\
         [[peers]]\nglobal_id = \"{peer_id}\"\npublic_key_pem = \"\"\"\n{peer_pem}\"\"\"\n\
         sealing_public_key = \"\"\"\n{peer_sealing_pem}\"\"\"\n\
         address = \"127.0.0.1:{peer_port}\"\n",
        slash(&key_path),
        slash(&sealing_path)
    );
    let config_path = dir.join(format!("{}.toml", id.split('@').next().unwrap()));
    std::fs::write(&config_path, &config_text).unwrap();

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_synapse-mcp"));
    command.arg("--config").arg(&config_path);
    let (transport, stderr) = TokioChildProcess::builder(command)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn synapse-mcp");
    let mut stderr = stderr.expect("stderr is piped");
    let stderr = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        text
    });
    let client = ().serve(transport).await.expect("MCP handshake");
    Process {
        client,
        stderr,
        key_path: slash(&key_path),
        sealing_key_path: slash(&sealing_path),
        key_file_marker,
        config_text,
    }
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text())
        .map(|t| t.text.clone())
        .collect()
}

async fn call(process: &Process, name: &str, args: Value, transcript: &mut Vec<String>) -> Value {
    let mut params = CallToolRequestParams::new(name.to_string());
    if let Value::Object(map) = args {
        params = params.with_arguments(map);
    }
    let result = process.client.call_tool(params).await.expect("call_tool");
    let text = text_of(&result);
    transcript.push(text.clone());
    assert_ne!(result.is_error, Some(true), "{name} failed: {text}");
    serde_json::from_str(&text).expect("tool output is JSON")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_processes_send_poll_ack_and_leak_no_key_path() {
    let dir = tempfile::tempdir().unwrap();
    let (mut alice_c, mut bob_c) = (CryptoManager::new(), CryptoManager::new());
    let (alice_sk, alice_pk) = alice_c.generate_keypair().unwrap();
    let (bob_sk, bob_pk) = bob_c.generate_keypair().unwrap();
    let (alice_seal_sk, alice_seal_pk) = alice_c.generate_sealing_key().unwrap();
    let (bob_seal_sk, bob_seal_pk) = bob_c.generate_sealing_key().unwrap();
    let (alice_port, bob_port) = (free_udp_port(), free_udp_port());
    let alice = spawn(
        dir.path(),
        ALICE,
        alice_port,
        &alice_sk,
        &alice_seal_sk,
        (BOB, &bob_pk, &bob_seal_pk, bob_port),
    )
    .await;
    let bob = spawn(
        dir.path(),
        BOB,
        bob_port,
        &bob_sk,
        &bob_seal_sk,
        (ALICE, &alice_pk, &alice_seal_pk, alice_port),
    )
    .await;
    let mut transcript = Vec::new();

    let mut names: Vec<String> = alice
        .client
        .list_all_tools()
        .await
        .unwrap()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["ack", "list", "poll", "send"]);

    let listed = call(&alice, "list", json!({}), &mut transcript).await;
    assert_eq!(listed["peers"][0]["global_id"], BOB);

    let sent = call(
        &alice,
        "send",
        json!({"to": BOB, "text": "over stdio"}),
        &mut transcript,
    )
    .await;
    let id = sent["message_id"].as_str().unwrap().to_string();

    let mut delivered = None;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let polled = call(&bob, "poll", json!({}), &mut transcript).await;
        if let Some(first) = polled["messages"].as_array().unwrap().first() {
            delivered = Some(first.clone());
            break;
        }
    }
    let delivered = delivered.expect("bob received the message");
    assert_eq!(delivered["message_id"], id.as_str());
    assert_eq!(delivered["text"], "over stdio");
    assert_eq!(delivered["sender"]["verdict"], "verified");
    assert_eq!(delivered["sealed"], true);

    call(&bob, "ack", json!({"message_id": id}), &mut transcript).await;

    let mut status = String::new();
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let polled = call(&alice, "poll", json!({}), &mut transcript).await;
        if let Some(d) = polled["deliveries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["message_id"] == id.as_str())
        {
            status = d["status"].as_str().unwrap().to_string();
            if status == "Acknowledged" {
                break;
            }
        }
    }
    assert_eq!(status, "Acknowledged");

    // Shut both down and collect every byte they wrote to stderr.
    let _ = alice.client.cancel().await;
    let _ = bob.client.cancel().await;
    let alice_err = tokio::time::timeout(Duration::from_secs(10), alice.stderr)
        .await
        .expect("alice's stderr closes")
        .unwrap();
    let bob_err = tokio::time::timeout(Duration::from_secs(10), bob.stderr)
        .await
        .expect("bob's stderr closes")
        .unwrap();

    let all_responses = transcript.join("\n");
    for (name, key_path, sealing_key_path, marker, config_text, stderr) in [
        (
            "alice",
            &alice.key_path,
            &alice.sealing_key_path,
            &alice.key_file_marker,
            &alice.config_text,
            &alice_err,
        ),
        (
            "bob",
            &bob.key_path,
            &bob.sealing_key_path,
            &bob.key_file_marker,
            &bob.config_text,
            &bob_err,
        ),
    ] {
        // Positive controls: the search could find a leak, and stderr was really captured.
        assert!(config_text.contains(key_path.as_str()), "{name}: control");
        assert!(
            config_text.contains(sealing_key_path.as_str()),
            "{name}: sealing control"
        );
        assert!(
            stderr.contains("UDP transport bound"),
            "{name}: stderr was not captured: {stderr}"
        );
        for haystack in [&all_responses, stderr] {
            assert!(
                !haystack.contains(key_path.as_str())
                    && !haystack.contains(sealing_key_path.as_str())
                    && !haystack.contains(marker.as_str()),
                "{name}: the key path leaked"
            );
            assert!(
                !haystack.contains("PRIVATE KEY"),
                "{name}: private key text leaked"
            );
        }
    }
}
