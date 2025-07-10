//! Cryptographic operations for EMRP

use crate::error::{CryptoError, Result};
#[cfg(feature = "crypto")]
use rsa::{
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey},
    Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey,
    rand_core::RngCore,
};
#[cfg(feature = "crypto")]
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use sha2::{Digest, Sha256};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::HashMap;

/// Manages all cryptographic operations for EMRP
#[cfg(feature = "crypto")]
pub struct CryptoManager {
    /// Our private key
    private_key: Option<RsaPrivateKey>,
    /// Our public key
    public_key: Option<RsaPublicKey>,
    /// Known public keys of other entities
    known_keys: HashMap<String, RsaPublicKey>,
}

#[cfg(feature = "crypto")]
impl CryptoManager {
    /// Create a new crypto manager
    pub fn new() -> Self {
        Self {
            private_key: None,
            public_key: None,
            known_keys: HashMap::new(),
        }
    }

    /// Generate a new RSA keypair for this entity
    pub fn generate_keypair(&mut self) -> Result<(String, String)> {
        let mut rng = OsRng;
        
        // Generate 2048-bit RSA key
        let private_key = RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|e| CryptoError::KeyGeneration(e.to_string()))?;
        
        let public_key = RsaPublicKey::from(&private_key);

        // Encode keys to PEM format
        let private_pem = private_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|e| CryptoError::KeyGeneration(e.to_string()))?
            .to_string();

        let public_pem = public_key
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|e| CryptoError::KeyGeneration(e.to_string()))?;

        // Store keys
        self.private_key = Some(private_key);
        self.public_key = Some(public_key);

        tracing::info!("Generated new RSA keypair");

        Ok((private_pem, public_pem))
    }

    /// Load private key from PEM string
    pub fn load_private_key(&mut self, pem: &str) -> Result<()> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(pem)
            .map_err(|e| CryptoError::InvalidKey(e.to_string()))?;
        
        let public_key = RsaPublicKey::from(&private_key);
        
        self.private_key = Some(private_key);
        self.public_key = Some(public_key);
        
        tracing::info!("Loaded private key from PEM");
        Ok(())
    }

    /// Import a public key for another entity
    pub fn import_public_key(&mut self, global_id: &str, pem: &str) -> Result<()> {
        let public_key = RsaPublicKey::from_public_key_pem(pem)
            .map_err(|e| CryptoError::InvalidKey(e.to_string()))?;
        
        self.known_keys.insert(global_id.to_string(), public_key);
        
        tracing::info!("Imported public key for {}", global_id);
        Ok(())
    }

    /// Get our public key in PEM format
    pub fn get_public_key_pem(&self) -> Result<String> {
        let public_key = self.public_key.as_ref()
            .ok_or_else(|| CryptoError::KeyNotFound("No public key loaded".to_string()))?;
        
        public_key
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|e| CryptoError::InvalidKey(e.to_string()).into())
    }

    /// Encrypt a message for a specific recipient
    pub fn encrypt_message(&self, message: &str, recipient_global_id: &str) -> Result<Vec<u8>> {
        let recipient_key = self.known_keys.get(recipient_global_id)
            .ok_or_else(|| CryptoError::KeyNotFound(format!("No public key for {}", recipient_global_id)))?;

        let message_bytes = message.as_bytes();

        // For large messages, use hybrid encryption (AES + RSA)
        if message_bytes.len() > 200 {
            self.encrypt_large_message(message_bytes, recipient_key)
        } else {
            self.encrypt_small_message(message_bytes, recipient_key)
        }
    }

    /// Encrypt small message directly with RSA
    fn encrypt_small_message(&self, message: &[u8], public_key: &RsaPublicKey) -> Result<Vec<u8>> {
        let mut rng = OsRng;
        
        public_key
            .encrypt(&mut rng, Pkcs1v15Encrypt, message)
            .map_err(|e| CryptoError::Encryption(e.to_string()).into())
    }

    /// Encrypt large message with hybrid AES+RSA encryption
    fn encrypt_large_message(&self, message: &[u8], public_key: &RsaPublicKey) -> Result<Vec<u8>> {
        let mut rng = OsRng;

        // Generate random AES key
        let mut aes_key_bytes = [0u8; 32];
        rng.fill_bytes(&mut aes_key_bytes);
        let aes_key = Key::<Aes256Gcm>::from_slice(&aes_key_bytes);

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        rng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt message with AES
        let cipher = Aes256Gcm::new(aes_key);
        let encrypted_message = cipher
            .encrypt(nonce, message)
            .map_err(|e| CryptoError::Encryption(e.to_string()))?;

        // Encrypt AES key with RSA
        let encrypted_key = public_key
            .encrypt(&mut rng, Pkcs1v15Encrypt, &aes_key_bytes)
            .map_err(|e| CryptoError::Encryption(e.to_string()))?;

        // Combine: encrypted_key_length (4 bytes) + encrypted_key + nonce (12 bytes) + encrypted_message
        let mut result = Vec::new();
        result.extend_from_slice(&(encrypted_key.len() as u32).to_be_bytes());
        result.extend_from_slice(&encrypted_key);
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&encrypted_message);

        Ok(result)
    }

    /// Decrypt a message with our private key
    pub fn decrypt_message(&self, encrypted_data: &[u8]) -> Result<String> {
        let private_key = self.private_key.as_ref()
            .ok_or_else(|| CryptoError::KeyNotFound("No private key loaded".to_string()))?;

        // Check if this is hybrid encryption
        if encrypted_data.len() > 256 + 12 + 4 { // encrypted_key + nonce + length field
            self.decrypt_large_message(encrypted_data, private_key)
        } else {
            self.decrypt_small_message(encrypted_data, private_key)
        }
    }

    /// Decrypt small message directly with RSA
    fn decrypt_small_message(&self, encrypted_data: &[u8], private_key: &RsaPrivateKey) -> Result<String> {
        let decrypted = private_key
            .decrypt(Pkcs1v15Encrypt, encrypted_data)
            .map_err(|e| CryptoError::Decryption(e.to_string()))?;

        String::from_utf8(decrypted)
            .map_err(|e| CryptoError::Decryption(e.to_string()).into())
    }

    /// Decrypt large message with hybrid AES+RSA decryption
    fn decrypt_large_message(&self, encrypted_data: &[u8], private_key: &RsaPrivateKey) -> Result<String> {
        if encrypted_data.len() < 4 {
            return Err(CryptoError::Decryption("Invalid encrypted data format".to_string()).into());
        }

        // Extract encrypted key length
        let key_length = u32::from_be_bytes([
            encrypted_data[0],
            encrypted_data[1],
            encrypted_data[2],
            encrypted_data[3],
        ]) as usize;

        if encrypted_data.len() < 4 + key_length + 12 {
            return Err(CryptoError::Decryption("Invalid encrypted data format".to_string()).into());
        }

        // Extract components
        let encrypted_key = &encrypted_data[4..4 + key_length];
        let nonce_bytes = &encrypted_data[4 + key_length..4 + key_length + 12];
        let encrypted_message = &encrypted_data[4 + key_length + 12..];

        // Decrypt AES key with RSA
        let aes_key_bytes = private_key
            .decrypt(Pkcs1v15Encrypt, encrypted_key)
            .map_err(|e| CryptoError::Decryption(e.to_string()))?;

        if aes_key_bytes.len() != 32 {
            return Err(CryptoError::Decryption("Invalid AES key length".to_string()).into());
        }

        let aes_key = Key::<Aes256Gcm>::from_slice(&aes_key_bytes);
        let nonce = Nonce::from_slice(nonce_bytes);

        // Decrypt message with AES
        let cipher = Aes256Gcm::new(aes_key);
        let decrypted_message = cipher
            .decrypt(nonce, encrypted_message)
            .map_err(|e| CryptoError::Decryption(e.to_string()))?;

        String::from_utf8(decrypted_message)
            .map_err(|e| CryptoError::Decryption(e.to_string()).into())
    }

    /// Sign a message with our private key
    pub fn sign_message(&self, message: &str) -> Result<Vec<u8>> {
        let private_key = self.private_key.as_ref()
            .ok_or_else(|| CryptoError::KeyNotFound("No private key loaded".to_string()))?;

        use rsa::Pkcs1v15Sign;
        
        let hash = Sha256::digest(message.as_bytes());
        
        let signature = private_key.sign(Pkcs1v15Sign::new_unprefixed(), &hash)
            .map_err(|e| CryptoError::Signing(e.to_string()))?;
        
        Ok(signature)
    }

    /// Verify a message signature  
    pub fn verify_signature(&self, message: &str, signature: &[u8], sender_global_id: &str) -> Result<bool> {
        let sender_key = self.known_keys.get(sender_global_id)
            .ok_or_else(|| CryptoError::KeyNotFound(format!("No public key for {}", sender_global_id)))?;

        use rsa::Pkcs1v15Sign;
        
        let hash = Sha256::digest(message.as_bytes());
        
        match sender_key.verify(Pkcs1v15Sign::new_unprefixed(), &hash, signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Generate a secure hash of data
    pub fn hash_data(&self, data: &[u8]) -> String {
        let result = Sha256::digest(data);
        STANDARD.encode(result)
    }

    /// Generate a random message ID
    pub fn generate_message_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Check if we have a key for an entity
    pub fn has_key_for(&self, global_id: &str) -> bool {
        self.known_keys.contains_key(global_id)
    }

    /// Get list of entities we have keys for
    pub fn known_entities(&self) -> Vec<String> {
        self.known_keys.keys().cloned().collect()
    }
}

#[cfg(feature = "crypto")]
impl Default for CryptoManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let mut crypto = CryptoManager::new();
        let result = crypto.generate_keypair();
        assert!(result.is_ok());
        
        let (private_pem, public_pem) = result.unwrap();
        assert!(private_pem.starts_with("-----BEGIN PRIVATE KEY-----"));
        assert!(public_pem.starts_with("-----BEGIN PUBLIC KEY-----"));
    }

    #[test]
    fn test_small_message_encryption() {
        let mut crypto1 = CryptoManager::new();
        let mut crypto2 = CryptoManager::new();
        
        let (_, pub1) = crypto1.generate_keypair().unwrap();
        let (_, pub2) = crypto2.generate_keypair().unwrap();
        
        crypto1.import_public_key("entity2", &pub2).unwrap();
        crypto2.import_public_key("entity1", &pub1).unwrap();
        
        let message = "Hello, World!";
        let encrypted = crypto1.encrypt_message(message, "entity2").unwrap();
        let decrypted = crypto2.decrypt_message(&encrypted).unwrap();
        
        assert_eq!(message, decrypted);
    }

    #[test]
    fn test_large_message_encryption() {
        let mut crypto1 = CryptoManager::new();
        let mut crypto2 = CryptoManager::new();
        
        let (_, pub1) = crypto1.generate_keypair().unwrap();
        let (_, pub2) = crypto2.generate_keypair().unwrap();
        
        crypto1.import_public_key("entity2", &pub2).unwrap();
        crypto2.import_public_key("entity1", &pub1).unwrap();
        
        let message = "A".repeat(1000); // Large message
        let encrypted = crypto1.encrypt_message(&message, "entity2").unwrap();
        let decrypted = crypto2.decrypt_message(&encrypted).unwrap();
        
        assert_eq!(message, decrypted);
    }

    #[test]
    fn test_message_signing() {
        let mut crypto1 = CryptoManager::new();
        let mut crypto2 = CryptoManager::new();
        
        let (_, pub1) = crypto1.generate_keypair().unwrap();
        let (_, _) = crypto2.generate_keypair().unwrap();
        
        crypto2.import_public_key("entity1", &pub1).unwrap();
        
        let message = "Important message";
        let signature = crypto1.sign_message(message).unwrap();
        let verified = crypto2.verify_signature(message, &signature, "entity1").unwrap();
        
        assert!(verified);
    }
}
