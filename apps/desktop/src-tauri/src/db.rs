//! Database helpers — SQLite via sqlx.

use anyhow::Result;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::PathBuf;

/// Get the database file path (~/.agentbox/data.db).
pub fn db_path() -> PathBuf {
    let home = dirs_next::home_dir().expect("cannot determine home directory");
    let dir = home.join(".agentbox");
    std::fs::create_dir_all(&dir).expect("cannot create ~/.agentbox");
    dir.join("data.db")
}

/// Create a connection pool and run migrations.
pub async fn init_pool() -> Result<SqlitePool> {
    let path = db_path();
    let url = format!("sqlite:{}?mode=rwc", path.display());

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}
