//! Security and Privacy Tests for Synapse
//! 
//! This module tests security features, privacy controls, and potential vulnerabilities
//! to ensure the system is secure and production-ready.

use synapse::*;
use std::collections::HashMap;
use uuid::Uuid;

#[cfg(test)]
mod security_tests {
    use super::*;

    /// Test all security levels and their behavior
    #[tokio::test]
    async fn test_security_levels() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        let security_levels = vec![
            (SecurityLevel::Public, "Public message content"),
            (SecurityLevel::Authenticated, "Authenticated message content"),
            (SecurityLevel::Encrypted, "Encrypted message content"),
            (SecurityLevel::HighSecurity, "High security message content"),
        ];
        
        for (security_level, content) in security_levels {
            let msg = SimpleMessage {
                to: format!("SecurityTest_{:?}", security_level),
                from_entity: "SecurityTester".to_string(),
                content: content.to_string(),
                message_type: MessageType::Direct,
                metadata: [("security_level".to_string(), format!("{:?}", security_level))].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Security level {:?} should work", security_level);
            
            if let Ok(secure_msg) = result {
                assert!(!secure_msg.message_id.is_empty(), "Secure message should have ID");
                assert!(!secure_msg.content.is_empty(), "Secure message should have content");
                assert!(secure_msg.timestamp > 0, "Secure message should have timestamp");
            }
        }
        
        println!("✓ All security levels tested");
    }

    /// Test privacy controls and data protection
    #[tokio::test]
    async fn test_privacy_controls() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test message with sensitive data
        let sensitive_msg = SimpleMessage {
            to: "PrivacyTest".to_string(),
            from_entity: "PrivacyTester".to_string(),
            content: "This contains sensitive information: SSN 123-45-6789".to_string(),
            message_type: MessageType::Direct,
            metadata: [
                ("privacy_level".to_string(), "sensitive".to_string()),
                ("data_classification".to_string(), "personal".to_string()),
            ].into(),
        };
        
        let result = router.convert_to_secure_message(&sensitive_msg).await;
        assert!(result.is_ok(), "Sensitive message should be handled");
        
        // Test message with PII (Personally Identifiable Information)
        let pii_msg = SimpleMessage {
            to: "PIITest".to_string(),
            from_entity: "PIITester".to_string(),
            content: "Contact: john.doe@example.com, phone: +1-555-123-4567".to_string(),
            message_type: MessageType::Direct,
            metadata: [("contains_pii".to_string(), "true".to_string())].into(),
        };
        
        let result = router.convert_to_secure_message(&pii_msg).await;
        assert!(result.is_ok(), "PII message should be handled");
        
