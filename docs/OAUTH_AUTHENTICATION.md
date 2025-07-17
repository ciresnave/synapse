# 🔐 Synapse OAuth/Authentication Integration

## Overview

The Synapse OAuth/Authentication Integration system provides comprehensive enterprise-grade authentication and authorization capabilities. This system supports multiple authentication providers, OAuth 2.0/OpenID Connect flows, and seamless integration with all Synapse transport layers.

## 🎯 Key Features

### 1. Multi-Provider Support

- **OAuth 2.0 Providers**: Google, Microsoft, GitHub, GitLab, and custom providers
- **OpenID Connect**: Full OIDC support with ID tokens and user information
- **Enterprise SSO**: SAML, LDAP, and Active Directory integration
- **Custom Providers**: Extensible framework for custom authentication systems

### 2. Comprehensive OAuth Flows

- **Authorization Code Flow**: Standard OAuth 2.0 authorization code flow
- **Client Credentials Flow**: Service-to-service authentication
- **Device Code Flow**: Device authentication for CLI and IoT applications
- **Refresh Token Flow**: Automatic token refresh and management

### 3. Advanced Security Features

- **JWT Token Management**: Secure token storage and validation
- **Token Refresh**: Automatic token refresh with fallback strategies
- **Scope Management**: Fine-grained permission control
- **Multi-Factor Authentication**: Support for MFA and enhanced security

### 4. Enterprise Integration

- **Role-Based Access Control (RBAC)**: Advanced permission management
- **API Key Management**: Secure API key generation and validation
- **Session Management**: Secure session handling and cleanup
- **Audit Logging**: Comprehensive authentication audit trails

## 🏗️ Architecture

### Authentication Flow

```text
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│     Client      │    │   Auth Server   │    │  OAuth Provider │
│   Application   │    │   (Synapse)     │    │  (Google, etc.) │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                        │                        │
         │ 1. Initiate Auth       │                        │
         │───────────────────────►│                        │
         │                        │                        │
         │ 2. Redirect to OAuth   │                        │
         │◄───────────────────────│                        │
         │                        │                        │
         │ 3. User Authorization  │                        │
         │───────────────────────────────────────────────►│
         │                        │                        │
         │ 4. Authorization Code  │                        │
         │◄───────────────────────────────────────────────│
         │                        │                        │
         │ 5. Send Auth Code      │                        │
         │───────────────────────►│                        │
         │                        │                        │
         │                        │ 6. Exchange for Token │
         │                        │───────────────────────►│
         │                        │                        │
         │                        │ 7. Access Token       │
         │                        │◄───────────────────────│
         │                        │                        │
         │ 8. Access Token        │                        │
         │◄───────────────────────│                        │
         │                        │                        │
         │ 9. Authenticated API   │                        │
         │───────────────────────►│                        │
```

### Integration Architecture

```rust
// Authentication integration with all Synapse components
use synapse::{
    auth::{AuthProvider, OAuthConfig, TokenManager},
    transport::Transport,
    router::EnhancedSynapseRouter,
};
```

## 🚀 Quick Start

### Basic OAuth Setup

```rust
use synapse::auth::{OAuthProvider, OAuthConfig, AuthenticatedTransport};

#[tokio::main]
async fn main() -> Result<()> {
    // Configure OAuth provider
    let oauth_config = OAuthConfig {
        provider: OAuthProvider::Google,
        client_id: "your-client-id".to_string(),
        client_secret: "your-client-secret".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["openid".to_string(), "profile".to_string(), "email".to_string()],
    };
    
    // Create authenticated transport
    let transport = AuthenticatedTransport::new(
        "my-entity",
        8080,
        oauth_config,
    ).await?;
    
    // All transport operations are now authenticated
    let message = SecureMessage::new("Hello, authenticated world!");
    transport.send_message("target", &message).await?;
    
    Ok(())
}
```

### Custom Authentication Provider

```rust
use synapse::auth::{AuthProvider, AuthResult, UserInfo};

// Implement custom authentication provider
struct CustomAuthProvider {
    api_endpoint: String,
    api_key: String,
}

#[async_trait]
impl AuthProvider for CustomAuthProvider {
    async fn authenticate(&self, credentials: &Credentials) -> Result<AuthResult> {
        // Custom authentication logic
        let response = self.validate_credentials(credentials).await?;
        
        Ok(AuthResult {
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            expires_in: response.expires_in,
            user_info: UserInfo {
                id: response.user_id,
                email: response.email,
                name: response.name,
                roles: response.roles,
            },
        })
    }
    
    async fn refresh_token(&self, refresh_token: &str) -> Result<AuthResult> {
        // Token refresh logic
        // ...
    }
    
    async fn validate_token(&self, token: &str) -> Result<UserInfo> {
        // Token validation logic
        // ...
    }
}
```

