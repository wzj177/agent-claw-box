//! Health checker — periodic health checks for running agents.
//!
//! Supports two strategies:
//! - **HTTP probe**: GET the agent's `health_url` and expect 2xx.
//! - **Process probe**: check the Docker container is running via `docker inspect`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Health status for a single agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthReport {
    pub agent_id: String,
    pub healthy: bool,
    pub last_check: String,
    pub detail: String,
}

/// Manages periodic health checks for all running agents.
pub struct HealthChecker {
    /// Interval between health-check rounds.
    interval: Duration,
    /// Latest reports keyed by agent_id.
    reports: Arc<RwLock<Vec<HealthReport>>>,
}

impl HealthChecker {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            interval: Duration::from_secs(interval_secs),
            reports: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get a snapshot of all latest health reports.
    pub async fn reports(&self) -> Vec<HealthReport> {
        self.reports.read().await.clone()
    }

    /// Start the background health-check loop.
    /// `agents_fn` is called each tick to get the current list of (agent_id, health_url | None).
    pub fn spawn<F>(
        &self,
        agents_fn: F,
    ) where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<(String, Option<String>)>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        let interval = self.interval;
        let reports = self.reports.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;

                let agents = agents_fn().await;
                let mut new_reports = Vec::with_capacity(agents.len());

                for (agent_id, health_url) in &agents {
                    let report = match health_url {
                        Some(url) => check_http(agent_id, url).await,
                        None => check_process(agent_id).await,
                    };
                    new_reports.push(report);
                }

                *reports.write().await = new_reports;
            }
        });
    }
}

/// HTTP health probe — GET url, expect 2xx.
async fn check_http(agent_id: &str, url: &str) -> HealthReport {
    let now = chrono::Utc::now().to_rfc3339();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    match client.get(url).send().await {
        Ok(resp) if resp.status().is_success() => {
            debug!(agent_id, url, "health check OK");
            HealthReport {
                agent_id: agent_id.to_string(),
                healthy: true,
                last_check: now,
                detail: format!("HTTP {}", resp.status()),
            }
        }
        Ok(resp) => {
            warn!(agent_id, url, status = %resp.status(), "health check failed");
            HealthReport {
                agent_id: agent_id.to_string(),
                healthy: false,
                last_check: now,
                detail: format!("HTTP {}", resp.status()),
            }
        }
        Err(e) => {
            warn!(agent_id, url, error = %e, "health check error");
            HealthReport {
                agent_id: agent_id.to_string(),
                healthy: false,
                last_check: now,
                detail: format!("error: {e}"),
            }
        }
    }
}

/// Process health probe — check Docker container is running via `docker inspect`.
async fn check_process(agent_id: &str) -> HealthReport {
    let now = chrono::Utc::now().to_rfc3339();
    let output = tokio::process::Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", agent_id])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let healthy = stdout == "true";
            if !healthy {
                warn!(agent_id, "container not running: {stdout}");
            }
            HealthReport {
                agent_id: agent_id.to_string(),
                healthy,
                last_check: now,
                detail: format!("container running={stdout}"),
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            HealthReport {
                agent_id: agent_id.to_string(),
                healthy: false,
                last_check: now,
                detail: format!("inspect failed: {stderr}"),
            }
        }
        Err(e) => HealthReport {
            agent_id: agent_id.to_string(),
            healthy: false,
            last_check: now,
            detail: format!("command error: {e}"),
        },
    }
}
