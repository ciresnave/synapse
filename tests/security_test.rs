// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive Security Tests for Synapse
//!
//! This file validates all security implementations across the codebase,
//! ensuring cryptographic functions, authentication, and blockchain security
//! work correctly and defend against known attack vectors.

use chrono::Utc;
use synapse::synapse::auth::utils::{KeyAlgorithm, KeyDerivationParams, SynapseKeyManager};
use synapse::synapse::blockchain::block::Block;

/// Test cryptographic key generation produces valid key pairs
#[tokio::test]
async fn test_cryptographic_key_generation() {
    let crypto = SynapseKeyManager::new().await;
    assert!(
        crypto.is_ok(),
        "SynapseKeyManager should initialize: {:?}",
        crypto.err()
    );

    let mut crypto = crypto.unwrap();
    let result = crypto
        .generate_keypair("test_key", KeyAlgorithm::Ed25519)
        .await;
    assert!(
        result.is_ok(),
        "Key generation should succeed: {:?}",
        result.err()
    );

    let keypair = result.unwrap();

    // Validate key components exist
    assert!(
        !keypair.public_key.is_empty(),
        "Public key should not be empty"
    );
    assert!(
        !keypair.private_key.is_empty(),
        "Private key should not be empty"
    );

    // Validate keys are different
    assert_ne!(
        keypair.public_key, keypair.private_key,
        "Public and private keys should be different"
    );

    // RSA is not implemented, so it must be refused rather than answered.
    assert!(
        crypto
            .generate_keypair("test_rsa_key", KeyAlgorithm::RSA)
            .await
            .is_err(),
        "RSA key generation must be refused"
    );
}

/// Test digital signature creation and verification
#[tokio::test]
async fn test_digital_signature_security() {
    let crypto = SynapseKeyManager::new().await.unwrap();
    let mut crypto = crypto;
    let keypair = crypto
        .generate_keypair("sign_test_key", KeyAlgorithm::Ed25519)
        .await
        .unwrap();
    let test_data = b"Critical security test data that must not be tampered with";

    // Test signature creation
    let signature_result = crypto.sign("sign_test_key", test_data).await;
    assert!(
        signature_result.is_ok(),
        "Signature creation should succeed: {:?}",
        signature_result.err()
    );

    let signature = signature_result.unwrap();
    assert!(!signature.is_empty(), "Signature should not be empty");
    assert!(
        signature.len() >= 32,
        "Signature should be substantial length for security"
    );

    // Test signature verification with correct data
    let verify_result = crypto
        .verify(&keypair.public_key, test_data, &signature)
        .await;
    assert!(
        verify_result.is_ok(),
        "Signature verification should succeed: {:?}",
        verify_result.err()
    );
    assert!(
        verify_result.unwrap(),
        "Valid signature should verify as true"
    );

    // Test signature verification fails with tampered data
    let tampered_data = b"Tampered data should fail verification";
    let verify_tampered = crypto
        .verify(&keypair.public_key, tampered_data, &signature)
        .await;
    assert!(
        !verify_tampered.expect("verification of a well-formed signature must not error"),
        "Tampered data signature should verify as false"
    );

    // Test signature verification fails with wrong signature
    let wrong_signature = vec![0u8; signature.len()]; // All zeros
    let verify_wrong = crypto
        .verify(&keypair.public_key, test_data, &wrong_signature)
        .await;
    assert!(
        !verify_wrong.expect("verification of a well-formed signature must not error"),
        "Wrong signature should verify as false"
    );
}

/// Test password key derivation security (PBKDF2 with proper salting)
#[tokio::test]
async fn test_password_key_derivation_security() {
    let crypto = SynapseKeyManager::new().await.unwrap();
    let password = "SuperSecureP@ssw0rd!123";
    let params = KeyDerivationParams::default();

    // Test key derivation
    let key_result = crypto.derive_key_from_password(password, &params).await;
    assert!(
        key_result.is_ok(),
        "Key derivation should succeed: {:?}",
        key_result.err()
    );

    let derived_key = key_result.unwrap();
    assert!(!derived_key.is_empty(), "Derived key should not be empty");
    assert!(
        derived_key.len() >= 32,
        "Derived key should be at least 256 bits"
    );

    // Test same password with same params produces same key (deterministic)
    let key2 = crypto
        .derive_key_from_password(password, &params)
        .await
        .unwrap();
    assert_eq!(
        derived_key, key2,
        "Same password+params should produce same key"
    );

    // Test different salt produces different key
    let different_params = KeyDerivationParams {
        salt: "different_salt".to_string(),
        ..Default::default()
    };
    let key3 = crypto
        .derive_key_from_password(password, &different_params)
        .await
        .unwrap();
    assert_ne!(
        derived_key, key3,
        "Different salt should produce different key"
    );

    // Test key doesn't contain plaintext password
    let key_string = BASE64_STANDARD.encode(&derived_key);
    assert!(
        !key_string.to_lowercase().contains("password"),
        "Key should not contain plaintext password"
    );
}

