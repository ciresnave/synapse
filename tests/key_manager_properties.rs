// SPDX-License-Identifier: MIT OR Apache-2.0
//! Properties that `synapse::synapse::auth::utils` and block signing must have.
//!
//! Each test states a property a real implementation has and a placeholder
//! does not. An algorithm this crate cannot perform must be refused with an
//! `Err`, never answered with an `Ok` that has no key behind it.

use std::collections::HashMap;

use chrono::Utc;
use ring::digest;
use synapse::synapse::auth::utils::{KeyAlgorithm, KeyManager, KeyPair, SynapseKeyManager};
use synapse::synapse::blockchain::serialization::DateTimeWrapper;
use synapse::synapse::blockchain::{Block, BlockchainConfig, VerificationEngine};
use synapse::synapse::models::trust::TrustBalance;

fn sha256(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA256, data).as_ref().to_vec()
}

fn unsigned_block(validator: &str) -> Block {
    let mut block = Block {
        number: 1,
        timestamp: DateTimeWrapper::new(Utc::now()),
        previous_hash: "genesis".to_string(),
        transactions: vec![],
        hash: String::new(),
        nonce: 7,
        validator: validator.to_string(),
        signature: None,
    };
    block.hash = block.calculate_hash();
    block
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

#[tokio::test]
async fn sign_block_signs_with_the_key_it_is_given() {
    let mut keys = SynapseKeyManager::new().await.unwrap();
    let validator = keys
        .generate_keypair("validator", KeyAlgorithm::Ed25519)
        .await
        .unwrap();

    let mut block = unsigned_block("validator");
    let signed_bytes = serde_json::to_vec(&block).unwrap();
    block
        .sign_block(&validator.private_key, "Ed25519")
        .await
        .unwrap();

    let signature = block.signature.as_ref().expect("block was signed");
    assert_eq!(signature.algorithm, "Ed25519");
    assert!(
        keys.verify(&validator.public_key, &signed_bytes, &signature.data)
            .await
            .unwrap(),
        "the block signature must verify under the validator's own public key"
    );
}

#[tokio::test]
async fn sign_block_refuses_an_algorithm_it_does_not_implement() {
    let mut keys = SynapseKeyManager::new().await.unwrap();
    let validator = keys
        .generate_keypair("validator", KeyAlgorithm::Ed25519)
        .await
        .unwrap();
    let mut block = unsigned_block("validator");
    assert!(
        block
            .sign_block(&validator.private_key, "not-an-algorithm")
            .await
            .is_err()
    );
    assert!(block.signature.is_none());
}

#[tokio::test]
async fn a_block_signature_made_without_any_key_is_rejected() {
    // A signature that is only a hash of the block involves no secret, so no
    // verifier may accept it, whatever the validator is called.
    for validator in [
        "v".repeat(17),
        "v".repeat(18),
        "v".repeat(50),
        "v".repeat(200),
    ] {
        let mut block = unsigned_block(&validator);
        let keyless = sha256(&serde_json::to_vec(&block).unwrap());
        block.signature = Some(synapse::synapse::blockchain::block::BlockSignature {
            data: keyless,
            algorithm: "Ed25519".to_string(),
        });

        let engine = VerificationEngine::new(BlockchainConfig::default());
        let balances: HashMap<String, TrustBalance> = HashMap::new();
        let result = engine.verify_block(&block, None, &balances).await.unwrap();
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("ignature") || e.contains("public key")),
            "validator of length {}: the signature stage accepted a keyless signature; errors: {:?}",
            validator.len(),
            result.errors
        );
    }
}
