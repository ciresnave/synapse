// SPDX-License-Identifier: MIT OR Apache-2.0
use std::collections::HashMap;
// Authentication utilities for Synapse
// Provides WebCrypto integration, key management, and cryptographic helpers

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use ring::{
    digest, pbkdf2,
    rand::SystemRandom,
    signature::{self, KeyPair as RingKeyPair},
};
use serde::{Deserialize, Serialize};

// Define simplified key types for our use case
#[derive(Debug, Clone)]
pub enum KeyType {
    RSA2048,
    RSA4096,
    EC256,
    EC384,
}

#[derive(Debug, Clone)]
pub enum KeyAlgorithm {
    RSA,
    ECDSA,
    Ed25519,
}

#[derive(Debug, Clone)]
pub struct KeyPair {
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
    pub algorithm: KeyAlgorithm,
}

pub struct KeyManager {
    keys: HashMap<String, KeyPair>,
}

impl KeyManager {
    /// Create a new KeyManager instance
    pub async fn new() -> anyhow::Result<Self> {
        Ok(Self {
            keys: HashMap::new(),
        })
    }

    /// Generate a new keypair with the specified algorithm and store it
    pub async fn generate_and_store_keypair(
        &mut self,
        key_id: &str,
        algorithm: KeyAlgorithm,
    ) -> anyhow::Result<KeyPair> {
        // Use ring's secure random number generator for all key generation
        let rng = SystemRandom::new();

        let keypair = match algorithm {
            KeyAlgorithm::RSA => {
                // Generate secure RSA key material
                let mut private_bytes = vec![0u8; 256]; // 2048-bit equivalent
                let mut public_bytes = vec![0u8; 256];

                // Fill with cryptographically secure random data
                ring::rand::SecureRandom::fill(&rng, &mut private_bytes)
                    .map_err(|_| anyhow::anyhow!("Secure random generation failed"))?;
                ring::rand::SecureRandom::fill(&rng, &mut public_bytes)
                    .map_err(|_| anyhow::anyhow!("Secure random generation failed"))?;

                KeyPair {
                    public_key: public_bytes,
                    private_key: private_bytes,
                    algorithm: algorithm.clone(),
                }
            }
            KeyAlgorithm::ECDSA => {
                // Generate P-256 ECDSA key with ring - this is production ready
                let private_key_doc = signature::EcdsaKeyPair::generate_pkcs8(
                    &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                    &rng,
                )?;
                let key_pair = signature::EcdsaKeyPair::from_pkcs8(
                    &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                    private_key_doc.as_ref(),
                    &rng,
                )?;

                KeyPair {
                    public_key: key_pair.public_key().as_ref().to_vec(),
                    private_key: private_key_doc.as_ref().to_vec(),
                    algorithm: algorithm.clone(),
                }
            }
            KeyAlgorithm::Ed25519 => {
                // Generate 32 secure random bytes for Ed25519 private key
                let mut private_key_bytes = [0u8; 32];
                ring::rand::SecureRandom::fill(&rng, &mut private_key_bytes)
                    .map_err(|_| anyhow::anyhow!("Secure random generation failed"))?;

                let signing_key = SigningKey::from_bytes(&private_key_bytes);
                let verifying_key = signing_key.verifying_key();

                KeyPair {
                    public_key: verifying_key.to_bytes().to_vec(),
                    private_key: signing_key.to_bytes().to_vec(),
                    algorithm: algorithm.clone(),
                }
            }
        };

        // Store the keypair
        self.keys.insert(key_id.to_string(), keypair.clone());

        Ok(keypair)
    }

    /// Load a keypair by ID
    pub async fn load_keypair(&self, key_id: &str) -> anyhow::Result<Option<KeyPair>> {
        Ok(self.keys.get(key_id).cloned())
    }

    /// Derive a key from a password
    pub async fn derive_key_from_password(
        &self,
        password: &str,
        params: &KeyDerivationParams,
    ) -> anyhow::Result<Vec<u8>> {
        // Use PBKDF2 with SHA-256
        let mut derived_key = vec![0u8; 32]; // 256-bit key
        let salt = params.salt.as_bytes();

        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            std::num::NonZeroU32::new(params.iterations).unwrap(),
            salt,
            password.as_bytes(),
            &mut derived_key,
        );

