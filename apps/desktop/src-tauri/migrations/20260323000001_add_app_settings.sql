CREATE TABLE IF NOT EXISTS app_settings (
    id                              INTEGER PRIMARY KEY CHECK (id = 1),
    instance_autostart_enabled      BOOLEAN NOT NULL DEFAULT 1,
    instance_autostart_delay_secs   INTEGER NOT NULL DEFAULT 8,
    proxy_enabled                   BOOLEAN NOT NULL DEFAULT 0,
    proxy_url                       TEXT,
    no_proxy                        TEXT,
    updated_at                      TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO app_settings (
    id,
    instance_autostart_enabled,
    instance_autostart_delay_secs,
    proxy_enabled,
    proxy_url,
    no_proxy,
    updated_at
) VALUES (1, 1, 8, 0, NULL, NULL, datetime('now'));