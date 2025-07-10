# Synapse Core Implementation

## Project Structure Setup

```bash
mkdir -p src/synapse/{models,services,storage,blockchain,api}
mkdir -p src/synapse/blockchain/{consensus,verification,staking}
```

## Core Dependencies (Cargo.toml)

```toml
[package]
name = "synapse"
version = "0.1.0"
edition = "2021"

[dependencies]
# Async runtime
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"

# Database
sqlx = { version = "0.7", features = ["runtime-tokio-rustls", "postgres", "uuid", "chrono", "json"] }
redis = { version = "0.24", features = ["tokio-comp"] }

# Blockchain & Crypto
sha2 = "0.10"
ring = "0.17"
ed25519-dalek = "2.0"
blake3 = "1.5"

# Networking
reqwest = { version = "0.11", features = ["json"] }
tokio-tungstenite = "0.20"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
bincode = "1.3"

# Utilities
uuid = { version = "1.6", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1.0"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"

# Search
tantivy = "0.21"

# Configuration
config = "0.14"
```

## Implementation Plan

### Phase 1: Core Registry Foundation (Week 1)

**Files to create:**
- `src/synapse/mod.rs` - Module root
- `src/synapse/models/mod.rs` - Data structures
- `src/synapse/models/participant.rs` - Participant profiles
- `src/synapse/models/trust.rs` - Trust system models
- `src/synapse/storage/database.rs` - Database layer
- `src/synapse/storage/cache.rs` - Redis caching

**Goals:**
- Basic participant registration and retrieval
- Database schema and migrations
- Trust point tracking (without blockchain initially)

### Phase 2: Blockchain Integration (Week 2)

**Files to create:**
- `src/synapse/blockchain/mod.rs` - Blockchain root
- `src/synapse/blockchain/block.rs` - Block structure
- `src/synapse/blockchain/consensus.rs` - Trust consensus algorithm
- `src/synapse/blockchain/staking.rs` - Trust point staking
- `src/synapse/blockchain/verification.rs` - Report verification

**Goals:**
- Custom blockchain for trust verification
- Trust point staking system
- Consensus mechanism for reports

### Phase 3: Discovery Services (Week 3)

**Files to create:**
- `src/synapse/services/mod.rs` - Services root
- `src/synapse/services/discovery.rs` - Contact discovery
- `src/synapse/services/trust_calculator.rs` - Trust scoring
- `src/synapse/services/privacy_manager.rs` - Privacy enforcement

**Goals:**
- Contextual contact discovery
- Privacy-aware search
- Trust-based filtering

### Phase 4: API & Integration (Week 4)

**Files to create:**
- `src/synapse/api/mod.rs` - API root
- `src/synapse/api/handlers.rs` - HTTP handlers
- `src/synapse/api/websocket.rs` - Real-time events
- `src/main.rs` - Application entry point

**Goals:**
- REST API for registry operations
- WebSocket for real-time updates
- Integration testing

## Key Design Decisions

### 1. Availability for Everyone
You're right - availability should be universal, not just for experts. We'll implement:
- `AvailabilityStatus` for all participants
- `ContactPreferences` for filtering incoming requests
- `RateLimits` to prevent spam

### 2. Trust Point Economics
```rust
pub struct TrustPointEconomics {
    pub base_decay_rate: f64,        // 2% per month baseline
    pub activity_bonus: f64,         // Reduced decay for active users
    pub staking_rewards: f64,        // Bonus for successful report validation
    pub minimum_stake: u32,          // Minimum points to stake on reports
    pub consensus_threshold: f64,    // 67% agreement needed
    pub witness_requirement: u32,    // Minimum witnesses for high-impact reports
}
```

### 3. Blockchain Characteristics
- **Proof of Stake**: Trust points are the stake
- **Consensus**: Weighted voting by reputation
- **Immutable**: Once consensus reached, blocks are permanent
- **Self-Policing**: Users stake reputation to validate reports
- **Decay**: Prevents point hoarding, encourages activity

## Next Steps

Let me start implementing the core registry with these files:

1. **Basic data models** with trust integration
2. **Database schema** for participants and trust
3. **Simple blockchain** for trust verification
4. **Discovery service** with privacy controls

Should I start with the data models and database setup, or would you prefer to see the blockchain implementation first?
