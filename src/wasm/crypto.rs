//! WebAssembly-compatible cryptography using Web Crypto API
//! 
//! This module provides cryptographic operations that work in browser
//! environments using the Web Crypto API instead of native crypto libraries.

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, Crypto, SubtleCrypto, CryptoKey, CryptoKeyPair};
use js_sys::{Array, Object, Uint8Array, Promise};

use crate::error::Result;

/// WebAssembly-compatible crypto manager
pub struct WebCrypto {
    subtle_crypto: SubtleCrypto,
    our_key_pair: Option<CryptoKeyPair>,
    peer_public_keys: std::collections::HashMap<String, CryptoKey>,
}

/// Cryptographic algorithm configurations
pub struct WebCryptoConfig {
    pub rsa_key_size: u32,
    pub aes_key_size: u32,
    pub hash_algorithm: String,
}

impl Default for WebCryptoConfig {
    fn default() -> Self {
        Self {
            rsa_key_size: 2048,
            aes_key_size: 256,
            hash_algorithm: "SHA-256".to_string(),
        }
    }
}

impl WebCrypto {
    /// Create a new WebCrypto instance
    pub fn new() -> Self {
        let window = window().expect("No window object available");
        let crypto = window.crypto().expect("No crypto object available");
        let subtle_crypto = crypto.subtle();
        
        web_sys::console::log_1(&"Initialized WebCrypto manager".into());
        
        Self {
            subtle_crypto,
            our_key_pair: None,
            peer_public_keys: std::collections::HashMap::new(),
        }
    }
    
    /// Generate our RSA key pair for encryption/decryption
    pub async fn generate_key_pair(&mut self) -> Result<()> {
        web_sys::console::log_1(&"Generating RSA key pair...".into());
        
        // Configure RSA key generation
        let algorithm = Object::new();
        js_sys::Reflect::set(&algorithm, &"name".into(), &"RSA-OAEP".into())
            .map_err(|e| anyhow::anyhow!("Failed to set algorithm name: {:?}", e))?;
        js_sys::Reflect::set(&algorithm, &"modulusLength".into(), &2048u32.into())
            .map_err(|e| anyhow::anyhow!("Failed to set modulus length: {:?}", e))?;
        js_sys::Reflect::set(&algorithm, &"publicExponent".into(), &Uint8Array::from(&[0x01, 0x00, 0x01][..]))
            .map_err(|e| anyhow::anyhow!("Failed to set public exponent: {:?}", e))?;
        js_sys::Reflect::set(&algorithm, &"hash".into(), &"SHA-256".into())
            .map_err(|e| anyhow::anyhow!("Failed to set hash algorithm: {:?}", e))?;
        
        // Generate key pair
        let key_usages = Array::new();
        key_usages.push(&"encrypt".into());
        key_usages.push(&"decrypt".into());
        
        let key_pair_promise = self.subtle_crypto.generate_key_with_object(
            &algorithm,
            false, // extractable
            &key_usages,
        ).map_err(|e| anyhow::anyhow!("Failed to initiate key generation: {:?}", e))?;
        
        let key_pair_result = JsFuture::from(key_pair_promise).await
            .map_err(|e| anyhow::anyhow!("Key generation failed: {:?}", e))?;
        
        let key_pair = key_pair_result.dyn_into::<CryptoKeyPair>()
            .map_err(|e| anyhow::anyhow!("Failed to cast to CryptoKeyPair: {:?}", e))?;
        
        self.our_key_pair = Some(key_pair);
        
        web_sys::console::log_1(&"RSA key pair generated successfully".into());
        Ok(())
    }
    
