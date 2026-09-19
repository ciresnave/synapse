// SPDX-License-Identifier: MIT OR Apache-2.0
//! **Synapse listens on loopback unless told otherwise.**
//!
//! Windows asks the user for firewall permission whenever a program listens on a non-loopback
//! address. Test and doc-test binaries get fresh paths, so every run asked again: 64 prompts were
//! measured on 2026-09-16/17 (Windows Firewall event 2097, "Query User"). The default is now
//! `BindScope::Loopback`; all-interfaces binding must be configured explicitly.
//!
//! This file checks the defaults. It also guards the source: no compiled module may name a wildcard
//! address except `src/network_scope.rs`, so every bind goes through the scope. It binds no
//! sockets itself.

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use synapse::Config;
use synapse::network_scope::{BindScope, outbound_udp_local_addr};

#[test]
fn the_default_scope_is_loopback() {
    assert_eq!(BindScope::default(), BindScope::Loopback);
    assert_eq!(
        BindScope::default().listen_addr(47000),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 47000)
    );
    assert!(BindScope::default().listen_addr(0).ip().is_loopback());
    assert_eq!(Config::default().network.bind_scope, BindScope::Loopback);
}

#[test]
fn the_scope_parses_from_a_transport_config_map() {
    let empty = HashMap::new();
    assert_eq!(
        BindScope::from_config_map(&empty).unwrap(),
        BindScope::Loopback
    );

    let mut all = HashMap::new();
    all.insert("bind_scope".to_string(), "all_interfaces".to_string());
    assert_eq!(
        BindScope::from_config_map(&all).unwrap(),
        BindScope::AllInterfaces
    );
    assert!(!BindScope::AllInterfaces.listen_addr(1).ip().is_loopback());

    let mut bad = HashMap::new();
    bad.insert("bind_scope".to_string(), "everywhere".to_string());
    assert!(BindScope::from_config_map(&bad).is_err());
}

#[test]
fn a_config_file_without_a_network_section_is_loopback() {
    let config: Config = toml::from_str(&toml::to_string(&Config::default()).unwrap()).unwrap();
    assert_eq!(config.network.bind_scope, BindScope::Loopback);

    let mut value: toml::Value = toml::Value::try_from(Config::default()).unwrap();
    value.as_table_mut().unwrap().remove("network");
    let without: Config = value.try_into().expect("an older config file still loads");
    assert_eq!(without.network.bind_scope, BindScope::Loopback);
}

#[test]
fn outbound_sockets_bind_loopback_for_loopback_peers() {
    let local = outbound_udp_local_addr(&"127.0.0.1:9".parse().unwrap());
    assert!(local.ip().is_loopback(), "{local}");
    assert_eq!(local.port(), 0);
    // An off-box peer cannot be reached from a loopback source, so this keeps the old wildcard.
    let remote = outbound_udp_local_addr(&"192.0.2.1:9".parse().unwrap());
    assert!(!remote.ip().is_loopback(), "{remote}");
}

/// Transport files that compile in no configuration: `src/transport/mod.rs` declares no module for
/// them (CAPABILITY_INVENTORY.md §5.3). They are excluded BY NAME, and the guard checks that the
/// exclusion is still true.
///
/// Task 1 of the transport contract deleted every file that was in this list (they never compiled,
/// so their deletion changes no behaviour). The list is empty rather than removed so this test keeps
/// meaning what it says if a future change reintroduces an uncompiled transport file.
const NOT_COMPILED: [&str; 0] = [];

/// The one file allowed to name a wildcard address.
const HOME: &str = "src/network_scope.rs";

/// Assembled at runtime so this file never contains its own needles.
fn needles() -> Vec<String> {
    vec![
        ["0", "0", "0", "0"].join("."),
        ["UNSPE", "CIFIED"].concat(),
        ["[", "::", "]"].concat(),
        ["Ipv4Addr::new(0", " 0", " 0", " 0)"].join(","),
    ]
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_compiled_module_names_a_wildcard_address_outside_network_scope() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let transport_mod = fs::read_to_string(root.join("src/transport/mod.rs")).unwrap();
    for excluded in NOT_COMPILED {
        assert!(root.join(excluded).exists(), "{excluded} no longer exists");
        let module = Path::new(excluded).file_stem().unwrap().to_string_lossy();
        let declared = transport_mod.lines().any(|line| {
            let line = line.trim_start();
            !line.starts_with("//")
                && (line.starts_with(&format!("pub mod {module};"))
                    || line.starts_with(&format!("mod {module};")))
        });
        assert!(
            !declared,
            "{excluded} is now a compiled module: remove it from NOT_COMPILED"
        );
    }

    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    files.sort();
    let needles = needles();
    let mut scanned = Vec::new();
    let mut home_hits = 0;
    let mut offenders = Vec::new();
    for path in &files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if NOT_COMPILED.contains(&relative.as_str()) {
            continue;
        }
        scanned.push(relative.clone());
        let text = fs::read_to_string(path).unwrap();
        for (number, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if needles.iter().any(|n| code.contains(n.as_str())) {
                if relative == HOME {
                    home_hits += 1;
                } else {
                    offenders.push(format!("{relative}:{}: {}", number + 1, code));
                }
            }
        }
    }
    println!("excluded (compile in no configuration): {NOT_COMPILED:?}");
    println!("scanned {} files: {scanned:?}", scanned.len());
    // Control: the guard does find the wildcard where it is allowed to be.
    assert!(
        home_hits > 0,
        "the scan found nothing even in {HOME}; it is not looking at the source"
    );
    assert!(
        offenders.is_empty(),
        "wildcard addresses outside {HOME}; route them through BindScope:\n{}",
        offenders.join("\n")
    );
}
