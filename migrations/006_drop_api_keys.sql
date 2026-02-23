-- Migration 006: Drop API key infrastructure
-- Passphrase auth replaces per-rig API keys and enrollment tokens.

ALTER TABLE rigs ALTER COLUMN api_key_hash SET DEFAULT '';
ALTER TABLE rigs ALTER COLUMN api_key_hash DROP NOT NULL;

DROP TABLE IF EXISTS enrollment_tokens;
