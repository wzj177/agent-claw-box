-- Agent backups table for upgrade/export workflow.
CREATE TABLE IF NOT EXISTS agent_backups (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id    TEXT NOT NULL,
    backup_path TEXT NOT NULL,
    version     TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_agent_backups_agent ON agent_backups(agent_id);
