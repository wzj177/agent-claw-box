-- AgentBox initial schema
CREATE TABLE IF NOT EXISTS agents (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    template    TEXT NOT NULL,
    instance_no INTEGER NOT NULL DEFAULT 1,
    port        INTEGER NOT NULL,
    status      TEXT NOT NULL DEFAULT 'CREATING',
    auto_start  BOOLEAN NOT NULL DEFAULT 0,
    health_url  TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Allow multiple instances of the same template
CREATE INDEX idx_agents_template ON agents(template);
CREATE INDEX idx_agents_status ON agents(status);

CREATE TABLE IF NOT EXISTS agent_metrics (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id    TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    cpu_percent REAL,
    memory_mb   REAL,
    net_rx_kb   REAL,
    net_tx_kb   REAL,
    healthy     BOOLEAN NOT NULL DEFAULT 1,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_metrics_agent ON agent_metrics(agent_id, recorded_at);
