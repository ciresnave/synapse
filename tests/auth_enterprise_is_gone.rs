// SPDX-License-Identifier: MIT OR Apache-2.0
//! Brute-force hardening P3 (`docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md`,
//! row 17): `auth_enterprise` and `auth_v4_example` are deleted, not hardened. They declared a
//! lockout, rate-limit counters and real-time alerts that nothing ever enforced or read, and
//! nothing outside three examples used them. The orphan `auth_integration.rs`, which `lib.rs` never
//! declared, goes with them.

use std::path::Path;

const DELETED: &[&str] = &[
    "src/auth_enterprise.rs",
    "src/auth_v4_example.rs",
    "src/auth_integration.rs",
    "examples/enterprise_ai_auth_platform.rs",
    "examples/federated_auth_demo.rs",
    "examples/auth_framework_v4_demo.rs",
    "docs/ENTERPRISE_AI_PLATFORM.md",
];

#[test]
fn the_enterprise_auth_modules_and_their_examples_are_deleted() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in DELETED {
        assert!(!root.join(path).exists(), "{path} should be deleted");
    }
    // Control: a kept module and a kept example exist, so the check looks in the right place.
    assert!(root.join("src/sealing.rs").exists());
    assert!(root.join("examples/hello_world.rs").exists());
}

#[test]
fn lib_rs_no_longer_declares_them() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    for module in ["pub mod auth_enterprise;", "pub mod auth_v4_example;"] {
        assert!(!lib.contains(module), "lib.rs still declares `{module}`");
    }
    assert!(
        lib.contains("pub mod sealing;"),
        "control: a kept module is declared"
    );
}