    /// Export our public key for sharing with peers
    pub async fn export_public_key(&self) -> Result<Vec<u8>> {
        if let Some(ref key_pair) = self.our_key_pair {
            let public_key = key_pair.public_key();
            
            let export_promise = self.subtle_crypto.export_key("spki", &public_key)
                .map_err(|e| anyhow::anyhow!("Failed to initiate key export: {:?}", e))?;
            
            let exported_result = JsFuture::from(export_promise).await
                .map_err(|e| anyhow::anyhow!("Key export failed: {:?}", e))?;
            
            let array_buffer = exported_result.dyn_into::<js_sys::ArrayBuffer>()
                .map_err(|e| anyhow::anyhow!("Failed to cast exported key: {:?}", e))?;
            
            let uint8_array = Uint8Array::new(&array_buffer);
            let mut key_bytes = vec![0u8; uint8_array.length() as usize];
            uint8_array.copy_to(&mut key_bytes);
            
            web_sys::console::log_1(&format!("Exported public key ({} bytes)", key_bytes.len()).into());
            Ok(key_bytes)
        } else {
            Err(anyhow::anyhow!("No key pair available for export"))
        }
    }
    
    /// Import a peer's public key
    pub async fn import_peer_public_key(&mut self, peer_id: String, key_data: &[u8]) -> Result<()> {
        web_sys::console::log_1(&format!("Importing public key for peer: {}", peer_id).into());
        
        // Configure key import
        let algorithm = Object::new();
        js_sys::Reflect::set(&algorithm, &"name".into(), &"RSA-OAEP".into())
            .map_err(|e| anyhow::anyhow!("Failed to set algorithm name: {:?}", e))?;
        js_sys::Reflect::set(&algorithm, &"hash".into(), &"SHA-256".into())
            .map_err(|e| anyhow::anyhow!("Failed to set hash algorithm: {:?}", e))?;
        
        let key_usages = Array::new();
        key_usages.push(&"encrypt".into());
        
        // Convert key data to ArrayBuffer
        let uint8_array = Uint8Array::from(key_data);
        let array_buffer = uint8_array.buffer();
        
        let import_promise = self.subtle_crypto.import_key_with_object(
            "spki",
            &array_buffer,
            &algorithm,
            false, // extractable
            &key_usages,
        ).map_err(|e| anyhow::anyhow!("Failed to initiate key import: {:?}", e))?;
        
        let imported_result = JsFuture::from(import_promise).await
            .map_err(|e| anyhow::anyhow!("Key import failed: {:?}", e))?;
        
        let public_key = imported_result.dyn_into::<CryptoKey>()
            .map_err(|e| anyhow::anyhow!("Failed to cast imported key: {:?}", e))?;
        
        self.peer_public_keys.insert(peer_id.clone(), public_key);
        
        web_sys::console::log_1(&format!("Successfully imported public key for {}", peer_id).into());
        Ok(())
    }
    
    /// Encrypt a message for a specific peer
    pub async fn encrypt_for_peer(&self, peer_id: &str, message: &[u8]) -> Result<Vec<u8>> {
        if let Some(public_key) = self.peer_public_keys.get(peer_id) {
            web_sys::console::log_1(&format!("Encrypting message for peer: {}", peer_id).into());
            
            // Use RSA-OAEP for encryption
            let algorithm = Object::new();
            js_sys::Reflect::set(&algorithm, &"name".into(), &"RSA-OAEP".into())
                .map_err(|e| anyhow::anyhow!("Failed to set algorithm name: {:?}", e))?;
            
            // Convert message to ArrayBuffer
            let uint8_array = Uint8Array::from(message);
            let array_buffer = uint8_array.buffer();
            
            let encrypt_promise = self.subtle_crypto.encrypt_with_object_and_buffer_source(
                &algorithm,
                public_key,
                &array_buffer,
            ).map_err(|e| anyhow::anyhow!("Failed to initiate encryption: {:?}", e))?;
            
            let encrypted_result = JsFuture::from(encrypt_promise).await
                .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;
            
            let encrypted_buffer = encrypted_result.dyn_into::<js_sys::ArrayBuffer>()
                .map_err(|e| anyhow::anyhow!("Failed to cast encrypted data: {:?}", e))?;
            
            let encrypted_array = Uint8Array::new(&encrypted_buffer);
            let mut encrypted_bytes = vec![0u8; encrypted_array.length() as usize];
            encrypted_array.copy_to(&mut encrypted_bytes);
            
            web_sys::console::log_1(&format!("Encrypted {} bytes for {}", encrypted_bytes.len(), peer_id).into());
            Ok(encrypted_bytes)
        } else {
            Err(anyhow::anyhow!("No public key available for peer: {}", peer_id))
        }
    }
    
