// SPDX-License-Identifier: MIT OR Apache-2.0
//! Key management for Synapse block signing (`utils`).
//!
//! This module used to hold `SynapseAuth`: password login, JWT validation, an HTTP-style auth API,
//! middleware and a trust bridge. Brute-force hardening P2 deleted all of it rather than harden it
//! (`docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md`, rows 15 and 16): it had an
//! unsalted SHA-256 password check with a user-existence oracle, a JWT verify secret that fell back
//! to a public constant, and a hardcoded auth-framework secret key, and nothing outside the module
//! used it. Only the key manager had outside users, so only it remains.

pub mod utils;

pub use utils::SynapseKeyManager;