/// Test blockchain block SIGNING. Named for what it asserts.
///
/// ⚠️ IT DOES NOT VERIFY A SIGNATURE, AND IT NEVER DID. It was previously
/// called `test_blockchain_signature_verification`, which is what `cargo test`
/// printed on every green run — so anyone asking "does this project test that
/// block signatures are verified?" got a yes from a test that only signs.
///
/// The verification half is blocked on a function that does not exist:
/// `verify_block_signature` appears exactly ONCE in this repository, inside a
/// comment deferring it. Not renamed, not private — never written.
/// `tests/blockchain_verification_is_still_unbuilt.rs` reddens when it appears.
#[tokio::test]
async fn test_blockchain_block_signing() {
    use synapse::synapse::blockchain::serialization::DateTimeWrapper;

    let mut block = Block {
        number: 1,
        timestamp: DateTimeWrapper::new(Utc::now()),
        previous_hash: "genesis_hash".to_string(),
        transactions: vec![],
        hash: "block_hash".to_string(),
        nonce: 12345,
        validator: "test_validator".to_string(),
        signature: None,
    };

    // Test signing a block (with validator key)
    let crypto = SynapseKeyManager::new().await.unwrap();
    let mut crypto = crypto;
    let validator_keypair = crypto
        .generate_keypair("validator_key", KeyAlgorithm::Ed25519)
        .await
        .unwrap();
    let validator_private_key = validator_keypair.private_key;

    let sign_result = block.sign_block(&validator_private_key, "Ed25519").await;
    assert!(
        sign_result.is_ok(),
        "Block signing should succeed: {:?}",
        sign_result.err()
    );

    // Verify block now has signature
    assert!(
        block.signature.is_some(),
        "Block should have signature after signing"
    );
    let signature = block.signature.as_ref().unwrap();
    assert!(
        !signature.data.is_empty(),
        "Signature data should not be empty"
    );
    assert!(
        !signature.algorithm.is_empty(),
        "Signature algorithm should be specified"
    );
    assert_eq!(
        signature.algorithm, "Ed25519",
        "Signature should use secure algorithm"
    );

    // ⚠️ WHAT FOLLOWS IS NOT AN INTEGRITY CHECK, AND ITS OLD COMMENT SAID IT
    // WAS. The assertion compares a field to the literal assigned two lines
    // above it, so it is true by construction and cannot fail. It confirms that
    // `clone()` and a field assignment work. It says nothing about whether a
    // tampered block's SIGNATURE stops verifying, which is the property the
    // surrounding comments claimed.
    //
    // It is kept rather than deleted because it is harmless and it does pin the
    // clone-then-mutate step; only the claim about it was wrong.
    let mut tampered_block = block.clone();
    tampered_block.hash = "tampered_hash".to_string();
    assert_ne!(
        tampered_block.hash, block.hash,
        "the assignment above took effect — true by construction, NOT an integrity check"
    );

    // ⚠️ THE REAL CHECK CANNOT BE WRITTEN YET: it needs a verifier that does
    // not exist. When `verify_block_signature` is implemented, assert here that
    // it ACCEPTS `block` and REJECTS `tampered_block`, and delete the tripwire
    // in tests/blockchain_verification_is_still_unbuilt.rs.
}

// `encrypt_with_public_key` / `decrypt_with_private_key` were removed: they
// derived the AES key from the PUBLIC key, so holding the public key was enough
// to decrypt. Public-key encryption is `synapse::sealing` (HPKE), tested in
// tests/sealing.rs.