    /// Decrypt a message using our private key
    pub async fn decrypt_message(&self, encrypted_data: &[u8]) -> Result<Vec<u8>> {
        if let Some(ref key_pair) = self.our_key_pair {
            web_sys::console::log_1(&"Decrypting received message...".into());
            
            let private_key = key_pair.private_key();
            
            // Use RSA-OAEP for decryption
            let algorithm = Object::new();
            js_sys::Reflect::set(&algorithm, &"name".into(), &"RSA-OAEP".into())
                .map_err(|e| anyhow::anyhow!("Failed to set algorithm name: {:?}", e))?;
            
            // Convert encrypted data to ArrayBuffer
            let uint8_array = Uint8Array::from(encrypted_data);
            let array_buffer = uint8_array.buffer();
            
            let decrypt_promise = self.subtle_crypto.decrypt_with_object_and_buffer_source(
                &algorithm,
                &private_key,
                &array_buffer,
            ).map_err(|e| anyhow::anyhow!("Failed to initiate decryption: {:?}", e))?;
            
            let decrypted_result = JsFuture::from(decrypt_promise).await
                .map_err(|e| anyhow::anyhow!("Decryption failed: {:?}", e))?;
            
            let decrypted_buffer = decrypted_result.dyn_into::<js_sys::ArrayBuffer>()
                .map_err(|e| anyhow::anyhow!("Failed to cast decrypted data: {:?}", e))?;
            
            let decrypted_array = Uint8Array::new(&decrypted_buffer);
            let mut decrypted_bytes = vec![0u8; decrypted_array.length() as usize];
            decrypted_array.copy_to(&mut decrypted_bytes);
            
            web_sys::console::log_1(&format!("Decrypted {} bytes", decrypted_bytes.len()).into());
            Ok(decrypted_bytes)
        } else {
            Err(anyhow::anyhow!("No private key available for decryption"))
        }
    }
    
    /// Generate a hash of data
    pub async fn hash_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Convert data to ArrayBuffer
        let uint8_array = Uint8Array::from(data);
        let array_buffer = uint8_array.buffer();
        
        let hash_promise = self.subtle_crypto.digest_with_str_and_buffer_source(
            "SHA-256",
            &array_buffer,
        ).map_err(|e| anyhow::anyhow!("Failed to initiate hashing: {:?}", e))?;
        
        let hash_result = JsFuture::from(hash_promise).await
            .map_err(|e| anyhow::anyhow!("Hashing failed: {:?}", e))?;
        
        let hash_buffer = hash_result.dyn_into::<js_sys::ArrayBuffer>()
            .map_err(|e| anyhow::anyhow!("Failed to cast hash result: {:?}", e))?;
        
        let hash_array = Uint8Array::new(&hash_buffer);
        let mut hash_bytes = vec![0u8; hash_array.length() as usize];
        hash_array.copy_to(&mut hash_bytes);
        