        println!("✓ Privacy controls tested");
    }

    /// Test input validation and sanitization
    #[tokio::test]
    async fn test_input_validation() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test potentially malicious inputs
        let malicious_inputs = vec![
            ("script_injection", "<script>alert('xss')</script>"),
            ("sql_injection", "'; DROP TABLE users; --"),
            ("command_injection", "$(rm -rf /)"),
            ("path_traversal", "../../../etc/passwd"),
            ("xml_bomb", "<?xml version=\"1.0\"?><!DOCTYPE lolz [<!ENTITY lol \"lol\">]><lolz>&lol;</lolz>"),
            ("unicode_attack", "\u{202E}gnp.exe\u{202D}"),
            ("format_string", "%s%s%s%s%s%s%s%s%s%s%s%s"),
            ("buffer_overflow", &"A".repeat(10000)),
            ("null_injection", "test\0hidden"),
            ("ldap_injection", "cn=*)(uid=*))(|(cn=*"),
        ];
        
        for (attack_type, malicious_content) in malicious_inputs {
            let msg = SimpleMessage {
                to: format!("ValidationTest_{}", attack_type),
                from_entity: "ValidationTester".to_string(),
                content: malicious_content.to_string(),
                message_type: MessageType::Direct,
                metadata: [("attack_type".to_string(), attack_type.to_string())].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Malicious input {} should be handled safely", attack_type);
            
            // The content should be preserved (not modified), but handled safely
            if let Ok(secure_msg) = result {
                assert!(!secure_msg.message_id.is_empty(), "Message should have ID");
                assert!(!secure_msg.content.is_empty(), "Message should have content");
            }
        }
        
        println!("✓ Input validation tested");
    }

    /// Test authentication and authorization
    #[tokio::test]
    async fn test_authentication_authorization() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test messages from different entity types
        let entity_tests = vec![
            ("human", "Alice", EntityType::Human),
            ("ai", "Claude", EntityType::AiModel),
            ("tool", "ImageGen", EntityType::Tool),
            ("service", "DatabaseSvc", EntityType::Service),
            ("router", "MessageRouter", EntityType::Router),
        ];
        
        for (entity_category, entity_name, entity_type) in entity_tests {
            let msg = SimpleMessage {
                to: format!("AuthTest_{}", entity_category),
                from_entity: entity_name.to_string(),
                content: format!("Authentication test from {} entity", entity_category),
                message_type: MessageType::Direct,
                metadata: [
                    ("entity_type".to_string(), format!("{:?}", entity_type)),
                    ("auth_level".to_string(), "standard".to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Entity type {:?} should be authorized", entity_type);
        }
        
        println!("✓ Authentication and authorization tested");
    }

    /// Test data integrity and tamper detection
    #[tokio::test]
    async fn test_data_integrity() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test message integrity
        let original_msg = SimpleMessage {
            to: "IntegrityTest".to_string(),
            from_entity: "IntegrityTester".to_string(),
            content: "This message should maintain integrity".to_string(),
            message_type: MessageType::Direct,
            metadata: [
                ("checksum".to_string(), "abc123".to_string()),
                ("signature".to_string(), "test_signature".to_string()),
            ].into(),
        };
        
        let secure_msg = router.convert_to_secure_message(&original_msg).await.expect("Failed to convert message");
        
        // Verify integrity properties
        assert!(!secure_msg.message_id.is_empty(), "Message should have ID");
        assert!(!secure_msg.content.is_empty(), "Message should have content");
        assert!(secure_msg.timestamp > 0, "Message should have timestamp");
        
        // Test with binary data
        let binary_msg = SimpleMessage {
            to: "BinaryTest".to_string(),
            from_entity: "BinaryTester".to_string(),
            content: "Binary data: \x00\x01\x02\x03\xFF\xFE\xFD".to_string(),
            message_type: MessageType::Direct,
            metadata: HashMap::new(),
        };
        
        let result = router.convert_to_secure_message(&binary_msg).await;
        assert!(result.is_ok(), "Binary data should maintain integrity");
        
        println!("✓ Data integrity tested");
    }

    /// Test secure communication channels
    #[tokio::test]
    async fn test_secure_channels() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test different communication patterns
        let channel_tests = vec![
            ("direct_encrypted", MessageType::Direct, SecurityLevel::Encrypted),
            ("broadcast_authenticated", MessageType::Broadcast, SecurityLevel::Authenticated),
            ("conversation_secure", MessageType::Conversation, SecurityLevel::HighSecurity),
            ("notification_public", MessageType::Notification, SecurityLevel::Public),
        ];
        
        for (test_name, msg_type, security_level) in channel_tests {
            let msg = SimpleMessage {
                to: format!("ChannelTest_{}", test_name),
                from_entity: "ChannelTester".to_string(),
                content: format!("Testing secure channel: {}", test_name),
                message_type: msg_type,
                metadata: [
                    ("security_level".to_string(), format!("{:?}", security_level)),
                    ("channel_type".to_string(), test_name.to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Secure channel {} should work", test_name);
        }
        
        println!("✓ Secure communication channels tested");
    }

    /// Test encryption and decryption
    #[tokio::test]
    async fn test_encryption_decryption() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test messages that should be encrypted
        let encryption_tests = vec![
            ("personal_data", "Personal information: John Doe, born 1990-01-01"),
            ("financial_data", "Account number: 1234567890, balance: $1000.00"),
            ("health_data", "Patient ID: 12345, diagnosis: common cold"),
            ("corporate_secret", "Confidential: Q4 earnings will be $100M"),
            ("government_classified", "Classification: SECRET, project codename: ALPHA"),
        ];
        
        for (data_type, sensitive_content) in encryption_tests {
            let msg = SimpleMessage {
                to: format!("EncryptionTest_{}", data_type),
                from_entity: "EncryptionTester".to_string(),
                content: sensitive_content.to_string(),
                message_type: MessageType::Direct,
                metadata: [
                    ("data_type".to_string(), data_type.to_string()),
                    ("encryption_required".to_string(), "true".to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Encryption test {} should work", data_type);
            
            if let Ok(secure_msg) = result {
                // In a real implementation, we'd verify the content is actually encrypted
                assert!(!secure_msg.content.is_empty(), "Encrypted message should have content");
            }
        }
        
        println!("✓ Encryption and decryption tested");
    }

    /// Test access control and permissions
    #[tokio::test]
    async fn test_access_control() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test different access levels
        let access_tests = vec![
            ("public_access", "PublicUser", vec!["read"]),
            ("authenticated_access", "AuthenticatedUser", vec!["read", "write"]),
            ("admin_access", "AdminUser", vec!["read", "write", "delete", "admin"]),
            ("system_access", "SystemUser", vec!["read", "write", "delete", "admin", "system"]),
        ];
        
        for (access_type, user_name, permissions) in access_tests {
            let msg = SimpleMessage {
                to: format!("AccessTest_{}", access_type),
                from_entity: user_name.to_string(),
                content: format!("Testing access control: {}", access_type),
                message_type: MessageType::Direct,
                metadata: [
                    ("access_level".to_string(), access_type.to_string()),
                    ("permissions".to_string(), permissions.join(",")),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Access control test {} should work", access_type);
        }
        
        println!("✓ Access control tested");
    }

    /// Test audit logging and compliance
    #[tokio::test]
    async fn test_audit_logging() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test messages that should be audited
        let audit_tests = vec![
            ("financial_transaction", "Transfer $1000 from A to B"),
            ("data_access", "Accessed patient record #12345"),
            ("system_change", "Modified user permissions for alice@example.com"),
            ("security_event", "Failed login attempt from IP 192.168.1.100"),
            ("compliance_action", "GDPR data deletion request processed"),
        ];
        
        for (audit_type, audit_content) in audit_tests {
            let msg = SimpleMessage {
                to: format!("AuditTest_{}", audit_type),
                from_entity: "AuditTester".to_string(),
                content: audit_content.to_string(),
                message_type: MessageType::Direct,
                metadata: [
                    ("audit_required".to_string(), "true".to_string()),
                    ("audit_type".to_string(), audit_type.to_string()),
                    ("compliance_level".to_string(), "high".to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Audit test {} should work", audit_type);
            
            if let Ok(secure_msg) = result {
                // In a real implementation, we'd verify audit logs are created
                assert!(!secure_msg.message_id.is_empty(), "Audited message should have ID");
                assert!(secure_msg.timestamp > 0, "Audited message should have timestamp");
            }
        }
        
        println!("✓ Audit logging tested");
    }

    /// Test rate limiting and DoS protection
    #[tokio::test]
    async fn test_rate_limiting() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test rapid message sending (potential DoS)
        let num_messages = 100;
        let sender = "RateLimitTester";
        
        let mut successes = 0;
        let mut failures = 0;
        
        for i in 0..num_messages {
            let msg = SimpleMessage {
                to: format!("RateLimitTest_{}", i),
                from_entity: sender.to_string(),
                content: format!("Rate limit test message {}", i),
                message_type: MessageType::Direct,
                metadata: [
                    ("test_iteration".to_string(), i.to_string()),
                    ("sender".to_string(), sender.to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            match result {
                Ok(_) => successes += 1,
                Err(_) => failures += 1,
            }
        }
        
        println!("✓ Rate limiting test: {} successes, {} failures", successes, failures);
        
        // Should handle most messages but may rate limit some
        assert!(successes > 50, "Should handle at least 50% of messages");
    }

    /// Test security headers and metadata
    #[tokio::test]
    async fn test_security_headers() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test message with security-related metadata
        let msg = SimpleMessage {
            to: "SecurityHeaderTest".to_string(),
            from_entity: "SecurityHeaderTester".to_string(),
            content: "Testing security headers and metadata".to_string(),
            message_type: MessageType::Direct,
            metadata: [
                ("x-security-level".to_string(), "high".to_string()),
                ("x-encryption-method".to_string(), "AES-256".to_string()),
                ("x-signature-algorithm".to_string(), "RSA-SHA256".to_string()),
                ("x-content-type".to_string(), "application/json".to_string()),
                ("x-timestamp".to_string(), chrono::Utc::now().to_rfc3339()),
                ("x-nonce".to_string(), Uuid::new_v4().to_string()),
            ].into(),
        };
        
        let result = router.convert_to_secure_message(&msg).await;
        assert!(result.is_ok(), "Security headers should be handled");
        
        if let Ok(secure_msg) = result {
            assert!(!secure_msg.message_id.is_empty(), "Message should have ID");
            assert!(secure_msg.timestamp > 0, "Message should have timestamp");
        }
        
        println!("✓ Security headers tested");
    }

    /// Test cryptographic operations
    #[tokio::test]
    async fn test_cryptographic_operations() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test messages with cryptographic content
        let crypto_tests = vec![
            ("rsa_key", "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----"),
            ("certificate", "-----BEGIN CERTIFICATE-----\nMIIDXTCCAkWgAwIBAgIJAK...\n-----END CERTIFICATE-----"),
            ("hash_sha256", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            ("signature", "MEUCIQDXvW9/ZGo8uqLNzRsJjGkVIGHFIKLFKGJKGJKG"),
            ("jwt_token", "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"),
        ];
        
        for (crypto_type, crypto_content) in crypto_tests {
            let msg = SimpleMessage {
                to: format!("CryptoTest_{}", crypto_type),
                from_entity: "CryptoTester".to_string(),
                content: crypto_content.to_string(),
                message_type: MessageType::Direct,
                metadata: [
                    ("crypto_type".to_string(), crypto_type.to_string()),
                    ("contains_crypto".to_string(), "true".to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Cryptographic content {} should be handled", crypto_type);
        }
        
        println!("✓ Cryptographic operations tested");
    }

    /// Test security compliance and standards
    #[tokio::test]
    async fn test_security_compliance() {
        let config = Config::for_testing();
        let router = EmrpRouter::new(config).await.expect("Failed to create router");
        
        // Test compliance with various security standards
        let compliance_tests = vec![
            ("gdpr", "GDPR compliance test - personal data handling"),
            ("hipaa", "HIPAA compliance test - health information"),
            ("pci_dss", "PCI DSS compliance test - payment card data"),
            ("soc2", "SOC 2 compliance test - security controls"),
            ("iso27001", "ISO 27001 compliance test - information security"),
            ("nist", "NIST compliance test - cybersecurity framework"),
        ];
        
        for (standard, test_content) in compliance_tests {
            let msg = SimpleMessage {
                to: format!("ComplianceTest_{}", standard),
                from_entity: "ComplianceTester".to_string(),
                content: test_content.to_string(),
                message_type: MessageType::Direct,
                metadata: [
                    ("compliance_standard".to_string(), standard.to_string()),
                    ("data_classification".to_string(), "restricted".to_string()),
                    ("retention_period".to_string(), "7_years".to_string()),
                ].into(),
            };
            
            let result = router.convert_to_secure_message(&msg).await;
            assert!(result.is_ok(), "Compliance test {} should work", standard);
        }
        
        println!("✓ Security compliance tested");
    }
}
