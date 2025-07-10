-- Synapse Participant Registry Schema
-- Migration: 001_create_participants_table

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Main participants table
CREATE TABLE participants (
    global_id VARCHAR(255) PRIMARY KEY,
    display_name VARCHAR(255) NOT NULL,
    entity_type JSONB NOT NULL,
    identities JSONB NOT NULL DEFAULT '[]',
    discovery_permissions JSONB NOT NULL,
    availability JSONB NOT NULL,
    contact_preferences JSONB NOT NULL DEFAULT '{}',
    trust_ratings JSONB NOT NULL DEFAULT '{}',
    topic_subscriptions JSONB NOT NULL DEFAULT '[]',
    organizational_context JSONB,
    public_key BYTEA,
    supported_protocols JSONB NOT NULL DEFAULT '[]',
    last_seen TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Trust balances table
CREATE TABLE trust_balances (
    participant_id VARCHAR(255) PRIMARY KEY REFERENCES participants(global_id) ON DELETE CASCADE,
    total_points INTEGER NOT NULL DEFAULT 0,
    available_points INTEGER NOT NULL DEFAULT 0,
    staked_points INTEGER NOT NULL DEFAULT 0,
    earned_lifetime INTEGER NOT NULL DEFAULT 0,
    last_activity TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    decay_rate DOUBLE PRECISION NOT NULL DEFAULT 0.02
);

-- Blockchain blocks table
CREATE TABLE blockchain_blocks (
    block_number BIGINT PRIMARY KEY,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    previous_hash VARCHAR(64) NOT NULL,
    hash VARCHAR(64) NOT NULL UNIQUE,
    transactions JSONB NOT NULL DEFAULT '[]',
    nonce BIGINT NOT NULL DEFAULT 0,
    validator VARCHAR(255) NOT NULL
);

-- Blockchain transactions table (for easier querying)
CREATE TABLE blockchain_transactions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    block_number BIGINT NOT NULL REFERENCES blockchain_blocks(block_number) ON DELETE CASCADE,
    transaction_type VARCHAR(50) NOT NULL,
    transaction_data JSONB NOT NULL,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    hash BYTEA NOT NULL
);

-- Participant relationships table
CREATE TABLE participant_relationships (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    from_participant VARCHAR(255) NOT NULL REFERENCES participants(global_id) ON DELETE CASCADE,
    to_participant VARCHAR(255) NOT NULL REFERENCES participants(global_id) ON DELETE CASCADE,
    relationship_type VARCHAR(50) NOT NULL,
    trust_score INTEGER CHECK (trust_score >= 0 AND trust_score <= 100),
    relationship_context JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    UNIQUE(from_participant, to_participant)
);

-- Direct trust ratings table
CREATE TABLE trust_ratings (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    rater_id VARCHAR(255) NOT NULL REFERENCES participants(global_id) ON DELETE CASCADE,
    subject_id VARCHAR(255) NOT NULL REFERENCES participants(global_id) ON DELETE CASCADE,
    category VARCHAR(50) NOT NULL,
    score INTEGER NOT NULL CHECK (score >= 0 AND score <= 100),
    comment TEXT,
    relationship_context VARCHAR(50),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    UNIQUE(rater_id, subject_id, category)
);

-- Create indexes for better performance
CREATE INDEX idx_participants_entity_type ON participants USING GIN (entity_type);
CREATE INDEX idx_participants_discovery ON participants USING GIN (discovery_permissions);
CREATE INDEX idx_participants_last_seen ON participants (last_seen);
CREATE INDEX idx_participants_updated_at ON participants (updated_at);

CREATE INDEX idx_trust_balances_total_points ON trust_balances (total_points);
CREATE INDEX idx_trust_balances_last_activity ON trust_balances (last_activity);

CREATE INDEX idx_blockchain_blocks_timestamp ON blockchain_blocks (timestamp);
CREATE INDEX idx_blockchain_blocks_validator ON blockchain_blocks (validator);

CREATE INDEX idx_blockchain_transactions_type ON blockchain_transactions (transaction_type);
CREATE INDEX idx_blockchain_transactions_timestamp ON blockchain_transactions (timestamp);
CREATE INDEX idx_blockchain_transactions_data ON blockchain_transactions USING GIN (transaction_data);

CREATE INDEX idx_relationships_from ON participant_relationships (from_participant);
CREATE INDEX idx_relationships_to ON participant_relationships (to_participant);
CREATE INDEX idx_relationships_type ON participant_relationships (relationship_type);

CREATE INDEX idx_trust_ratings_subject ON trust_ratings (subject_id);
CREATE INDEX idx_trust_ratings_rater ON trust_ratings (rater_id);
CREATE INDEX idx_trust_ratings_category ON trust_ratings (category);
CREATE INDEX idx_trust_ratings_score ON trust_ratings (score);

-- Create function to automatically update updated_at timestamps
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Create triggers for automatic timestamp updates
CREATE TRIGGER update_participants_updated_at BEFORE UPDATE
    ON participants FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_relationships_updated_at BEFORE UPDATE
    ON participant_relationships FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Create views for common queries
CREATE VIEW participant_trust_summary AS
SELECT 
    p.global_id,
    p.display_name,
    p.entity_type,
    tb.total_points,
    tb.available_points,
    tb.staked_points,
    COALESCE(AVG(tr.score), 0) as avg_trust_rating,
    COUNT(tr.score) as total_ratings
FROM participants p
LEFT JOIN trust_balances tb ON p.global_id = tb.participant_id
LEFT JOIN trust_ratings tr ON p.global_id = tr.subject_id
GROUP BY p.global_id, p.display_name, p.entity_type, tb.total_points, tb.available_points, tb.staked_points;

-- Create view for blockchain statistics
CREATE VIEW blockchain_stats AS
SELECT 
    COUNT(*) as total_blocks,
    MAX(block_number) as latest_block,
    COUNT(DISTINCT validator) as unique_validators,
    AVG(EXTRACT(EPOCH FROM (timestamp - LAG(timestamp) OVER (ORDER BY block_number)))) as avg_block_time
FROM blockchain_blocks;

-- Comments for documentation
COMMENT ON TABLE participants IS 'Core participant profiles in the Synapse network';
COMMENT ON TABLE trust_balances IS 'Trust point balances for blockchain staking system';
COMMENT ON TABLE blockchain_blocks IS 'Synapse blockchain blocks for trust verification';
COMMENT ON TABLE blockchain_transactions IS 'Individual transactions within blocks';
COMMENT ON TABLE participant_relationships IS 'Direct relationships between participants';
COMMENT ON TABLE trust_ratings IS 'Direct trust ratings between participants';

COMMENT ON COLUMN participants.entity_type IS 'Type of entity: Human, AiModel, Service, Bot, Organization';
COMMENT ON COLUMN participants.discovery_permissions IS 'Privacy and discoverability settings';
COMMENT ON COLUMN participants.availability IS 'Current availability status and preferences';
COMMENT ON COLUMN trust_balances.decay_rate IS 'Monthly decay rate for trust points (default 2%)';
COMMENT ON COLUMN blockchain_blocks.hash IS 'SHA-256 hash of the block';
COMMENT ON COLUMN blockchain_transactions.transaction_type IS 'Type: TrustReport, Stake, Unstake, Transfer, Registration';