        Ok(hash_bytes)
    }
    
    /// Generate random bytes using the crypto-secure random number generator
    pub fn generate_random_bytes(&self, length: usize) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; length];
        let uint8_array = Uint8Array::from(&mut bytes[..]);
        
        let window = window().ok_or_else(|| anyhow::anyhow!("No window object"))?;
        let crypto = window.crypto().map_err(|e| anyhow::anyhow!("No crypto object: {:?}", e))?;
        
        crypto.get_random_values_with_u8_array(&uint8_array)
            .map_err(|e| anyhow::anyhow!("Failed to generate random bytes: {:?}", e))?;
        
        uint8_array.copy_to(&mut bytes);
        Ok(bytes)
    }
    
    /// Generate a symmetric AES key for session encryption
    pub async fn generate_aes_key(&self) -> Result<CryptoKey> {
        web_sys::console::log_1(&"Generating AES key...".into());
        
        let algorithm = Object::new();
        js_sys::Reflect::set(&algorithm, &"name".into(), &"AES-GCM".into())
            .map_err(|e| anyhow::anyhow!("Failed to set algorithm name: {:?}", e))?;
        js_sys::Reflect::set(&algorithm, &"length".into(), &256u32.into())
            .map_err(|e| anyhow::anyhow!("Failed to set key length: {:?}", e))?;
        
        let key_usages = Array::new();
        key_usages.push(&"encrypt".into());
        key_usages.push(&"decrypt".into());
        
        let key_promise = self.subtle_crypto.generate_key_with_object(
            &algorithm,
            false, // extractable
            &key_usages,
        ).map_err(|e| anyhow::anyhow!("Failed to initiate AES key generation: {:?}", e))?;
        
        let key_result = JsFuture::from(key_promise).await
            .map_err(|e| anyhow::anyhow!("AES key generation failed: {:?}", e))?;
        
        let aes_key = key_result.dyn_into::<CryptoKey>()
            .map_err(|e| anyhow::anyhow!("Failed to cast to CryptoKey: {:?}", e))?;
        
        web_sys::console::log_1(&"AES key generated successfully".into());
        Ok(aes_key)
    }
    
    /// Check if our key pair is available
    pub fn has_key_pair(&self) -> bool {
        self.our_key_pair.is_some()
    }
    
    /// Get the number of peer public keys stored
    pub fn peer_key_count(&self) -> usize {
        self.peer_public_keys.len()
    }
    
    /// Remove a peer's public key
    pub fn remove_peer_key(&mut self, peer_id: &str) -> bool {
        self.peer_public_keys.remove(peer_id).is_some()
    }
}

/// Utility functions for WebAssembly crypto operations
pub mod utils {
    use super::*;
    
    /// Check if Web Crypto API is available
    pub fn is_web_crypto_available() -> bool {
        if let Some(window) = window() {
            if let Ok(crypto) = window.crypto() {
                js_sys::Reflect::has(&crypto, &"subtle".into()).unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        }
    }
    
    /// Get supported crypto algorithms
    pub fn get_supported_algorithms() -> Vec<String> {
        let mut algorithms = Vec::new();
        
        if is_web_crypto_available() {
            // These are commonly supported in modern browsers
            algorithms.push("RSA-OAEP".to_string());
            algorithms.push("AES-GCM".to_string());
            algorithms.push("SHA-256".to_string());
            algorithms.push("HMAC".to_string());
            algorithms.push("PBKDF2".to_string());
        }
        
        algorithms
    }
    
    /// Convert bytes to hex string
    pub fn bytes_to_hex(bytes: &[u8]) -> String {
        bytes.iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
    
    /// Convert hex string to bytes
    pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>> {
        if hex.len() % 2 != 0 {
            return Err(anyhow::anyhow!("Hex string must have even length"));
        }
        
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Invalid hex string: {}", e))
    }
    
    /// Base64 encode bytes
    pub fn base64_encode(bytes: &[u8]) -> String {
        let uint8_array = Uint8Array::from(bytes);
        let window = window().expect("No window object");
        let btoa = js_sys::Reflect::get(&window, &"btoa".into()).expect("No btoa function");
        let btoa_fn = btoa.dyn_into::<js_sys::Function>().expect("btoa is not a function");
        
        // Convert Uint8Array to string for btoa
        let binary_string = String::from_utf8_lossy(bytes);
        let result = btoa_fn.call1(&window, &binary_string.into()).expect("btoa failed");
        result.as_string().expect("btoa result is not a string")
    }
    
    /// Base64 decode string
    pub fn base64_decode(encoded: &str) -> Result<Vec<u8>> {
        let window = window().ok_or_else(|| anyhow::anyhow!("No window object"))?;
        let atob = js_sys::Reflect::get(&window, &"atob".into())
            .map_err(|_| anyhow::anyhow!("No atob function"))?;
        let atob_fn = atob.dyn_into::<js_sys::Function>()
            .map_err(|_| anyhow::anyhow!("atob is not a function"))?;
        
        let result = atob_fn.call1(&window, &encoded.into())
            .map_err(|e| anyhow::anyhow!("atob failed: {:?}", e))?;
        let binary_string = result.as_string()
            .ok_or_else(|| anyhow::anyhow!("atob result is not a string"))?;
        
        Ok(binary_string.into_bytes())
    }
}
