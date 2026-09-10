// SPDX-License-Identifier: MIT OR Apache-2.0
//! Input validation functions for security-critical operations

use regex::Regex;

/// Validate email address format
pub fn validate_email(email: &str) -> Result<(), String> {
    if email.is_empty() {
        return Err("Email cannot be empty".to_string());
    }

    if email.len() > 254 {
        return Err("Email too long".to_string());
    }

    let email_regex = Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
        .map_err(|_| "Invalid email regex")?;

    if !email_regex.is_match(email) {
        return Err("Invalid email format".to_string());
    }

    Ok(())
}

/// Validate username format
pub fn validate_username(username: &str) -> Result<(), String> {
    if username.is_empty() {
        return Err("Username cannot be empty".to_string());
    }

    if username.len() < 3 || username.len() > 64 {
        return Err("Username must be between 3 and 64 characters".to_string());
    }

    let username_regex = Regex::new(r"^[a-zA-Z0-9_.-]+$").map_err(|_| "Invalid username regex")?;

    if !username_regex.is_match(username) {
        return Err("Username contains invalid characters".to_string());
    }

    Ok(())
}

/// Validate password strength
pub fn validate_password_strength(password: &str) -> Result<(), String> {
    if password.len() < 8 {
        return Err("Password must be at least 8 characters long".to_string());
    }

    if password.len() > 128 {
        return Err("Password too long".to_string());
    }

    let has_upper = password.chars().any(|c| c.is_uppercase());
    let has_lower = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password
        .chars()
        .any(|c| "!@#$%^&*()_+-=[]{}|;:,.<>?".contains(c));

    let strength_count = [has_upper, has_lower, has_digit, has_special]
        .iter()
        .filter(|&&x| x)
        .count();

    if strength_count < 3 {
        return Err(
            "Password must contain at least 3 of: uppercase, lowercase, digit, special character"
                .to_string(),
        );
    }

    Ok(())
}

/// Validate domain name format
pub fn validate_domain(domain: &str) -> Result<(), String> {
    if domain.is_empty() {
        return Err("Domain cannot be empty".to_string());
    }

    if domain.len() > 253 {
        return Err("Domain name too long".to_string());
    }

    let domain_regex = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$")
        .map_err(|_| "Invalid domain regex")?;

    if !domain_regex.is_match(domain) {
        return Err("Invalid domain format".to_string());
    }

    Ok(())
}

/// Validate IP address (IPv4 or IPv6)
pub fn validate_ip_address(ip: &str) -> Result<(), String> {
    use std::net::{Ipv4Addr, Ipv6Addr};

    if ip.parse::<Ipv4Addr>().is_ok() || ip.parse::<Ipv6Addr>().is_ok() {
        Ok(())
    } else {
        Err("Invalid IP address format".to_string())
    }
}

/// Validate API key format
pub fn validate_api_key_format(api_key: &str) -> Result<(), String> {
    if api_key.is_empty() {
        return Err("API key cannot be empty".to_string());
    }

    if api_key.len() < 32 || api_key.len() > 128 {
        return Err("API key must be between 32 and 128 characters".to_string());
    }

    // Check for basic format requirements
    if !api_key
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err("API key contains invalid characters".to_string());
    }

    Ok(())
}

/// Validate trust report evidence size and format
pub fn validate_trust_evidence(evidence: &str) -> Result<(), String> {
    if evidence.len() > 10240 {
        // 10KB max
        return Err("Trust evidence too large (max 10KB)".to_string());
    }

    // Check for potentially dangerous content
    if evidence.contains("<script") || evidence.contains("javascript:") {
        return Err("Trust evidence contains potentially dangerous content".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_email() {
        assert!(validate_email("user@example.com").is_ok());
        assert!(validate_email("test.email+tag@domain.co.uk").is_ok());
        assert!(validate_email("").is_err());
        assert!(validate_email("invalid-email").is_err());
        assert!(validate_email("@example.com").is_err());
    }

    #[test]
    fn test_validate_username() {
        assert!(validate_username("validuser123").is_ok());
        assert!(validate_username("user.name").is_ok());
        assert!(validate_username("").is_err());
        assert!(validate_username("ab").is_err());
        assert!(validate_username("user with spaces").is_err());
    }

    #[test]
    fn test_validate_password_strength() {
        assert!(validate_password_strength("StrongPass123!").is_ok());
        assert!(validate_password_strength("weak").is_err());
        assert!(validate_password_strength("NoNumbers!").is_ok()); // Has 3/4 types: upper, lower, special
        assert!(validate_password_strength("nonumbers123").is_err()); // Only lower + digit (2/4)
        assert!(validate_password_strength("ALLUPPERCASE123").is_err()); // Only upper + digit (2/4)
    }

    #[test]
    fn test_validate_domain() {
        assert!(validate_domain("example.com").is_ok());
        assert!(validate_domain("subdomain.example.com").is_ok());
        assert!(validate_domain("").is_err());
        assert!(validate_domain("invalid domain").is_err());
    }

    #[test]
    fn test_validate_ip_address() {
        assert!(validate_ip_address("192.168.1.1").is_ok());
        assert!(validate_ip_address("::1").is_ok());
        assert!(validate_ip_address("invalid-ip").is_err());
    }
}
