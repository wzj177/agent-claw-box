//! Background metrics collector — periodically runs `docker stats` for running
//! containers and stores results in the `agent_metrics` table.
//!
//! Also prunes old records (>24 h) on each cycle to prevent unbounded growth.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Parsed row from `docker stats --no-stream`.
#[derive(Debug)]
struct StatsRow {
    container_name: String,
    cpu_percent: f64,
    memory_mb: f64,
    net_rx_kb: f64,
    net_tx_kb: f64,
}

/// Spawn a background task that collects metrics every `interval_secs` seconds.
pub fn spawn_collector(
    db: SqlitePool,
    docker: Arc<RwLock<agentbox_docker::ContainerRuntime>>,
    interval_secs: u64,
) {
    let interval = Duration::from_secs(interval_secs);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if let Err(e) = collect_once(&db, &docker).await {
                warn!(error = %e, "metrics collection failed");
            }
            if let Err(e) = prune(&db).await {
                warn!(error = %e, "metrics pruning failed");
            }
        }
    });
}

/// Run one collection cycle: query running containers, fetch stats, insert rows.
async fn collect_once(
    db: &SqlitePool,
    docker: &Arc<RwLock<agentbox_docker::ContainerRuntime>>,
) -> anyhow::Result<()> {
    // Get running agents from DB so we can map container_name → agent_id.
    let agents: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, container_name FROM agents WHERE status = 'RUNNING'"
    )
    .fetch_all(db)
    .await?;

    if agents.is_empty() {
        return Ok(());
    }

    let stats = fetch_docker_stats(docker).await?;

    for row in &stats {
        // Find matching agent
        if let Some((agent_id, _)) = agents.iter().find(|(_, cn)| cn == &row.container_name) {
            sqlx::query(
                "INSERT INTO agent_metrics (agent_id, cpu_percent, memory_mb, net_rx_kb, net_tx_kb, healthy, recorded_at)
                 VALUES (?, ?, ?, ?, ?, 1, datetime('now'))"
            )
            .bind(agent_id)
            .bind(row.cpu_percent)
            .bind(row.memory_mb)
            .bind(row.net_rx_kb)
            .bind(row.net_tx_kb)
            .execute(db)
            .await?;

            debug!(
                agent_id,
                cpu = row.cpu_percent,
                mem = row.memory_mb,
                "recorded metrics"
            );
        }
    }

    Ok(())
}

/// Delete metrics older than 24 hours.
async fn prune(db: &SqlitePool) -> anyhow::Result<()> {
    let deleted = sqlx::query(
        "DELETE FROM agent_metrics WHERE recorded_at < datetime('now', '-24 hours')"
    )
    .execute(db)
    .await?;

    if deleted.rows_affected() > 0 {
        debug!(rows = deleted.rows_affected(), "pruned old metrics");
    }
    Ok(())
}

/// Call `docker stats --no-stream --format` and parse the output.
async fn fetch_docker_stats(
    docker: &Arc<RwLock<agentbox_docker::ContainerRuntime>>,
) -> anyhow::Result<Vec<StatsRow>> {
    let docker = docker.read().await;
    let stdout = docker.stats().await?;

    let mut rows = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            continue;
        }

        let container_name = parts[0].to_string();
        let cpu_percent = parse_percent(parts[1]);
        let memory_mb = parse_memory_mb(parts[2]);
        let (net_rx_kb, net_tx_kb) = parse_net_io(parts[3]);

        rows.push(StatsRow {
            container_name,
            cpu_percent,
            memory_mb,
            net_rx_kb,
            net_tx_kb,
        });
    }

    Ok(rows)
}

/// Parse "12.34%" → 12.34
fn parse_percent(s: &str) -> f64 {
    s.trim().trim_end_matches('%').parse::<f64>().unwrap_or(0.0)
}

/// Parse "123.4MiB / 1.5GiB" → 123.4 (take the usage part)
fn parse_memory_mb(s: &str) -> f64 {
    let usage = s.split('/').next().unwrap_or("0").trim();
    parse_size_to_mb(usage)
}

