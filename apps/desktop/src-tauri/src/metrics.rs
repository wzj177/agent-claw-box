//! Background metrics collector — periodically gathers metrics for both Docker
//! containers and native agents, then stores results in the `agent_metrics` table.
//!
//! Also prunes old records (>24 h) on each cycle to prevent unbounded growth.

use std::time::Duration;

use crate::state::AppState;
use sqlx::SqlitePool;
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
    state: AppState,
    interval_secs: u64,
) {
    let interval = Duration::from_secs(interval_secs);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if let Err(e) = collect_once(&state).await {
                warn!(error = %e, "metrics collection failed");
            }
            if let Err(e) = prune(&state.db).await {
                warn!(error = %e, "metrics pruning failed");
            }
        }
    });
}

/// Run one collection cycle: query running containers, fetch stats, insert rows.
async fn collect_once(state: &AppState) -> anyhow::Result<()> {
    collect_docker_metrics(state).await?;
    collect_native_metrics(state).await?;
    Ok(())
}

async fn collect_docker_metrics(state: &AppState) -> anyhow::Result<()> {
    // Get running Docker-backed agents so we can map container_name → agent_id per VM.
    let agents: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, container_name, vm_name FROM agents WHERE status = 'RUNNING' AND install_method != 'native'"
    )
    .fetch_all(&state.db)
    .await?;

    if agents.is_empty() {
        return Ok(());
    }

    let mut by_vm: std::collections::HashMap<String, Vec<(String, String)>> = std::collections::HashMap::new();
    for (agent_id, container_name, vm_name) in agents {
        by_vm.entry(vm_name).or_default().push((agent_id, container_name));
    }

    for (vm_name, vm_agents) in by_vm {
        let docker = state.docker_for_vm_name(&vm_name);
        let stats = fetch_docker_stats(&docker).await?;

        for row in &stats {
            if let Some((agent_id, _)) = vm_agents.iter().find(|(_, cn)| cn == &row.container_name) {
                insert_metrics_row(&state.db, agent_id, row.cpu_percent, row.memory_mb, row.net_rx_kb, row.net_tx_kb, true).await?;

                debug!(
                    agent_id,
                    vm_name,
                    cpu = row.cpu_percent,
                    mem = row.memory_mb,
                    "recorded metrics"
                );
            }
        }
    }

    Ok(())
}

async fn collect_native_metrics(state: &AppState) -> anyhow::Result<()> {
    let agents: Vec<(String, String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, container_name, vm_name, health_url, port FROM agents WHERE status = 'RUNNING' AND install_method = 'native'"
    )
    .fetch_all(&state.db)
    .await?;

    if agents.is_empty() {
        return Ok(());
    }

    for (agent_id, container_name, vm_name, health_url, port) in agents {
        let vm = state.vm_for_name(&vm_name);
        let Some((cpu_percent, memory_mb)) = fetch_native_process_stats(&vm, &container_name).await? else {
            continue;
        };
        let (net_rx_kb, net_tx_kb) = fetch_native_network_stats(&vm, port as u16).await?;

        let healthy = if health_url.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_some() {
            probe_health(health_url.as_deref()).await
        } else {
            true
        };

        insert_metrics_row(&state.db, &agent_id, cpu_percent, memory_mb, net_rx_kb, net_tx_kb, healthy).await?;

        debug!(
            agent_id,
            vm_name,
            cpu = cpu_percent,
            mem = memory_mb,
            "recorded native metrics"
        );
    }

    Ok(())
}

async fn insert_metrics_row(
    db: &SqlitePool,
    agent_id: &str,
    cpu_percent: f64,
    memory_mb: f64,
    net_rx_kb: f64,
    net_tx_kb: f64,
    healthy: bool,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO agent_metrics (agent_id, cpu_percent, memory_mb, net_rx_kb, net_tx_kb, healthy, recorded_at)
         VALUES (?, ?, ?, ?, ?, ?, datetime('now'))"
    )
    .bind(agent_id)
    .bind(cpu_percent)
    .bind(memory_mb)
    .bind(net_rx_kb)
    .bind(net_tx_kb)
    .bind(healthy)
    .execute(db)
    .await?;

    Ok(())
}

fn native_pid_path(container_name: &str) -> String {
    format!("$HOME/.agentbox/pids/{container_name}.pid")
}

fn native_net_rx_comment(port: u16) -> String {
    format!("agentbox-net-rx-{port}")
}

fn native_net_tx_comment(port: u16) -> String {
    format!("agentbox-net-tx-{port}")
}

async fn fetch_native_process_stats(
    vm: &agentbox_vm::VmManager,
    container_name: &str,
) -> anyhow::Result<Option<(f64, f64)>> {
    let pid_path = native_pid_path(container_name);
    let cmd = format!(
        "pid=\"$(cat {pid_path} 2>/dev/null || true)\"; \
         if [ -z \"$pid\" ] || ! kill -0 \"$pid\" 2>/dev/null; then echo MISSING; exit 0; fi; \
         ps -o %cpu=,rss= -p \"$pid\" | awk 'NF >= 2 {{ gsub(/^[ \\t]+|[ \\t]+$/, \"\", $1); gsub(/^[ \\t]+|[ \\t]+$/, \"\", $2); printf \"%s\\t%s\\n\", $1, $2 }}'"
    );

    let output = vm.shell_run(&cmd).await?;
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed == "MISSING" {
        return Ok(None);
    }

    let mut parts = trimmed.split_whitespace();
    let cpu_percent = parts.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let rss_kb = parts.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let memory_mb = rss_kb / 1024.0;

    Ok(Some((cpu_percent, memory_mb)))
}

async fn fetch_native_network_stats(
    vm: &agentbox_vm::VmManager,
    port: u16,
) -> anyhow::Result<(f64, f64)> {
    let rx_comment = native_net_rx_comment(port);
    let tx_comment = native_net_tx_comment(port);
    let cmd = format!(
        "if ! command -v iptables >/dev/null 2>&1 || ! sudo -n true >/dev/null 2>&1; then echo 0 0; exit 0; fi; \
         rx=$(sudo -n iptables-save -c 2>/dev/null | awk '/{rx_comment}/ {{ if (match($1, /\\[([0-9]+):/, a)) {{ print a[1]; exit }} }}'); \
         tx=$(sudo -n iptables-save -c 2>/dev/null | awk '/{tx_comment}/ {{ if (match($1, /\\[([0-9]+):/, a)) {{ print a[1]; exit }} }}'); \
         printf '%s %s\n' \"${{rx:-0}}\" \"${{tx:-0}}\""
    );

    let output = vm.shell_run(&cmd).await?;
    let mut parts = output.split_whitespace();
    let rx_bytes = parts.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    let tx_bytes = parts.next().unwrap_or("0").parse::<f64>().unwrap_or(0.0);
    Ok((rx_bytes / 1024.0, tx_bytes / 1024.0))
}

async fn probe_health(health_url: Option<&str>) -> bool {
    let Some(url) = health_url.filter(|value| !value.trim().is_empty()) else {
        return true;
    };

    reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
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
    docker: &agentbox_docker::ContainerRuntime,
) -> anyhow::Result<Vec<StatsRow>> {
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