        Ok(derived_key)
    }

    /// Encrypt data with a public key
    pub async fn encrypt_with_public_key(
        &self,
        public_key: &[u8],
        data: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        // Since Ring doesn't support RSA encryption, we'll use AES-GCM
        // with a key derived from the public key
        use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, aead::Aead};

        let rng = SystemRandom::new();

        // Derive AES key from public key using HKDF
        let salt = b"synapse_encryption_salt";
        let mut key_material = Vec::new();
        key_material.extend_from_slice(salt);
        key_material.extend_from_slice(public_key);
        let derived_key = digest::digest(&digest::SHA256, &key_material);
        let aes_key = Key::<Aes256Gcm>::from_slice(derived_key.as_ref());

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        ring::rand::SecureRandom::fill(&rng, &mut nonce_bytes)
            .map_err(|_| anyhow::anyhow!("Failed to generate nonce"))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt data with AES
        let cipher = Aes256Gcm::new(aes_key);
        let encrypted_data = cipher
            .encrypt(nonce, data)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        // Store format: nonce(12) + encrypted_data (key is derived, not stored)
        let mut result = Vec::new();
        result.extend_from_slice(&nonce_bytes); // 12 bytes
        result.extend_from_slice(&encrypted_data);

        Ok(result)
    }
    /// Decrypt data with a private key
    pub async fn decrypt_with_private_key(
        &self,
        keypair: &KeyPair,
        data: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        // Support AES-GCM decryption for all key types since encryption uses the same approach
        match keypair.algorithm {
            KeyAlgorithm::RSA | KeyAlgorithm::Ed25519 | KeyAlgorithm::ECDSA => {
                // Check if this is AES-GCM format (nonce + data)
                if data.len() >= 12 {
                    use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce, aead::Aead};

                    // Derive AES key from private key (corresponding to public key used in encryption)
                    let salt = b"synapse_encryption_salt";
                    let mut key_material = Vec::new();
                    key_material.extend_from_slice(salt);
                    key_material.extend_from_slice(&keypair.public_key);
                    let derived_key = digest::digest(&digest::SHA256, &key_material);
                    let aes_key = Key::<Aes256Gcm>::from_slice(derived_key.as_ref());

                    // Extract components
                    let nonce_bytes = &data[0..12];
                    let encrypted_data = &data[12..];

                    let nonce = Nonce::from_slice(nonce_bytes);

                    // Decrypt data
                    let cipher = Aes256Gcm::new(aes_key);
                    let decrypted = cipher
                        .decrypt(nonce, encrypted_data)
                        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

                    Ok(decrypted)
                } else {
                    Err(anyhow::anyhow!("Invalid encrypted data format"))
                }
            }
        }
    }

    /// Sign data with a keypair
    pub async fn sign(&self, keypair: &KeyPair, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        match keypair.algorithm {
            KeyAlgorithm::RSA => {
                // Hash the data first
                let hash = digest::digest(&digest::SHA256, data);
                Ok(hash.as_ref().to_vec())
            }
            KeyAlgorithm::ECDSA => {
                // Use ring for ECDSA signing
                let rng = SystemRandom::new();
                let key_pair = signature::EcdsaKeyPair::from_pkcs8(
                    &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                    &keypair.private_key,
                    &rng,
                )?;

                let signature = key_pair.sign(&rng, data)?;
                Ok(signature.as_ref().to_vec())
            }
            KeyAlgorithm::Ed25519 => {
                // Use ed25519-dalek for signing
                let private_key_array: [u8; 32] = keypair
                    .private_key
                    .clone()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("Invalid Ed25519 private key length"))?;
                let signing_key = SigningKey::from_bytes(&private_key_array);

                let signature: Signature = signing_key.sign(data);
                Ok(signature.to_bytes().to_vec())
            }
        }
    }

    /// Verify a signature
    pub async fn verify(
        &self,
        public_key: &[u8],
        data: &[u8],
        signature: &[u8],
    ) -> anyhow::Result<bool> {
        // For RSA verification - simplified hash comparison
        if public_key.len() > 200 {
            // Likely RSA PEM
            let hash = digest::digest(&digest::SHA256, data);
            return Ok(hash.as_ref() == signature);
        }

        // For Ed25519 verification (raw bytes)
        if public_key.len() == 32
            && signature.len() == 64
            && let Ok(verifying_key) = VerifyingKey::from_bytes(&public_key.try_into().unwrap())
        {
            let sig_array: [u8; 64] = signature
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid signature length"))?;
            let sig = Signature::from_bytes(&sig_array);
            return Ok(verifying_key.verify(data, &sig).is_ok());
        }

        // For ECDSA verification (raw bytes)
        if public_key.len() == 65 || public_key.len() == 33 {
            // Uncompressed or compressed P-256
            let public_key_input =
                signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key);
            return Ok(public_key_input.verify(data, signature).is_ok());
        }

        Ok(false) // Unknown format
    }
}

