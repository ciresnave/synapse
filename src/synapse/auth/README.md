# Authentication Framework Integration for Synapse

This module integrates the `auth-framework` crate with Synapse's neural communication network to provide enhanced authentication, authorization, and key management capabilities.

## Features

- **Multiple Authentication Methods**: Password, OAuth2, Email link, MFA
- **Trust System Integration**: Authentication events automatically update trust ratings
- **Privacy-Aware Identity Verification**: Respects Synapse's privacy levels
- **WebCrypto Integration**: For browser-based environments
- **Role-Based Access Control**: For API endpoints and resources
- **Token Management**: JWT-based authentication tokens

## Usage

### Enable the Authentication Module

Add the `enhanced-auth` feature flag when building Synapse:

```bash
cargo build --features enhanced-auth
```

### Initialize Authentication

```rust
use synapse::{Synapse, Config};
use synapse::auth::{SynapseAuth, SynapseAuthConfig};

// Create Synapse instance
let config = Config::default();
let synapse = Synapse::new(config).await?;

// Create auth configuration
let auth_config = SynapseAuthConfig::default();

// Initialize auth module
let auth = SynapseAuth::new(
    auth_config, 
    Arc::clone(&synapse.registry)
).await?;

// Register auth event listener
let trust_bridge = AuthTrustBridge::new(
    Arc::clone(&synapse.registry),
    Arc::clone(&synapse.trust_manager)
);
auth.auth_framework.register_event_listener(Box::new(trust_bridge));
```

### Integrate with Your APIs

```rust
// Create auth API handler
let auth_api = Arc::new(AuthApi::new(Arc::clone(&auth)));

// Create auth middleware
let auth_middleware = Arc::new(AuthMiddleware::new(
    Arc::clone(&auth),
    Arc::clone(&synapse.registry)
));

// Use in your API handlers
async fn handle_protected_request(req: Request) -> Response {
    let token = extract_token_from_request(&req);
    let auth_context = auth_middleware.authenticate(token).await;
    
    // Check permission
    if !auth_middleware.check_permission(
        &auth_context,
        &[ParticipantRole::User],
        SecurityClearance::Standard
    ) {
        return Response::unauthorized();
    }
    
    // Process authenticated request
    // ...
}
```

## Authentication Methods

### 1. Password Authentication

Secure password authentication with argon2 hashing.

### 2. OAuth2 Integration

Connect with external identity providers:

- Google
- GitHub
- Microsoft
- Custom OAuth2 providers

### 3. Passwordless Authentication

Email-based magic links for passwordless login.

### 4. Multi-Factor Authentication (MFA)

- TOTP (Time-based One-Time Password)
- Email verification codes
- SMS verification codes
- Hardware security keys (WebAuthn)

## Trust System Integration

Authentication events automatically update the participant's trust ratings:

| Auth Event | Trust Effect |
|------------|-------------|
| Email Verification | Basic verification level |
| OAuth Login | Enhanced verification level |
| MFA Setup | Strong verification level |
| Hardware Key | Strong verification level |

## WebCrypto Integration

For browser environments, the authentication module integrates with the WebCrypto API for client-side cryptographic operations:

- Key generation
- Digital signatures
- Encryption/decryption
- Secure key storage

## Security Considerations

1. **Key Management**: Private keys never leave the client device
2. **Token Security**: Short-lived JWTs with proper validation
3. **Rate Limiting**: Prevent brute-force attacks
4. **MFA Enforcement**: For sensitive operations

## Configuration Options

```rust
SynapseAuthConfig {
    // Require MFA for sensitive operations like key management
    require_mfa_for_sensitive_operations: true,
    
    // Map auth methods to verification levels
    verification_level_mapping: {
        "password": VerificationLevel::Basic,
        "email_otp": VerificationLevel::Confirmed,
        "oauth2": VerificationLevel::Enhanced,
        "hardware_token": VerificationLevel::Strong,
    },
    
    // Automatically upgrade trust on stronger auth
    auto_upgrade_trust_on_stronger_auth: true,
    
    // OAuth provider configurations
    oauth_providers: [
        OAuthProviderConfig {
            provider_name: "google",
            client_id: "...",
            client_secret: "...",
            redirect_uri: "...",
            scopes: ["profile", "email"],
            verification_level: VerificationLevel::Enhanced,
        }
    ],
}
```
