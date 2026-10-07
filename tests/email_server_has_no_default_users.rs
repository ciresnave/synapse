// SPDX-License-Identifier: MIT OR Apache-2.0
//! Brute-force hardening P1 (`docs/superpowers/specs/2026-10-07-brute-force-hardening-design.md`,
//! row 14): the email server's auth handler starts with no accounts. It used to create `admin/admin`
//! (admin, may relay) and `emrp/emrp123` on every construction: credentials with a guess space of
//! zero, behind IMAP LOGIN and SMTP AUTH.

use synapse::email_server::{AuthHandler, SynapseAuthHandler, UserPermissions};

fn permissions() -> UserPermissions {
    UserPermissions {
        can_send: true,
        can_receive: true,
        can_relay: false,
        is_admin: false,
    }
}

/// Negative control: no built-in account authenticates, through either constructor.
#[test]
fn a_new_auth_handler_accepts_no_built_in_credentials() {
    for handler in [SynapseAuthHandler::new(), SynapseAuthHandler::default()] {
        for (user, password) in [
            ("admin", "admin"),
            ("emrp", "emrp123"),
            ("testuser", "testpass"),
        ] {
            assert!(
                !handler.authenticate(user, password).unwrap(),
                "a fresh handler accepted the built-in credential {user}/{password}"
            );
        }
    }
}

/// Positive control: an account the operator adds does authenticate, and only with its password.
#[test]
fn an_account_the_operator_adds_authenticates() {
    let handler = SynapseAuthHandler::new();
    handler
        .add_user_with_password(
            "ops",
            "a long operator password",
            "ops@example.com",
            permissions(),
        )
        .unwrap();
    assert!(
        handler
            .authenticate("ops", "a long operator password")
            .unwrap()
    );
    assert!(!handler.authenticate("ops", "wrong").unwrap());
}