## 🔧 OAuth Provider Configuration

### Google OAuth Configuration

```rust
use synapse::auth::{GoogleOAuthProvider, GoogleOAuthConfig};

let google_config = GoogleOAuthConfig {
    client_id: "your-google-client-id".to_string(),
    client_secret: "your-google-client-secret".to_string(),
    redirect_uri: "http://localhost:8080/auth/google/callback".to_string(),
    scopes: vec![
        "https://www.googleapis.com/auth/userinfo.email".to_string(),
        "https://www.googleapis.com/auth/userinfo.profile".to_string(),
        "openid".to_string(),
    ],
    hosted_domain: Some("your-company.com".to_string()), // Optional
};

let provider = GoogleOAuthProvider::new(google_config).await?;
```

### Microsoft OAuth Configuration

```rust
use synapse::auth::{MicrosoftOAuthProvider, MicrosoftOAuthConfig};

let microsoft_config = MicrosoftOAuthConfig {
    client_id: "your-microsoft-client-id".to_string(),
    client_secret: "your-microsoft-client-secret".to_string(),
    tenant_id: "your-tenant-id".to_string(),
    redirect_uri: "http://localhost:8080/auth/microsoft/callback".to_string(),
    scopes: vec![
        "https://graph.microsoft.com/User.Read".to_string(),
        "openid".to_string(),
        "profile".to_string(),
        "email".to_string(),
    ],
};

let provider = MicrosoftOAuthProvider::new(microsoft_config).await?;
```

### GitHub OAuth Configuration

```rust
use synapse::auth::{GitHubOAuthProvider, GitHubOAuthConfig};

let github_config = GitHubOAuthConfig {
    client_id: "your-github-client-id".to_string(),
    client_secret: "your-github-client-secret".to_string(),
    redirect_uri: "http://localhost:8080/auth/github/callback".to_string(),
    scopes: vec![
        "user:email".to_string(),
        "read:user".to_string(),
    ],
};

let provider = GitHubOAuthProvider::new(github_config).await?;
```

## 🔐 Advanced Authentication Features

### JWT Token Management

```rust
use synapse::auth::{JWTManager, JWTConfig, Claims};

// Configure JWT manager
let jwt_config = JWTConfig {
    secret: "your-jwt-secret".to_string(),
    issuer: "synapse-auth".to_string(),
    audience: "synapse-api".to_string(),
    expiration: Duration::from_hours(24),
    refresh_threshold: Duration::from_hours(2),
};

let jwt_manager = JWTManager::new(jwt_config).await?;

// Generate JWT token
let claims = Claims {
    sub: "user-123".to_string(),
    email: "user@example.com".to_string(),
    roles: vec!["user".to_string(), "admin".to_string()],
    exp: (Utc::now() + Duration::hours(24)).timestamp() as usize,
};

let token = jwt_manager.generate_token(&claims).await?;

// Validate JWT token
let validated_claims = jwt_manager.validate_token(&token).await?;
println!("User: {}", validated_claims.sub);
```

### Multi-Factor Authentication

```rust
use synapse::auth::{MFAProvider, MFAConfig, MFAType};

// Configure MFA
let mfa_config = MFAConfig {
    enabled: true,
    required_for_admin: true,
    totp_issuer: "Synapse".to_string(),
    backup_codes_count: 10,
    sms_provider: Some(SMSProvider::Twilio),
};

let mfa_provider = MFAProvider::new(mfa_config).await?;

// Enable MFA for user
let mfa_secret = mfa_provider.generate_totp_secret("user-123").await?;
let qr_code = mfa_provider.generate_qr_code("user-123", &mfa_secret).await?;

// Verify MFA code
let is_valid = mfa_provider.verify_totp("user-123", "123456").await?;
```

### API Key Management

```rust
use synapse::auth::{APIKeyManager, APIKeyConfig, APIKeyScope};

// Configure API key manager
let api_key_config = APIKeyConfig {
    key_length: 32,
    prefix: "syn_".to_string(),
    expiration: Some(Duration::from_days(90)),
    rate_limit: Some(1000), // requests per hour
};

let api_key_manager = APIKeyManager::new(api_key_config).await?;

// Generate API key
let api_key = api_key_manager.generate_key(
    "user-123",
    vec![APIKeyScope::Read, APIKeyScope::Write],
    "My API Key".to_string(),
).await?;

// Validate API key
let key_info = api_key_manager.validate_key(&api_key).await?;
println!("API Key owner: {}", key_info.owner);
```

## 🔄 Authentication Flows