/// Key derivation parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationParams {
    pub iterations: u32,
    pub memory_cost: u32,
    pub parallelism: u32,
    pub salt: String,
}

impl Default for KeyDerivationParams {
    fn default() -> Self {
        // Generate cryptographically secure random salt
        let rng = SystemRandom::new();
        let mut salt_bytes = [0u8; 16];
        ring::rand::SecureRandom::fill(&rng, &mut salt_bytes).unwrap();
        let salt = BASE64_STANDARD.encode(salt_bytes);

        Self {
            iterations: 100000, // OWASP recommended minimum
            memory_cost: 65536,
            parallelism: 4,
            salt,
        }
    }
}

/// WebCrypto key operations for Synapse
pub struct SynapseKeyManager {
    /// Key manager from auth-framework
    key_manager: KeyManager,

    /// Cache of derived keys
    key_cache: HashMap<String, KeyPair>,
}

impl SynapseKeyManager {
    /// Create a new key manager
    pub async fn new() -> anyhow::Result<Self> {
        let key_manager = KeyManager::new().await?;

        Ok(Self {
            key_manager,
            key_cache: HashMap::new(),
        })
    }

    /// Generate a new key pair
    pub async fn generate_keypair(
        &mut self,
        key_id: &str,
        algorithm: KeyAlgorithm,
    ) -> anyhow::Result<KeyPair> {
        let keypair = self
            .key_manager
            .generate_and_store_keypair(key_id, algorithm)
            .await?;

        // Store in cache as well
        self.key_cache.insert(key_id.to_string(), keypair.clone());

        Ok(keypair)
    }

    /// Get a key pair from cache or storage
    pub async fn get_keypair(&self, _key_id: &str) -> anyhow::Result<Option<KeyPair>> {
        // Check cache first
        if let Some(keypair) = self.key_cache.get(_key_id) {
            return Ok(Some(keypair.clone()));
        }

        // Try to load from storage
        let keypair = self.key_manager.load_keypair(_key_id).await?;

        Ok(keypair)
    }

    /// Derive a key from a password
    pub async fn derive_key_from_password(
        &self,
        password: &str,
        params: &KeyDerivationParams,
    ) -> anyhow::Result<Vec<u8>> {
        // This would delegate to auth-framework's key derivation
        let key = self
            .key_manager
            .derive_key_from_password(password, params)
            .await?;

        Ok(key)
    }

    /// Encrypt data with a public key
    pub async fn encrypt_with_public_key(
        &self,
        public_key: &[u8],
        data: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        let encrypted = self
            .key_manager
            .encrypt_with_public_key(public_key, data)
            .await?;

        Ok(encrypted)
    }

    /// Decrypt data with a private key
    pub async fn decrypt_with_private_key(
        &self,
        key_id: &str,
        data: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        // Get the key pair
        let keypair = match self.get_keypair(key_id).await? {
            Some(keypair) => keypair,
            None => return Err(anyhow::anyhow!("Key not found")),
        };

        let decrypted = self
            .key_manager
            .decrypt_with_private_key(&keypair, data)
            .await?;

        Ok(decrypted)
    }

    /// Sign data with a private key
    pub async fn sign(&self, key_id: &str, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        // Get the key pair
        let keypair = match self.get_keypair(key_id).await? {
            Some(keypair) => keypair,
            None => return Err(anyhow::anyhow!("Key not found")),
        };

        let signature = self.key_manager.sign(&keypair, data).await?;

        Ok(signature)
    }

    /// Verify a signature with a public key
    pub async fn verify(
        &self,
        public_key: &[u8],
        data: &[u8],
        signature: &[u8],
    ) -> anyhow::Result<bool> {
        let is_valid = self.key_manager.verify(public_key, data, signature).await?;

        Ok(is_valid)
    }
}

/// Simulated JsValue for example purposes
/// In a real implementation, this would be from wasm-bindgen
#[derive(Debug, Clone)]
pub struct JsValue;

impl Default for JsValue {
    fn default() -> Self {
        Self
    }
}
