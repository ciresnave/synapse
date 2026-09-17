// SPDX-License-Identifier: MIT OR Apache-2.0
//! Which network interfaces synapse listens on.
//!
//! **Loopback is the default.** A listener bound to every interface is reachable from other
//! machines, and on Windows it also makes the firewall ask the user for permission. Test and
//! doc-test binaries get fresh paths, so every run asked again: 64 prompts were measured on
//! 2026-09-16/17.
//!
//! Binding every interface must be configured explicitly, in one of two places:
//! - `bind_scope = "all_interfaces"` in a transport factory's config map ([`BIND_SCOPE_KEY`]);
//! - `network.bind_scope` in [`crate::Config`].
//!
//! Under [`BindScope::Loopback`], features that only make sense off-box are switched off rather than
//! half-working:
//! - the mDNS transport refuses to open its multicast socket;
//! - NAT traversal skips STUN and UPnP discovery;
//! - email connectivity detection skips external-IP detection.
//!
//! This is the only module that names a wildcard address. `tests/loopback_by_default.rs` enforces
//! that.

use crate::error::{Result, SynapseError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

/// The config-map key transport factories read.
pub const BIND_SCOPE_KEY: &str = "bind_scope";

/// Which interfaces a listener binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindScope {
    /// Listen on 127.0.0.1 only. Nothing on another machine can connect.
    #[default]
    Loopback,
    /// Listen on every interface. Needed to accept peers from other machines, and for mDNS, NAT
    /// discovery and external-IP detection.
    AllInterfaces,
}

impl BindScope {
    /// The address listeners bind in this scope.
    pub fn ip(self) -> IpAddr {
        match self {
            BindScope::Loopback => IpAddr::V4(Ipv4Addr::LOCALHOST),
            BindScope::AllInterfaces => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    }

    /// The socket address a listener on `port` binds in this scope.
    pub fn listen_addr(self, port: u16) -> SocketAddr {
        SocketAddr::new(self.ip(), port)
    }

    pub fn is_all_interfaces(self) -> bool {
        self == BindScope::AllInterfaces
    }

    /// The value [`BIND_SCOPE_KEY`] takes for this scope in a transport config map.
    pub fn config_value(self) -> &'static str {
        match self {
            BindScope::Loopback => "loopback",
            BindScope::AllInterfaces => "all_interfaces",
        }
    }

    /// Read [`BIND_SCOPE_KEY`] from a transport config map. An absent key means loopback; any value
    /// other than `loopback` or `all_interfaces` is an error, never a silent default.
    pub fn from_config_map(config: &HashMap<String, String>) -> Result<Self> {
        match config.get(BIND_SCOPE_KEY).map(String::as_str) {
            None | Some("loopback") => Ok(BindScope::Loopback),
            Some("all_interfaces") => Ok(BindScope::AllInterfaces),
            Some(other) => Err(SynapseError::ConfigurationError(format!(
                "{BIND_SCOPE_KEY} must be \"loopback\" or \"all_interfaces\", not {other:?}"
            ))),
        }
    }

    /// The address a multicast (mDNS) listener binds. Multicast reception needs the wildcard
    /// address, so this refuses under [`BindScope::Loopback`].
    pub fn multicast_listen_addr(self, port: u16) -> Result<SocketAddrV4> {
        match self {
            BindScope::AllInterfaces => Ok(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)),
            BindScope::Loopback => Err(SynapseError::ConfigurationError(
                "mDNS needs to listen on every interface; set bind_scope to all_interfaces"
                    .to_string(),
            )),
        }
    }
}

/// The local address for an outbound UDP socket that will talk to `peer`.
///
/// For an IPv4 loopback peer this is loopback, so talking to this machine never needs a
/// wildcard socket. For any other peer it is `0.0.0.0:0`, as before: a loopback source cannot
/// reach another machine.
pub fn outbound_udp_local_addr(peer: &SocketAddr) -> SocketAddr {
    match peer {
        SocketAddr::V4(v4) if v4.ip().is_loopback() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
        }
        _ => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
    }
}