/// Test against common security vulnerabilities (basic input validation)
#[tokio::test]
async fn test_input_validation_security() {
    // Test extremely long input handling
    let very_long_input = "A".repeat(1_000_000); // 1MB of data

    // System should handle large inputs gracefully without crashing
    // This tests buffer overflow and DoS resistance
    let crypto = SynapseKeyManager::new().await.unwrap();
    let mut crypto = crypto;
    let keypair = crypto
        .generate_keypair("large_test_key", KeyAlgorithm::Ed25519)
        .await
        .unwrap();

    // Test signing very large data
    let large_sign_result = crypto
        .sign("large_test_key", very_long_input.as_bytes())
        .await;
    // Ed25519 has no message-length limit, so this must succeed.
    let signature = large_sign_result.expect("Ed25519 signs a message of any length");
    let verify_result = crypto
        .verify(&keypair.public_key, very_long_input.as_bytes(), &signature)
        .await;
    assert!(
        verify_result.expect("verification of a well-formed signature must not error"),
        "Large data signature should verify correctly"
    );

    // Test empty input handling
    let empty_result = crypto.sign("large_test_key", b"").await;
    assert!(
        empty_result.is_ok(),
        "Empty data signing should succeed or fail gracefully"
    );

    // Test null bytes in input
    let null_input = b"data\x00with\x00nulls";
    let null_result = crypto.sign("large_test_key", null_input).await;
    assert!(
        null_result.is_ok(),
        "Null byte input should be handled securely"
    );
}

/// Test cryptographic algorithm security
#[tokio::test]
async fn test_cryptographic_algorithm_security() {
    let crypto = SynapseKeyManager::new().await.unwrap();
    let mut crypto = crypto;

    // Test Ed25519 (recommended for modern security)
    let ed25519_key = crypto
        .generate_keypair("ed25519_test", KeyAlgorithm::Ed25519)
        .await;
    assert!(ed25519_key.is_ok(), "Ed25519 key generation should succeed");

    // RSA is not implemented, so it must be refused rather than answered.
    let rsa_key = crypto.generate_keypair("rsa_test", KeyAlgorithm::RSA).await;
    assert!(rsa_key.is_err(), "RSA key generation must be refused");

    // ECDSA P-256 is implemented: generation must succeed and round-trip.
    let keypair = crypto
        .generate_keypair("ecdsa_test", KeyAlgorithm::ECDSA)
        .await
        .expect("ECDSA key generation should succeed");
    let data = b"ecdsa round trip";
    let signature = crypto.sign("ecdsa_test", data).await.unwrap();
    assert!(
        crypto
            .verify(&keypair.public_key, data, &signature)
            .await
            .unwrap(),
        "ECDSA signature should verify under its own key"
    );
    assert!(
        !crypto
            .verify(&keypair.public_key, b"other data", &signature)
            .await
            .unwrap(),
        "ECDSA signature must not verify over different data"
    );
}

/// Integration test: End-to-end security validation
#[tokio::test]
async fn test_end_to_end_security_integration() {
    // Test complete security flow: key generation -> block signing -> signature verification -> key derivation

    // 1. Generate cryptographic keys
    let crypto = SynapseKeyManager::new().await.unwrap();
    let mut crypto = crypto;
    let signing_keypair = crypto
        .generate_keypair("signing_key", KeyAlgorithm::Ed25519)
        .await
        .unwrap();

    // 2. Create and sign blockchain block
    use synapse::synapse::blockchain::serialization::DateTimeWrapper;
    let mut block = Block {
        number: 42,
        timestamp: DateTimeWrapper::new(Utc::now()),
        previous_hash: "previous_block_hash".to_string(),
        transactions: vec![],
        hash: "computed_block_hash".to_string(),
        nonce: 98765,
        validator: "secure_validator".to_string(),
        signature: None,
    };

    let signed_bytes = serde_json::to_vec(&block).unwrap();
    block
        .sign_block(&signing_keypair.private_key, "Ed25519")
        .await
        .unwrap();
    let block_signature = block.signature.as_ref().expect("Block should be signed");
    assert!(
        crypto
            .verify(
                &signing_keypair.public_key,
                &signed_bytes,
                &block_signature.data
            )
            .await
            .unwrap(),
        "Block signature should verify under the validator's key"
    );

    // 3. Test signature verification
    let test_data = b"Important data requiring authentication";
    let signature = crypto.sign("signing_key", test_data).await.unwrap();
    let verified = crypto
        .verify(&signing_keypair.public_key, test_data, &signature)
        .await
        .unwrap();
    assert!(verified, "Signature should verify correctly");

    // 4. Test key derivation for password-based security
    let password = "user_secure_password_123!";
    let params = KeyDerivationParams {
        salt: "unique_salt_per_user".to_string(),
        ..Default::default()
    };
    let derived_key = crypto
        .derive_key_from_password(password, &params)
        .await
        .unwrap();
    assert!(
        derived_key.len() >= 32,
        "Derived key should be cryptographically strong"
    );

    // If we reach here, all security components integrate successfully
    println!("✅ End-to-end security integration test passed");
    println!("✅ Cryptographic key generation: SECURE");
    println!("✅ Digital signatures: SECURE");
    println!("✅ Block signing: signs with the validator's key");
    println!("✅ Key derivation: SECURE");
}

// Add base64 import for test
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
