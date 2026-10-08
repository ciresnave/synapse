-- The trust system (blockchain, staking, trust balances, ratings) was removed from the core and is
-- to be rebuilt as its own project. Nothing in the core reads or writes these any more.
-- `participants.trust_ratings` stays: it is an opaque JSON column the core neither reads nor writes.
DROP VIEW IF EXISTS participant_trust_summary;
DROP VIEW IF EXISTS blockchain_stats;
DROP TABLE IF EXISTS blockchain_transactions;
DROP TABLE IF EXISTS blockchain_blocks;
DROP TABLE IF EXISTS trust_reports;
DROP TABLE IF EXISTS trust_ratings;
DROP TABLE IF EXISTS trust_balances;