### Authorization Code Flow

```rust
use synapse::auth::{AuthFlow, AuthorizationCodeFlow};

// Initiate authorization code flow
let auth_flow = AuthorizationCodeFlow::new(oauth_config).await?;

// Get authorization URL
let auth_url = auth_flow.get_authorization_url(&["openid", "profile", "email"]).await?;
println!("Visit: {}", auth_url);

// Handle callback with authorization code
let auth_code = "authorization_code_from_callback";
let token_result = auth_flow.exchange_code_for_token(auth_code).await?;

println!("Access Token: {}", token_result.access_token);
println!("User: {:?}", token_result.user_info);
```

### Client Credentials Flow

```rust
use synapse::auth::ClientCredentialsFlow;

// Service-to-service authentication
let client_flow = ClientCredentialsFlow::new(
    "service-client-id".to_string(),
    "service-client-secret".to_string(),
    "https://oauth.provider.com/token".to_string(),
).await?;

// Get service token
let service_token = client_flow.get_token(&["api:read", "api:write"]).await?;

// Use service token for API calls
let authenticated_request = client_flow.create_authenticated_request(
    &service_token,
    "https://api.example.com/data",
).await?;
```

### Device Code Flow

```rust
use synapse::auth::DeviceCodeFlow;

// Device authentication for CLI/IoT
let device_flow = DeviceCodeFlow::new(oauth_config).await?;

// Initiate device flow
let device_auth = device_flow.initiate_device_authorization(&["openid", "profile"]).await?;

println!("Visit: {}", device_auth.verification_uri);
println!("Enter code: {}", device_auth.user_code);

// Poll for authorization
let token_result = device_flow.poll_for_token(&device_auth.device_code).await?;
println!("Device authenticated: {:?}", token_result.user_info);
```

## 🎯 Role-Based Access Control

### RBAC Configuration

```rust
use synapse::auth::{RBACManager, Role, Permission, Resource};

// Configure RBAC
let rbac_manager = RBACManager::new().await?;

// Define roles
let admin_role = Role {
    name: "admin".to_string(),
    permissions: vec![
        Permission::All,
    ],
};

let user_role = Role {
    name: "user".to_string(),
    permissions: vec![
        Permission::Read(Resource::Messages),
        Permission::Write(Resource::Messages),
    ],
};

// Create roles
rbac_manager.create_role(&admin_role).await?;
rbac_manager.create_role(&user_role).await?;

// Assign roles to users
rbac_manager.assign_role("user-123", "user").await?;
rbac_manager.assign_role("admin-456", "admin").await?;
```

### Permission Checking

```rust
// Check permissions
let can_read = rbac_manager.check_permission(
    "user-123",
    &Permission::Read(Resource::Messages),
).await?;

let can_admin = rbac_manager.check_permission(
    "user-123",
    &Permission::All,
).await?;

// Enforce permissions in transport
#[async_trait]
impl AuthenticatedTransport {
    async fn send_message_with_auth(&self, target: &str, message: &SecureMessage) -> Result<String> {
        // Check permissions
        let user_info = self.get_current_user().await?;
        let can_send = self.rbac_manager.check_permission(
            &user_info.id,
            &Permission::Write(Resource::Messages),
        ).await?;
        
        if !can_send {
            return Err("Insufficient permissions".into());
        }
        
        // Send message
        self.transport.send_message(target, message).await
    }
}
```

## 🔍 Session Management

### Session Configuration

```rust
use synapse::auth::{SessionManager, SessionConfig};

// Configure session management
let session_config = SessionConfig {
    session_timeout: Duration::from_hours(8),
    idle_timeout: Duration::from_hours(2),
    max_concurrent_sessions: 5,
    secure_cookies: true,
    same_site: SameSite::Strict,
    session_storage: SessionStorage::Redis,
};

let session_manager = SessionManager::new(session_config).await?;
```

### Session Operations

```rust
// Create session
let session = session_manager.create_session(&user_info).await?;

// Validate session
let session_info = session_manager.validate_session(&session.id).await?;

// Update session activity
session_manager.update_activity(&session.id).await?;

// Destroy session
session_manager.destroy_session(&session.id).await?;

// Clean up expired sessions
session_manager.cleanup_expired_sessions().await?;
```

## 🔗 Transport Integration

### Authenticated Transport Usage

```rust
use synapse::transport::AuthenticatedTransport;

// Create authenticated transport
let auth_transport = AuthenticatedTransport::new(
    "authenticated-entity",
    8080,
    oauth_config,
).await?;

// Send authenticated message
let message = SecureMessage::new("Authenticated message");
let result = auth_transport.send_authenticated_message("target", &message).await?;

// Get current user information
let user_info = auth_transport.get_current_user().await?;
println!("Authenticated as: {}", user_info.email);
```

