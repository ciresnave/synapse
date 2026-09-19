// SPDX-License-Identifier: MIT OR Apache-2.0
//! Helpers shared by the integration tests (`mod common;` in each test file that uses them).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

/// The ports [`free_udp_port`] hands out: `20000..32000`, below every common OS ephemeral range
/// (Windows and macOS start at 49152, Linux at 32768).
const FIRST: u32 = 20_000;
const SPAN: u32 = 12_000;

/// A loopback UDP port that is free now, and that no other call in this process returns.
///
/// The old helper, repeated in each test file, bound `127.0.0.1:0`, read the port and released
/// it for the caller to bind later. In that window any `bind(":0")` — a parallel test's own
/// `free_udp_port`, a raw sender, a transport's temporary socket — could be given the same
/// ephemeral port, and the node under test then failed to start (`os error 10048`). Ports here
/// come from outside the ephemeral range, so the OS never hands them to a `:0` bind, and a
/// process-wide counter never hands one out twice. Each process starts at an offset derived from
/// its pid, so test binaries running at the same time walk different ports.
pub fn free_udp_port() -> u16 {
    static START: OnceLock<u32> = OnceLock::new();
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let start = *START.get_or_init(|| std::process::id().wrapping_mul(7919) % SPAN);
    for _ in 0..SPAN {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let port = (FIRST + (start + n) % SPAN) as u16;
        // Skip anything already bound, or excluded by the OS (Windows reserves port ranges).
        if std::net::UdpSocket::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    panic!("no free loopback UDP port in {FIRST}..{}", FIRST + SPAN);
}
