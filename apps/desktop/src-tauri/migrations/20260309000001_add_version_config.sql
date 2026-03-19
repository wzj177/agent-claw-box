-- Add version, install_method, and container_name to agents table.
-- Add agent_configs table for storing per-agent configuration key-value pairs.

ALTER TABLE agents ADD COLUMN version TEXT NOT NULL DEFAULT '';
ALTER TABLE agents ADD COLUMN install_method TEXT NOT NULL DEFAULT 'docker';
ALTER TABLE agents ADD COLUMN container_name TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS agent_configs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id    TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    config_key  TEXT NOT NULL,
    config_value TEXT NOT NULL DEFAULT '',
    is_secret   BOOLEAN NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(agent_id, config_key)
);

CREATE INDEX idx_agent_configs_agent ON agent_configs(agent_id);
