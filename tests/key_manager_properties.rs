// SPDX-License-Identifier: MIT OR Apache-2.0
//! Properties that `synapse::synapse::auth::utils` must have.
//!
//! Each test states a property a real implementation has and a placeholder
//! does not. An algorithm this crate cannot perform must be refused with an
//! `Err`, never answered with an `Ok` that has no key behind it.

use ring::digest;
use synapse::synapse::auth::utils::{KeyAlgorithm, KeyManager, KeyPair, SynapseKeyManager};

fn sha256(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA256, data).as_ref().to_vec()
}

#[tokio::test]
async fn rsa_key_generation_is_refused() {
    let mut keys = SynapseKeyManager::new().await.unwrap();
    let result = keys.generate_keypair("rsa", KeyAlgorithm::RSA).await;
    assert!(
        result.is_err(),
        "RSA is not implemented, so key generation must be refused, not answered with bytes"
    );
}

#[tokio::test]
async fn rsa_signing_is_refused() {
    // KeyPair's fields are public, so a caller can hand in an RSA pair built elsewhere.
    let keys = KeyManager::new().await.unwrap();
    let pair = KeyPair {
        public_key: vec![1; 256],
        private_key: vec![2; 256],
        algorithm: KeyAlgorithm::RSA,
    };
    assert!(keys.sign(&pair, b"data").await.is_err());
}

#[tokio::test]
async fn verify_refuses_a_key_format_it_cannot_parse() {
    let keys = KeyManager::new().await.unwrap();
    let data = b"data";
    // 256 bytes matches no supported format (Ed25519 = 32, P-256 = 33 or 65).
    let result = keys.verify(&[3u8; 256], data, &sha256(data)).await;
    assert!(
        result.is_err(),
        "an unrecognised key format must be an error, got {result:?}"
    );
}

#[tokio::test]
async fn an_ed25519_signature_does_not_verify_under_another_key() {
    let mut keys = SynapseKeyManager::new().await.unwrap();
    keys.generate_keypair("a", KeyAlgorithm::Ed25519)
        .await
        .unwrap();
    let b = keys
        .generate_keypair("b", KeyAlgorithm::Ed25519)
        .await
        .unwrap();
    let sig = keys.sign("a", b"data").await.unwrap();
    assert!(!keys.verify(&b.public_key, b"data", &sig).await.unwrap());
}