### Router Integration

```rust
use synapse::router::AuthenticatedRouter;

// Create authenticated router
let auth_router = AuthenticatedRouter::new(
    router_config,
    oauth_config,
    "authenticated-entity",
).await?;

// Send message with authentication
auth_router.send_message_with_auth(
    "target",
    "Hello from authenticated user!",
    MessageType::Direct,
    SecurityLevel::Authenticated,
).await?;
```

## 📊 Audit Logging

### Authentication Audit

```rust
use synapse::auth::{AuditLogger, AuditEvent};

// Configure audit logging
let audit_logger = AuditLogger::new(AuditConfig {
    log_level: AuditLevel::Info,
    storage: AuditStorage::Database,
    retention_days: 365,
}).await?;

// Log authentication events
audit_logger.log_event(AuditEvent::LoginSuccess {
    user_id: "user-123".to_string(),
    ip_address: "192.168.1.100".to_string(),
    user_agent: "Mozilla/5.0...".to_string(),
    timestamp: Utc::now(),
}).await?;

// Query audit logs
let logs = audit_logger.query_logs(AuditQuery {
    user_id: Some("user-123".to_string()),
    event_type: Some(AuditEventType::Login),
    start_time: Some(Utc::now() - Duration::days(30)),
    end_time: Some(Utc::now()),
}).await?;
```

## 🌐 Enterprise Features

### SAML Integration

```rust
use synapse::auth::{SAMLProvider, SAMLConfig};

// Configure SAML provider
let saml_config = SAMLConfig {
    entity_id: "https://your-app.com/saml".to_string(),
    sso_url: "https://idp.company.com/sso".to_string(),
    slo_url: "https://idp.company.com/slo".to_string(),
    certificate: "-----BEGIN CERTIFICATE-----...".to_string(),
    private_key: "-----BEGIN PRIVATE KEY-----...".to_string(),
};

let saml_provider = SAMLProvider::new(saml_config).await?;

// Handle SAML response
let saml_response = "base64_encoded_saml_response";
let user_info = saml_provider.process_saml_response(saml_response).await?;
```

### LDAP Integration

```rust
use synapse::auth::{LDAPProvider, LDAPConfig};

// Configure LDAP provider
let ldap_config = LDAPConfig {
    server: "ldap://ldap.company.com:389".to_string(),
    base_dn: "dc=company,dc=com".to_string(),
    bind_dn: "cn=admin,dc=company,dc=com".to_string(),
    bind_password: "admin_password".to_string(),
    user_filter: "(&(objectClass=person)(uid={username}))".to_string(),
    group_filter: "(&(objectClass=group)(member={user_dn}))".to_string(),
};

let ldap_provider = LDAPProvider::new(ldap_config).await?;

// Authenticate against LDAP
let user_info = ldap_provider.authenticate("username", "password").await?;
```

## 📚 API Reference

### Core Authentication Types

```rust
// OAuth configuration
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub provider: OAuthProvider,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
}

// Authentication result
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Duration,
    pub token_type: String,
    pub user_info: UserInfo,
}

// User information
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub name: String,
    pub roles: Vec<String>,
    pub permissions: Vec<Permission>,
    pub metadata: HashMap<String, String>,
}
```

### Authentication Provider Trait

```rust
#[async_trait]
pub trait AuthProvider: Send + Sync {
    async fn authenticate(&self, credentials: &Credentials) -> Result<AuthResult>;
    async fn refresh_token(&self, refresh_token: &str) -> Result<AuthResult>;
    async fn validate_token(&self, token: &str) -> Result<UserInfo>;
    async fn revoke_token(&self, token: &str) -> Result<()>;
    async fn get_user_info(&self, token: &str) -> Result<UserInfo>;
}
```

## 🎉 Conclusion

The Synapse OAuth/Authentication Integration system provides comprehensive enterprise-grade authentication and authorization capabilities. Key benefits include:

- **Multi-Provider Support**: OAuth 2.0, OpenID Connect, SAML, and LDAP integration
- **Enterprise Features**: RBAC, MFA, API keys, and audit logging
- **Seamless Integration**: Works with all Synapse transport layers and components
- **Security Best Practices**: JWT tokens, secure sessions, and comprehensive audit trails
- **Flexible Architecture**: Extensible provider system for custom authentication

The authentication system is essential for enterprise applications requiring secure, scalable, and comprehensive authentication and authorization capabilities.

For more information, see the [Authentication API Documentation](../api/auth.md) and [OAuth Integration Examples](../examples/oauth_demo.rs).
