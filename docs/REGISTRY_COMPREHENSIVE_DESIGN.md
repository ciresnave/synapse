# 🏗️ EMRP Participant Registry Design Document

## Executive Summary

This document outlines the design for a comprehensive, privacy-respecting participant registry for the Email-Based Message Routing Protocol (EMRP). The registry supports contextual lookup, privacy controls, trust/reputation systems, relationship-based priority, topic-based addressing, delegation/forwarding, federation, and organizational boundaries.

## Architecture Overview

### Design Principles

1. **Privacy by Design**: Users control their discoverability and data sharing
2. **Contextual Intelligence**: Rich context enables natural contact resolution  
3. **Trust-Based Networking**: Multiple trust metrics inform routing decisions
4. **Federation-Ready**: Decentralized architecture supporting multiple servers
5. **Cross-Protocol Compatibility**: Bridges existing communication systems
6. **Organizational Awareness**: Respects business boundaries and policies

### Core Components

```
┌─────────────────────────────────────────────────────────────────┐
│                    EMRP Participant Registry                    │
├─────────────────────────────────────────────────────────────────┤
│  Contact Resolver  │  Privacy Manager  │  Federation Client     │
│  ┌──────────────┐  │  ┌─────────────┐   │  ┌──────────────────┐  │
│  │ Contextual   │  │  │ Consent     │   │  │ Server Registry  │  │
│  │ Lookup       │  │  │ Management  │   │  │ & Trust          │  │
│  │              │  │  │             │   │  │                  │  │
│  │ Topic-Based  │  │  │ Privacy     │   │  │ Protocol         │  │
│  │ Routing      │  │  │ Filters     │   │  │ Adapters         │  │
│  │              │  │  │             │   │  │                  │  │
│  │ Trust        │  │  │ Audit       │   │  │ Cross-Server     │  │
│  │ Calculation  │  │  │ Logging     │   │  │ Communication    │  │
│  └──────────────┘  │  └─────────────┘   │  └──────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│                    Participant Registry                         │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  Profiles  │  Trust Ratings  │  Relationships  │  Topics │   │
│  └──────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                    Storage & Federation                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │ Local DB    │  │ Cache Layer │  │ Federation Network      │  │
│  │ (SQLite/    │  │ (Redis)     │  │ (Server-to-Server)      │  │
│  │ PostgreSQL) │  │             │  │                         │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Detailed Component Design

### 1. Participant Profiles

#### Core Profile Structure
```rust
pub struct ParticipantProfile {
    // Identity Information
    pub global_id: String,
    pub identities: Vec<IdentityContext>,
    
    // Privacy & Discovery
    pub discovery_permissions: DiscoveryPermissions,
    pub forwarding_rules: Vec<ForwardingRule>,
    
    // Trust & Relationships
    pub trust_ratings: TrustRatings,
    pub relationships: Vec<Relationship>,
    
    // Capabilities & Interests
    pub topic_subscriptions: Vec<TopicSubscription>,
    pub organizational_context: Option<OrganizationalContext>,
    
    // Routing & Priority
    pub priority_settings: PrioritySettings,
    
    // Federation & Cross-Protocol
    pub federation_config: FederationConfig,
    