/// Parse "1.23kB / 4.56MB" → (rx_kb, tx_kb)
fn parse_net_io(s: &str) -> (f64, f64) {
    let parts: Vec<&str> = s.split('/').collect();
    let rx = parts.first().map(|p| parse_size_to_kb(p.trim())).unwrap_or(0.0);
    let tx = parts.get(1).map(|p| parse_size_to_kb(p.trim())).unwrap_or(0.0);
    (rx, tx)
}

/// Convert a human-readable size (e.g. "123.4MiB", "1.5GiB", "456KiB") to MB.
fn parse_size_to_mb(s: &str) -> f64 {
    let s = s.trim();
    if let Some(v) = s.strip_suffix("GiB") {
        v.trim().parse::<f64>().unwrap_or(0.0) * 1024.0
    } else if let Some(v) = s.strip_suffix("MiB") {
        v.trim().parse::<f64>().unwrap_or(0.0)
    } else if let Some(v) = s.strip_suffix("KiB") {
        v.trim().parse::<f64>().unwrap_or(0.0) / 1024.0
    } else if let Some(v) = s.strip_suffix("GB") {
        v.trim().parse::<f64>().unwrap_or(0.0) * 1000.0
    } else if let Some(v) = s.strip_suffix("MB") {
        v.trim().parse::<f64>().unwrap_or(0.0)
    } else if let Some(v) = s.strip_suffix("kB") {
        v.trim().parse::<f64>().unwrap_or(0.0) / 1000.0
    } else if let Some(v) = s.strip_suffix('B') {
        v.trim().parse::<f64>().unwrap_or(0.0) / 1_000_000.0
    } else {
        s.parse::<f64>().unwrap_or(0.0)
    }
}

/// Convert a human-readable size to KB.
fn parse_size_to_kb(s: &str) -> f64 {
    parse_size_to_mb(s) * 1024.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_percent_normal() {
        assert!((parse_percent("12.34%") - 12.34).abs() < 0.001);
        assert!((parse_percent("0.00%") - 0.0).abs() < 0.001);
        assert!((parse_percent("100.00%") - 100.0).abs() < 0.001);
    }

    #[test]
    fn parse_percent_edge_cases() {
        assert!((parse_percent("  5.5%  ") - 5.5).abs() < 0.001);
        assert!((parse_percent("invalid") - 0.0).abs() < 0.001);
        assert!((parse_percent("") - 0.0).abs() < 0.001);
    }

    #[test]
    fn parse_memory_mb_mib() {
        assert!((parse_memory_mb("123.4MiB / 1.5GiB") - 123.4).abs() < 0.1);
    }

    #[test]
    fn parse_memory_mb_gib() {
        assert!((parse_memory_mb("1.5GiB / 4GiB") - 1536.0).abs() < 0.1);
    }

    #[test]
    fn parse_memory_mb_kib() {
        assert!((parse_memory_mb("512KiB / 1GiB") - 0.5).abs() < 0.1);
    }

    #[test]
    fn parse_net_io_kb() {
        let (rx, tx) = parse_net_io("1.23kB / 4.56MB");
        // 1.23kB = 1.23/1000 MB = 0.00123 MB * 1024 KB ≈ 1.26 KB
        assert!(rx > 0.0);
        assert!(tx > 0.0);
    }

    #[test]
    fn parse_net_io_mb() {
        let (rx, tx) = parse_net_io("10.5MB / 20.3MB");
        assert!(rx > 10_000.0); // > 10 MB in KB
        assert!(tx > 20_000.0);
    }

    #[test]
    fn parse_size_to_mb_all_units() {
        assert!((parse_size_to_mb("1GiB") - 1024.0).abs() < 0.1);
        assert!((parse_size_to_mb("100MiB") - 100.0).abs() < 0.1);
        assert!((parse_size_to_mb("1024KiB") - 1.0).abs() < 0.1);
        assert!((parse_size_to_mb("1GB") - 1000.0).abs() < 0.1);
        assert!((parse_size_to_mb("500MB") - 500.0).abs() < 0.1);
        assert!((parse_size_to_mb("500kB") - 0.5).abs() < 0.1);
        assert!((parse_size_to_mb("1000000B") - 1.0).abs() < 0.1);
    }
}