    // Metadata
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

#### Identity Contexts
Support multiple identities per person:
- **Personal**: Family, friends, personal projects
- **Professional**: Work, career, professional networks
- **Service**: Bots, automated services, monitoring
- **Organization**: Department, team, role-based identities

### 2. Privacy & Discoverability System

#### Discoverability Levels

| Level | Search Visibility | Contact Method | Use Case |
|-------|------------------|----------------|----------|
| **Public** | Appears in searches | Anyone can contact | Public figures, services |
| **Unlisted** | No search results | Contact if known | Most professionals |
| **Private** | Invisible | Explicit permission only | Privacy-conscious users |
| **Stealth** | Completely hidden | Pre-authorized only | High-security scenarios |

#### Privacy Controls
```rust
pub struct DiscoveryPermissions {
    pub discoverability: DiscoverabilityLevel,
    pub searchable_fields: Vec<SearchableField>,
    pub contact_methods: Vec<ContactMethod>,
    pub require_introduction: bool,
    pub auto_accept_from: Vec<String>,
    pub min_trust_score: Option<f64>,
    pub geographic_restrictions: Option<Vec<String>>,
    pub time_restrictions: Option<TimeRestrictions>,
}
```

### 3. Trust & Reputation System

#### Dual Trust Model

**A. Network Proximity** (Objective)
- Degrees of separation in the EMRP network
- Calculated via graph traversal algorithms
- Used for filtering and relevance scoring

**B. Explicit Trust Ratings** (Subjective)
- Entity-to-entity trust scores (0-100)
- Category-specific ratings (technical, communication, reliability)
- Network-wide reputation scores
- Weighted by relationship strength and recency

#### Trust Categories
1. **Communication**: Respectful, clear, responsive
2. **Technical**: Competent, knowledgeable, helpful
3. **Collaboration**: Good team player, reliable
4. **Privacy**: Respects confidentiality and data
5. **Overall**: General trustworthiness

#### Composite Trust Calculation
```rust
composite_score = (
    network_proximity_weight * proximity_score +
    direct_trust_weight * direct_trust_score +
    network_reputation_weight * network_reputation +
    verification_weight * verification_score
) / total_weights
```

### 4. Relationship & Priority System

#### Relationship Types & Default Priorities

| Relationship | Default Priority | Behavior |
|-------------|------------------|----------|
| **Boss** | Critical | Always gets through |
| **Family** | High | After-hours delivery OK |
| **Direct Report** | High | Work hours prioritized |
| **Close Collaborator** | Normal | Standard processing |
| **Colleague** | Normal | Standard processing |
| **Service Provider** | Low | Batched delivery |
| **Bot/Monitor** | Background | Minimal priority |

#### Priority Escalation Rules
- **Emergency Keywords**: "urgent", "emergency", "critical"
- **Time-Based**: Higher priority during work hours
- **Relationship-Based**: Boss messages always escalate
- **Topic-Based**: Subject matter expert gets priority for their topics

### 5. Topic-Based Addressing

#### Topic Subscription Model
```rust
pub struct TopicSubscription {
    pub topic: String,
    pub subscription_type: SubscriptionType, // Expert, Interested, Learning
    pub expertise_level: ExpertiseLevel,     // Beginner to Authority
    pub availability: AvailabilityStatus,
    pub auto_respond: bool,
    pub geographic_scope: Option<Vec<String>>,
    pub organization_scope: Option<Vec<String>>,
}
```

#### Expert Discovery Algorithm
1. **Find topic subscribers** matching query criteria
2. **Filter by expertise level** (minimum threshold)
3. **Check availability status** (available, busy, offline)
4. **Apply geographic/org constraints**
5. **Calculate relevance score** based on:
   - Expertise level match
   - Recent activity in topic
   - Trust/reputation scores
   - Network proximity to requester

### 6. Organizational Boundaries

#### Access Control Levels
- **Public**: Anyone can see this information
- **Internal**: Only organization members
- **Department**: Only department members  
- **Team**: Only team members
- **Confidential**: Restricted access

#### Cross-Organization Policies
```rust
pub struct CrossOrgPermissions {
    pub allow_external_contact: bool,
    pub external_contact_approval: ApprovalLevel,
    pub partner_organizations: Vec<String>,
    pub blocked_organizations: Vec<String>,
    pub compliance_requirements: Vec<ComplianceFramework>,
}
```

### 7. Federation Architecture

#### Server-to-Server Model (Matrix-Style)
- **Home Server**: Primary server for a participant
- **Federation**: Trusted server network with shared protocols
- **Data Sovereignty**: Each server controls its participant data
- **Trust Propagation**: Reputation scores federate with decay

#### Federation Trust Levels
1. **Trusted**: Full federation, shared trust data
2. **Verified**: Verified server, limited data sharing
3. **Provisional**: Trial period, restricted access
4. **Restricted**: Limited federation capabilities
5. **Blocked**: No federation allowed

### 8. Cross-Protocol Integration

#### Protocol Adapters
- **Email**: DNS TXT records, SMTP compatibility
- **Matrix**: Native federation protocol
- **Slack/Discord/Teams**: API-based integration
- **OAuth 2.0**: Identity verification bridge

#### Identity Verification Methods
- **OAuth 2.0**: Google, Microsoft, GitHub integration
- **Email Verification**: Domain ownership proof
- **Corporate Directory**: LDAP/Active Directory
- **Web of Trust**: Peer verification chains
- **Cryptographic Proof**: Public key signatures

## Implementation Strategy

### Phase 1: Foundation (4 weeks)
**Week 1-2: Core Data Structures**
- Participant profile schema
- Basic privacy controls
- Local registry implementation

**Week 3-4: Trust & Discovery**
- Simple trust calculation
- Contextual lookup algorithms
- Privacy filtering

### Phase 2: Advanced Features (4 weeks)
**Week 5-6: Relationships & Priority**
- Relationship management
- Priority-based routing
- Time-based rules

**Week 7-8: Topics & Organizations**
- Topic subscription system
- Expert discovery algorithms
- Organizational boundary enforcement

### Phase 3: Federation (4 weeks)
**Week 9-10: Server Federation**
- Federation protocol implementation
- Server trust management
- Cross-server discovery

**Week 11-12: Protocol Integration**
- Email protocol adapter
- OAuth 2.0 identity verification
- Basic cross-protocol mapping

### Phase 4: Production Ready (4 weeks)
**Week 13-14: Performance & Security**
- Caching implementation
- Security hardening
- Rate limiting

**Week 15-16: Ecosystem Integration**
- Additional protocol adapters
- Monitoring and analytics
- Documentation and examples

## Technical Implementation Details

### Database Schema Design

#### Core Tables
```sql
-- Participant profiles
CREATE TABLE participants (
    global_id VARCHAR(255) PRIMARY KEY,
    profile_data JSONB NOT NULL,
    discovery_permissions JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Identity contexts
CREATE TABLE identities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    participant_id VARCHAR(255) REFERENCES participants(global_id),
    name VARCHAR(255) NOT NULL,
    context_type VARCHAR(50) NOT NULL,
    identity_data JSONB NOT NULL,
    active BOOLEAN DEFAULT true
);

-- Relationships
CREATE TABLE relationships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_participant VARCHAR(255) REFERENCES participants(global_id),
    to_participant VARCHAR(255) REFERENCES participants(global_id),
    relationship_type VARCHAR(50) NOT NULL,
    priority_level VARCHAR(20) NOT NULL,
    mutual BOOLEAN DEFAULT false,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Trust ratings
CREATE TABLE trust_ratings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_participant VARCHAR(255) REFERENCES participants(global_id),
    to_participant VARCHAR(255) REFERENCES participants(global_id),
    category VARCHAR(50) NOT NULL,
    score INTEGER CHECK (score >= 0 AND score <= 100),
    comment TEXT,
    given_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Topic subscriptions
CREATE TABLE topic_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    participant_id VARCHAR(255) REFERENCES participants(global_id),
    topic VARCHAR(255) NOT NULL,
    subscription_type VARCHAR(50) NOT NULL,
    expertise_level VARCHAR(20) NOT NULL,
    availability_status VARCHAR(20) NOT NULL,
    auto_respond BOOLEAN DEFAULT false,
    subscription_data JSONB
);
```

#### Indexes for Performance
```sql
-- Search indexes
CREATE INDEX idx_participants_search ON participants USING GIN ((profile_data->>'name'));
CREATE INDEX idx_identities_search ON identities USING GIN (to_tsvector('english', name));
CREATE INDEX idx_topic_search ON topic_subscriptions (topic, expertise_level);

-- Relationship indexes  
CREATE INDEX idx_relationships_from ON relationships (from_participant);
CREATE INDEX idx_relationships_to ON relationships (to_participant);
CREATE INDEX idx_relationships_type ON relationships (relationship_type);

-- Trust indexes
CREATE INDEX idx_trust_ratings_to ON trust_ratings (to_participant, category);
CREATE INDEX idx_trust_ratings_score ON trust_ratings (score DESC);
```

### Caching Strategy

#### Redis Cache Layers
```rust
pub struct CacheManager {
    // Profile cache (1 hour TTL)
    profile_cache: RedisCache<String, ParticipantProfile>,
    
    // Trust calculation cache (30 minutes TTL)  
    trust_cache: RedisCache<String, TrustInfo>,
    
    // Discovery results cache (10 minutes TTL)
    discovery_cache: RedisCache<String, Vec<ContactCandidate>>,
    
    // Topic expert cache (1 hour TTL)
    expert_cache: RedisCache<String, Vec<TopicExpert>>,
}
```

#### Cache Invalidation Strategy
- **Profile updates**: Invalidate profile and related caches
- **Trust updates**: Invalidate trust calculations for affected entities
- **Relationship changes**: Invalidate discovery and trust caches
- **Topic subscriptions**: Invalidate expert discovery caches

### API Design

#### RESTful Endpoints
```
GET  /api/v1/participants/{id}                   # Get participant profile
PUT  /api/v1/participants/{id}                   # Update participant profile
POST /api/v1/participants/{id}/contact           # Request contact permission

GET  /api/v1/discovery                           # Search participants
POST /api/v1/discovery/contextual               # Contextual lookup
POST /api/v1/discovery/topic                    # Topic-based discovery

GET  /api/v1/trust/{id}                         # Get trust info
POST /api/v1/trust/{id}/rate                    # Submit trust rating

GET  /api/v1/relationships                       # Get relationships
POST /api/v1/relationships                      # Create relationship
PUT  /api/v1/relationships/{id}                 # Update relationship

GET  /api/v1/topics                             # Get topic subscriptions
POST /api/v1/topics                             # Subscribe to topic
DELETE /api/v1/topics/{id}                      # Unsubscribe

# Federation endpoints
POST /api/v1/federation/lookup                  # Federated lookup
POST /api/v1/federation/trust-propagation       # Trust data sharing
```

#### WebSocket Events
```rust
pub enum RegistryEvent {
    ContactRequest {
        from: String,
        to: String, 
        request_id: String,
    },
    TrustRatingUpdate {
        participant_id: String,
        new_score: f64,
    },
    RelationshipUpdate {
        participant_id: String,
        relationship_type: RelationshipType,
    },
    TopicActivity {
        topic: String,
        activity_type: String,
    },
}
```

### Security Considerations

#### Authentication & Authorization
- **API Keys**: For programmatic access
- **OAuth 2.0**: For user authentication
- **mTLS**: For server-to-server federation
- **JWT Tokens**: For session management

#### Privacy Protection
- **Data Minimization**: Only store necessary information
- **Encryption**: Encrypt sensitive profile data at rest
- **Audit Logging**: Track all data access and modifications
- **GDPR Compliance**: Support data export and deletion

#### Rate Limiting
```rust
pub struct RateLimiter {
    // Discovery queries: 100/hour per participant
    discovery_limiter: TokenBucket,
    
    // Contact requests: 20/hour per participant  
    contact_limiter: TokenBucket,
    
    // Trust ratings: 50/day per participant
    trust_limiter: TokenBucket,
    
    // Federation queries: 1000/hour per server
    federation_limiter: TokenBucket,
}
```

### Monitoring & Analytics

#### Key Metrics
- **Discovery success rate**: How often lookups find relevant contacts
- **Contact request acceptance rate**: How often contact requests are approved
- **Trust score distribution**: Health of the trust network
- **Topic expert coverage**: Availability of experts per topic
- **Federation efficiency**: Performance of cross-server queries

#### Alert Conditions
- **High rejection rates**: Potential privacy issues or spam
- **Trust score anomalies**: Possible gaming or abuse
- **Federation failures**: Server connectivity issues
- **Performance degradation**: Response time increases

This comprehensive design provides the foundation for a production-ready, privacy-respecting participant registry that can scale to support large-scale EMRP deployments while maintaining user trust and regulatory compliance.
