//! Tauri IPC commands — all `#[tauri::command]` handlers.

use std::collections::HashMap;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tauri::State;

use agentbox_docker::{ContainerConfig, ContainerStatus};
use crate::health::HealthReport;
use crate::pty::PtySessionManager;
use crate::state::AppState;
use crate::template::{self, AgentTemplate, ConfigField};
use crate::system;

const SECRET_MASK_PLACEHOLDER: &str = "********";
const LEGACY_SECRET_MASK_PLACEHOLDER: &str = "••••••••";
const STATUS_CREATING: &str = "CREATING";
const STATUS_CREATE_FAILED: &str = "CREATE_FAILED";
const STATUS_PENDING: &str = "PENDING";
const STATUS_STARTING: &str = "STARTING";
const STATUS_RUNNING: &str = "RUNNING";
const STATUS_START_FAILED: &str = "START_FAILED";

fn is_secret_mask_placeholder(value: &str) -> bool {
    value == SECRET_MASK_PLACEHOLDER || value == LEGACY_SECRET_MASK_PLACEHOLDER
}

fn normalize_legacy_status(status: &str) -> &str {
    match status {
        "STOPPED" | "ERROR" => STATUS_PENDING,
        other => other,
    }
}

pub async fn normalize_agent_statuses(state: &AppState) -> Result<(), String> {
    sqlx::query(
        "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE status IN ('STOPPED', 'ERROR')"
    )
    .bind(STATUS_PENDING)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn recover_interrupted_agent_statuses(state: &AppState) -> Result<(), String> {
    // In-flight create/start tasks live only in the app process. If the desktop app
    // restarts (for example during `cargo tauri dev` rebuild), persisted CREATING/
    // STARTING rows are orphaned and must be marked failed instead of waiting forever.
    sqlx::query(
        "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE status = ?"
    )
    .bind(STATUS_CREATE_FAILED)
    .bind(STATUS_CREATING)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE status = ?"
    )
    .bind(STATUS_START_FAILED)
    .bind(STATUS_STARTING)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn shell_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn native_log_dir() -> &'static str {
    "$HOME/agentbox-logs"
}

fn native_log_path(container_name: &str) -> String {
    format!("{}/{container_name}.log", native_log_dir())
}

fn native_pid_dir() -> &'static str {
    "$HOME/.agentbox/pids"
}

fn native_pid_path(container_name: &str) -> String {
    format!("{}/{container_name}.pid", native_pid_dir())
}

fn openclaw_home_dir(container_name: &str) -> String {
    format!("$HOME/.agentbox/native/{container_name}")
}

fn openclaw_host_home_dir(container_name: &str) -> Result<std::path::PathBuf, String> {
    let home = dirs_next::home_dir().ok_or_else(|| "无法确定主目录".to_string())?;
    Ok(home.join(".agentbox").join("native").join(container_name))
}

fn openclaw_state_dir(container_name: &str) -> String {
    format!("{}/.openclaw", openclaw_home_dir(container_name))
}

fn openclaw_shell_exports(container_name: &str) -> String {
    format!("export OPENCLAW_HOME=\"{}\";", openclaw_home_dir(container_name))
}

async fn resolve_openclaw_gateway_token(
    state: &AppState,
    agent: &AgentInfo,
) -> Option<String> {
    let vm = vm_for_agent(state, agent);
    let openclaw_exports = openclaw_shell_exports(&agent.container_name);

    // Prefer the effective config seen inside the instance over the DB cache.
    let config_token = vm
        .shell_run(&format!(
            "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports} openclaw config get gateway.auth.token 2>/dev/null || true"
        ))
        .await
        .ok()
        .map(|output| output.trim().to_string())
        .filter(|token| !token.is_empty());

    if config_token.is_some() {
        return config_token;
    }

    sqlx::query_scalar::<_, String>(
        "SELECT config_value FROM agent_configs WHERE agent_id = ? AND config_key = 'gateway_token'"
    )
    .bind(&agent.id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .filter(|token| !token.is_empty())
}

fn append_gateway_token_to_url(url: &str, token: &str) -> String {
    if url.contains("token=") {
        return url.to_string();
    }

    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}token={token}")
}

async fn agent_record_exists(db: &sqlx::SqlitePool, agent_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agents WHERE id = ?")
        .bind(agent_id)
        .fetch_one(db)
        .await
        .map(|count| count > 0)
        .unwrap_or(false)
}

async fn provisioning_cancelled_or_deleted(state: &AppState, agent_id: &str) -> bool {
    state.is_provisioning_cancelled(agent_id).await || !agent_record_exists(&state.db, agent_id).await
}

async fn ensure_provisioning_active(
    state: &AppState,
    vm: &agentbox_vm::VmManager,
    agent_id: &str,
) -> Result<(), anyhow::Error> {
    if provisioning_cancelled_or_deleted(state, agent_id).await {
        let _ = vm.delete().await;
        anyhow::bail!("实例创建已取消");
    }

    Ok(())
}

async fn verify_vm_internet_connectivity(
    state: &AppState,
    vm: &agentbox_vm::VmManager,
    agent_id: &str,
    max_attempts: u32,
    interval_secs: u64,
) -> Result<(), anyhow::Error> {
    let check_cmd = "curl --connect-timeout 8 --max-time 15 -fsS -o /dev/null https://openclaw.ai/ && echo OK || echo FAIL";

    for attempt in 1..=max_attempts {
        ensure_provisioning_active(state, vm, agent_id).await?;
        tracing::info!(attempt, max_attempts, "Verifying VM internet connectivity...");

        let net_ok = vm.shell_run(check_cmd).await.unwrap_or_default();
        if net_ok.trim() == "OK" {
            tracing::info!(attempt, max_attempts, "VM internet connectivity OK");
            return Ok(());
        }

        tracing::warn!(attempt, max_attempts, "VM internet connectivity check failed");
        if attempt < max_attempts {
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        }
    }

    anyhow::bail!(
        "VM 无法访问互联网，请检查网络连接后重试。已尝试 {max_attempts} 次。\n\
         可在终端运行: limactl shell {vm_name} -- curl -v https://openclaw.ai/",
        vm_name = vm.name(),
    );
}

fn runtime_prefix_from_vm_name(vm_name: &str) -> Option<&'static str> {
    if vm_name.starts_with("wsl-") {
        Some("wsl")
    } else if vm_name.starts_with("qemu-") {
        Some("qemu")
    } else {
        None
    }
}

fn vm_name_for_instance(template: &str, instance_no: i64, runtime_mode: Option<&str>) -> String {
    let base = format!("agentbox-{}-{}", template, instance_no);
    match runtime_mode.map(|s| s.trim().to_lowercase()) {
        Some(mode) if mode == "wsl" => format!("wsl-{}", base),
        Some(mode) if mode == "qemu" => format!("qemu-{}", base),
        _ => base,
    }
}

fn vm_for_agent(state: &AppState, agent: &AgentInfo) -> Arc<agentbox_vm::VmManager> {
    state.vm_for_name(&agent.vm_name)
}

fn docker_for_agent(state: &AppState, agent: &AgentInfo) -> agentbox_docker::ContainerRuntime {
    state.docker_for_vm_name(&agent.vm_name)
}

struct ProvisioningCounterGuard {
    counter: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl ProvisioningCounterGuard {
    fn new(counter: Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self {
            counter: Some(counter),
        }
    }

    fn release_to_spawn(mut self) -> Arc<std::sync::atomic::AtomicUsize> {
        self.counter.take().expect("provisioning counter already released")
    }
}

impl Drop for ProvisioningCounterGuard {
    fn drop(&mut self) {
        if let Some(counter) = &self.counter {
            counter.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
}

async fn stop_native_agent_process(
    state: &AppState,
    agent: &AgentInfo,
    start_cmd: Option<&str>,
) {
    let vm = vm_for_agent(state, agent);
    let pid_path = native_pid_path(&agent.container_name);
    let pid_stop_cmd = format!(
        "if [ -f {pid_path} ]; then pid=$(cat {pid_path}); kill \"$pid\" 2>/dev/null || true; rm -f {pid_path}; fi"
    );
    let _ = vm.shell_run(&pid_stop_cmd).await;

    if agent.template == "openclaw" {
        let _ = vm
            .shell_run(&format!(
                "pkill -f 'openclaw gateway --port {}' 2>/dev/null || true",
                agent.port
            ))
            .await;
    } else if let Some(start_cmd) = start_cmd {
        let proc_name = start_cmd.split_whitespace().next().unwrap_or("agent");
        let _ = vm
            .shell_run(&format!("pkill -f '{}' 2>/dev/null || true", shell_escape(proc_name)))
            .await;
    }
}

async fn upsert_openclaw_auth_profile(
    state: &AppState,
    agent: &AgentInfo,
    provider_id: &str,
    api_key: &str,
) -> Result<(), String> {
    let vm = vm_for_agent(state, agent);
    let auth_store_path = format!(
        "{}/agents/main/agent/auth-profiles.json",
        openclaw_state_dir(&agent.container_name)
    );
    let shell_prefix = openclaw_shell_exports(&agent.container_name);
    let existing = vm
        .shell_run(&format!(
            "{shell_prefix} mkdir -p {dir} && if [ -f {auth_store_path} ]; then cat {auth_store_path}; else printf '{{}}'; fi",
            dir = format!("{}/agents/main/agent", openclaw_state_dir(&agent.container_name)),
            auth_store_path = auth_store_path,
        ))
        .await
        .map_err(|e| e.to_string())?;

    let mut root = serde_json::from_str::<serde_json::Value>(&existing)
        .unwrap_or_else(|_| serde_json::json!({}));

    if !root.is_object() {
        root = serde_json::json!({});
    }

    if root.get("profiles").and_then(|v| v.as_object()).is_none() {
        root["profiles"] = serde_json::json!({});
    }

    let profile_id = format!("{provider_id}:default");
    root["profiles"][&profile_id] = serde_json::json!({
        "type": "api_key",
        "provider": provider_id,
        "key": api_key,
    });

    let auth_json = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    let write_cmd = format!(
        "{shell_prefix} mkdir -p {dir} && printf '%s\n' '{}' > {auth_store_path}",
        auth_json.replace('\'', "'\\''"),
        dir = format!("{}/agents/main/agent", openclaw_state_dir(&agent.container_name)),
        auth_store_path = auth_store_path,
    );
    vm.shell_run(&write_cmd).await.map_err(|e| e.to_string())?;

    let order_cmd = format!(
        "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {shell_prefix} openclaw models auth order set --provider '{provider_id}' --agent main '{profile_id}' 2>/dev/null || true"
    );
    let _ = vm.shell_run(&order_cmd).await;

    Ok(())
}

fn openclaw_provider_id(provider: &str) -> &str {
    match provider {
        "qwen" => "qwen-api",
        other => other,
    }
}

async fn reconcile_agent_runtime_status(state: &AppState, agent: &AgentInfo) -> Option<String> {
    match agent.install_method.as_str() {
        "native" => {
            let vm = vm_for_agent(state, agent);
            let url = agent.health_url.clone()?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .ok()?;
            match client.get(url).send().await {
                Ok(resp) if resp.status().is_success() => Some(STATUS_RUNNING.to_string()),
                _ => {
                    if vm
                        .shell_run(&format!(
                            "bash -lc 'true </dev/tcp/127.0.0.1/{}' >/dev/null 2>&1",
                            agent.port
                        ))
                        .await
                        .is_ok()
                    {
                        Some(STATUS_RUNNING.to_string())
                    } else {
                        Some(STATUS_PENDING.to_string())
                    }
                }
            }
        }
        "docker" | "compose" => {
            let docker = docker_for_agent(state, agent);
            match docker.status(&agent.container_name).await.ok()? {
                ContainerStatus::Running => Some(STATUS_RUNNING.to_string()),
                ContainerStatus::Created | ContainerStatus::Stopped => Some(STATUS_PENDING.to_string()),
                ContainerStatus::Removing | ContainerStatus::Error(_) => None,
            }
        }
        _ => None,
    }
}

pub async fn reconcile_agent_statuses(state: &AppState) -> Result<(), String> {
    let agents: Vec<AgentInfo> = sqlx::query_as("SELECT * FROM agents")
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    for agent in agents {
        // Provisioning may take minutes for native templates. During that window
        // the port is expected to be closed, so skip reconciliation entirely.
        if matches!(agent.status.as_str(), STATUS_CREATING | STATUS_STARTING) {
            continue;
        }

        if let Some(actual_status) = reconcile_agent_runtime_status(state, &agent).await {
            let desired_status = if actual_status == STATUS_PENDING
                && matches!(agent.status.as_str(), STATUS_CREATE_FAILED | STATUS_START_FAILED)
            {
                agent.status.clone()
            } else {
                actual_status
            };

            if agent.status != desired_status {
                sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
                    .bind(&desired_status)
                    .bind(&agent.id)
                    .execute(&state.db)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    Ok(())
}

/// Agent record returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub template: String,
    pub instance_no: i64,
    pub port: u16,
    pub status: String,
    pub auto_start: bool,
    pub health_url: Option<String>,
    pub created_at: String,
    pub version: String,
    pub install_method: String,
    pub container_name: String,
    pub vm_name: String,
    pub runtime_mode: Option<String>,
    pub ubuntu_image: Option<String>,
}

/// Metrics snapshot for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentMetrics {
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub net_rx_kb: f64,
    pub net_tx_kb: f64,
    pub healthy: bool,
    pub recorded_at: String,
}

/// Template info returned to the frontend marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub install_method: String,
    pub resources: template::ResourceConfig,
    pub config_schema: Vec<ConfigField>,
}

/// Optional deployment preferences selected in marketplace.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateAgentOptions {
    /// Runtime mode on Windows: auto | wsl | qemu
    pub runtime_mode: Option<String>,
    /// Ubuntu image preference for WSL provisioning: noble | jammy | ubuntu-22.04-desktop
    pub ubuntu_image: Option<String>,
}

/// System resource info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub cpu_cores: u32,
    pub total_memory_mb: u64,
    pub available_memory_mb: u64,
    pub free_disk_gb: u64,
    pub max_instances: u32,
    pub max_running: u32,
}

/// Per-agent config entry.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentConfigEntry {
    pub config_key: String,
    pub config_value: String,
    pub is_secret: bool,
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// List all agents.
#[tauri::command]
pub async fn list_agents(state: State<'_, AppState>) -> Result<Vec<AgentInfo>, String> {
    let _ = normalize_agent_statuses(&state).await;
    let _ = reconcile_agent_statuses(&state).await;

    let mut rows: Vec<AgentInfo> = sqlx::query_as(
        "SELECT a.*, 
            (SELECT config_value FROM agent_configs c WHERE c.agent_id = a.id AND c.config_key = 'runtime_mode' LIMIT 1) AS runtime_mode,
            (SELECT config_value FROM agent_configs c WHERE c.agent_id = a.id AND c.config_key = 'ubuntu_image' LIMIT 1) AS ubuntu_image
         FROM agents a
         ORDER BY a.created_at DESC"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e: sqlx::Error| e.to_string())?;

    for row in &mut rows {
        row.status = normalize_legacy_status(&row.status).to_string();
    }

    Ok(rows)
}

/// Check whether any agent is currently being provisioned (creating VM/container).
#[tauri::command]
pub async fn is_provisioning(state: State<'_, AppState>) -> Result<bool, String> {
    if state.provisioning_count.load(std::sync::atomic::Ordering::Acquire) > 0 {
        return Ok(true);
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE status = 'CREATING'")
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

/// List available templates from the templates/ directory.
#[tauri::command]
pub async fn list_templates() -> Result<Vec<TemplateInfo>, String> {
    let templates = template::load_all_templates().map_err(|e| e.to_string())?;
    let result: Vec<TemplateInfo> = templates
        .into_iter()
        .map(|(id, t)| TemplateInfo {
            id,
            name: t.name,
            description: t.description,
            version: t.version,
            install_method: t.install_method,
            resources: t.resources,
            config_schema: t.config_schema,
        })
        .collect();
    Ok(result)
}

/// Get system resource info and instance limits.
#[tauri::command]
pub async fn get_system_info() -> Result<SystemInfo, String> {
    let res = system::detect();
    // Use average agent resource requirements for limit calculation
    let (max_instances, max_running) = system::calculate_limits(&res, 2, 2048, 5);
    Ok(SystemInfo {
        cpu_cores: res.cpu_cores,
        total_memory_mb: res.total_memory_mb,
        available_memory_mb: res.available_memory_mb,
        free_disk_gb: res.free_disk_gb,
        max_instances,
        max_running,
    })
}

/// Get recent metrics for an agent.
#[tauri::command]
pub async fn get_agent_metrics(
    id: String,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<AgentMetrics>, String> {
    let n = limit.unwrap_or(60);
    let rows: Vec<AgentMetrics> = sqlx::query_as(
        "SELECT cpu_percent, memory_mb, net_rx_kb, net_tx_kb, healthy, recorded_at
         FROM agent_metrics
         WHERE agent_id = ?
         ORDER BY recorded_at DESC
         LIMIT ?"
    )
    .bind(&id)
    .bind(n)
    .fetch_all(&state.db)
    .await
    .map_err(|e: sqlx::Error| e.to_string())?;

    Ok(rows)
}

/// Get latest health reports from the background checker.
#[tauri::command]
pub async fn get_health_reports(state: State<'_, AppState>) -> Result<Vec<HealthReport>, String> {
    Ok(state.health.reports().await)
}

/// Get config entries for an agent.
#[tauri::command]
pub async fn get_agent_config(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AgentConfigEntry>, String> {
    let rows: Vec<AgentConfigEntry> = sqlx::query_as(
        "SELECT config_key, config_value, is_secret FROM agent_configs WHERE agent_id = ? ORDER BY config_key"
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // Mask secret values for display
    let masked: Vec<AgentConfigEntry> = rows
        .into_iter()
        .map(|mut e| {
            if e.is_secret {
                if is_secret_mask_placeholder(&e.config_value) {
                    e.config_value.clear();
                } else if !e.config_value.is_empty() {
                    e.config_value = SECRET_MASK_PLACEHOLDER.to_string();
                }
            }
            e
        })
        .collect();

    Ok(masked)
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

/// Check that the VM/Docker environment is fully ready.
/// On Linux this is always true (Docker runs natively).
/// On macOS/Windows it becomes true after Lima/WSL setup completes.
fn check_vm_ready(state: &AppState) -> Result<(), String> {
    if !state.vm_ready.load(std::sync::atomic::Ordering::Acquire) {
        return Err("运行环境尚未就绪，请等待初始化完成后再操作".into());
    }
    Ok(())
}

/// Find an available port starting from a base, checking agent DB.
async fn find_available_port(db: &sqlx::SqlitePool, base_port: u16) -> Result<u16, String> {
    let used_ports: Vec<i64> = sqlx::query_scalar("SELECT port FROM agents")
        .fetch_all(db)
        .await
        .map_err(|e| e.to_string())?;

    let used: std::collections::HashSet<u16> = used_ports.iter().map(|p| *p as u16).collect();

    for port in base_port..=65535 {
        if !used.contains(&port) {
            // Also check if the port is actually free on the host
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return Ok(port);
            }
        }
    }

    Err("No available port found".to_string())
}

/// Create a new agent from a template and provision its container.
#[tauri::command]
pub async fn create_agent(
    name: String,
    template: String,
    options: Option<CreateAgentOptions>,
    state: State<'_, AppState>,
) -> Result<AgentInfo, String> {
    // Verify VM/Docker is ready before proceeding
    check_vm_ready(&state)?;

    // Reject if another agent is already being provisioned
    if state.provisioning_count.load(std::sync::atomic::Ordering::Acquire) > 0 {
        return Err("已有实例正在创建中，请等待完成后再部署新实例".into());
    }

    let creating_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE status = 'CREATING'")
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    if creating_count > 0 {
        return Err("已有实例正在创建中，请等待完成后再部署新实例".into());
    }

    let provisioning_guard = ProvisioningCounterGuard::new(state.provisioning_count.clone());

    // Load template definition
    let tmpl = template::load_template(&template).map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Determine instance number: max existing for same template + 1
    let instance_no: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(instance_no), 0) + 1 FROM agents WHERE template = ?"
    )
    .bind(&template)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // Pick an available port based on the template's first port mapping
    let base_port = tmpl.ports.first().map(|p| p.host).unwrap_or(3000);
    let port = find_available_port(&state.db, base_port).await?;

    let runtime_mode = options
        .as_ref()
        .and_then(|o| o.runtime_mode.clone())
        .filter(|s| !s.trim().is_empty());
    let ubuntu_image = options
        .as_ref()
        .and_then(|o| o.ubuntu_image.clone())
        .filter(|s| !s.trim().is_empty());

    let vm_name = vm_name_for_instance(&template, instance_no, runtime_mode.as_deref());
    let container_name = vm_name.clone();

    let health_url = tmpl.health.url.replace(
        &format!(":{}", tmpl.ports.first().map(|p| p.container).unwrap_or(0)),
        &format!(":{port}"),
    );

    // Insert DB record
    sqlx::query(
        "INSERT INTO agents (id, name, template, instance_no, port, status, health_url,
                            version, install_method, container_name, vm_name, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, 'CREATING', ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id).bind(&name).bind(&template)
    .bind(instance_no).bind(port)
    .bind(&health_url)
    .bind(&tmpl.version)
    .bind(&tmpl.install_method)
    .bind(&container_name)
        .bind(&vm_name)
    .bind(&now).bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // Persist deployment preferences for display and future diagnostics.
    if let Some(mode) = &runtime_mode {
        sqlx::query(
            "INSERT INTO agent_configs (agent_id, config_key, config_value, is_secret, updated_at)
             VALUES (?, 'runtime_mode', ?, 0, datetime('now'))"
        )
        .bind(&id)
        .bind(mode)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }
    if let Some(image) = &ubuntu_image {
        sqlx::query(
            "INSERT INTO agent_configs (agent_id, config_key, config_value, is_secret, updated_at)
             VALUES (?, 'ubuntu_image', ?, 0, datetime('now'))"
        )
        .bind(&id)
        .bind(image)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    // Capture fields before moving tmpl into async block
    let ret_version = tmpl.version.clone();
    let ret_install_method = tmpl.install_method.clone();

    // Spawn provisioning in background
    let app_state = state.inner().clone();
    let db = state.db.clone();
    let agent_id = id.clone();
    let c_name = container_name.clone();
    let vm_name_clone = vm_name.clone();
    let runtime_mode_clone = runtime_mode.clone();
    let ubuntu_image_clone = ubuntu_image.clone();
    let prov_lock = state.provisioning_lock.clone();
    let prov_count = provisioning_guard.release_to_spawn();
    tauri::async_runtime::spawn(async move {
        // Acquire provisioning lock — only one create/upgrade at a time
        let _guard = prov_lock.lock().await;
        let vm = Arc::new(agentbox_vm::VmManager::new(agentbox_vm::VmConfig {
            name: vm_name_clone.clone(),
            cpus: tmpl.resources.cpus,
            memory_mb: tmpl.resources.memory_mb,
            disk_gb: tmpl.resources.disk_gb,
            runtime_mode: runtime_mode_clone.clone(),
            ubuntu_image: ubuntu_image_clone.clone(),
        }));
        let docker = agentbox_docker::ContainerRuntime::with_prefix(vm.docker_cmd_prefix());
        // 30-minute timeout prevents agents from being stuck in CREATING forever
        // (native agents like OpenClaw may need 20+ min for npm install with retries)
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30 * 60),
            provision_agent(&app_state, &docker, &vm, &tmpl, &c_name, port, &db, &agent_id),
        ).await;
        let final_result = match result {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("创建超时（超过30分钟），请删除此实例后重试")),
        };
        let cancelled = provisioning_cancelled_or_deleted(&app_state, &agent_id).await;
        match final_result {
            Ok(result) => {
                if cancelled {
                    let _ = vm.delete().await;
                } else if result.needs_manual_install {
                    tracing::warn!(agent_id = %agent_id, "Agent set to CREATE_FAILED — manual install required");
                    let _ = sqlx::query(
                        "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?"
                    )
                    .bind(STATUS_CREATE_FAILED)
                    .bind(&agent_id)
                    .execute(&db)
                    .await;
                } else {
                    tracing::info!(agent_id = %agent_id, "Agent provisioned successfully");
                    let _ = sqlx::query(
                        "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?"
                    )
                    .bind(STATUS_RUNNING)
                    .bind(&agent_id)
                    .execute(&db)
                    .await;
                }
            }
            Err(e) => {
                if cancelled {
                    let _ = vm.delete().await;
                    tracing::info!(agent_id = %agent_id, "Provisioning cancelled while create was in progress");
                } else {
                    tracing::error!(agent_id = %agent_id, error = %e, "Failed to provision agent");
                    let _ = sqlx::query(
                        "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?"
                    )
                    .bind(STATUS_CREATE_FAILED)
                    .bind(&agent_id)
                    .execute(&db)
                    .await;
                }
            }
        }

        app_state.clear_provisioning_cancel(&agent_id).await;
        prov_count.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    });

    Ok(AgentInfo {
        id,
        name,
        template,
        instance_no,
        port,
        status: STATUS_CREATING.into(),
        auto_start: false,
        health_url: Some(health_url),
        created_at: now,
        version: ret_version,
        install_method: ret_install_method,
        container_name,
        vm_name,
        runtime_mode,
        ubuntu_image,
    })
}

/// Result of provisioning an agent.
struct ProvisionResult {
    /// If true, automatic installation failed — agent is left in STOPPED state
    /// so the user can enter the VM shell and install manually.
    needs_manual_install: bool,
}

/// Actually provision the agent container/compose/native process.
async fn provision_agent(
    state: &AppState,
    docker: &agentbox_docker::ContainerRuntime,
    vm: &agentbox_vm::VmManager,
    tmpl: &AgentTemplate,
    container_name: &str,
    host_port: u16,
    db: &sqlx::SqlitePool,
    agent_id: &str,
) -> Result<ProvisionResult, anyhow::Error> {
    use crate::network::NetworkPolicy;

    vm.ensure_ready(None).await?;
    ensure_provisioning_active(state, vm, agent_id).await?;

    // For Docker/Compose/Script methods, ensure Docker is installed on-demand
    if matches!(tmpl.install_method.as_str(), "docker" | "compose" | "script") {
        if !vm.provider().is_docker_ready(vm.name()).await {
            tracing::info!("Docker not found, installing on-demand...");
            vm.provider().install_docker(vm.name()).await?;
        }
        // Ensure agentbox-net exists
        NetworkPolicy::ensure_network_via(docker).await?;
        ensure_provisioning_active(state, vm, agent_id).await?;
    }

    let template_dir = template::templates_dir()
        .join(tmpl.name.to_lowercase())
        .to_string_lossy()
        .to_string();

    match tmpl.install_method.as_str() {
        "docker" => {
            // Pull or build the image
            let image = tmpl.runtime.image.as_deref()
                .ok_or_else(|| anyhow::anyhow!("Template missing runtime.image"))?;

            tracing::info!(image = %image, "Pulling Docker image");
            docker.pull(image).await?;

            // Prepare data directory for volumes
            let data_dir = agent_data_dir(container_name)?;

            // Build port mappings
            let mut ports = HashMap::new();
            if let Some(first_port) = tmpl.ports.first() {
                ports.insert(host_port, first_port.container);
            }

            // Build volume args
            let mut extra_args = NetworkPolicy::default().docker_args();
            for vol in &tmpl.volumes {
                let host_path = data_dir.join(&vol.host_suffix);
                std::fs::create_dir_all(&host_path)?;
                extra_args.push("-v".to_string());
                extra_args.push(format!("{}:{}", host_path.display(), vol.container));
            }

            // Create and start container
            let config = ContainerConfig {
                image: image.to_string(),
                name: container_name.to_string(),
                ports,
                env: HashMap::new(),
                cpus: Some(tmpl.resources.cpus as f64),
                memory_mb: Some(tmpl.resources.memory_mb),
            };

            docker.create_with_args(&config, &extra_args).await?;
            ensure_provisioning_active(state, vm, agent_id).await?;
        }

        "compose" => {
            let compose_file_name = tmpl.runtime.compose_file.as_deref()
                .unwrap_or("docker-compose.yml");
            let compose_path = format!("{}/{}", template_dir, compose_file_name);

            let data_dir = agent_data_dir(container_name)?;

            tracing::info!(compose = %compose_path, "Starting Docker Compose");
            run_compose(docker, &compose_path, container_name, host_port, &data_dir).await?;
            ensure_provisioning_active(state, vm, agent_id).await?;
        }

        "script" => {
            // Build custom image from template Dockerfile
            let image_tag = format!("agentbox/{}", container_name);
            tracing::info!(tag = %image_tag, dir = %template_dir, "Building custom image");
            docker.build_image(&image_tag, &template_dir).await?;

            let data_dir = agent_data_dir(container_name)?;

            let mut ports = HashMap::new();
            if let Some(first_port) = tmpl.ports.first() {
                ports.insert(host_port, first_port.container);
            }

            let mut extra_args = NetworkPolicy::default().docker_args();
            for vol in &tmpl.volumes {
                let host_path = data_dir.join(&vol.host_suffix);
                std::fs::create_dir_all(&host_path)?;
                extra_args.push("-v".to_string());
                extra_args.push(format!("{}:{}", host_path.display(), vol.container));
            }

            let config = ContainerConfig {
                image: image_tag,
                name: container_name.to_string(),
                ports,
                env: HashMap::new(),
                cpus: Some(tmpl.resources.cpus as f64),
                memory_mb: Some(tmpl.resources.memory_mb),
            };

            docker.create_with_args(&config, &extra_args).await?;
            ensure_provisioning_active(state, vm, agent_id).await?;
        }

        "native" => {
            // Run install_cmd directly in the VM (no Docker)
            let install_cmd = tmpl.runtime.install_cmd.as_deref()
                .ok_or_else(|| anyhow::anyhow!("Native template missing runtime.install_cmd"))?;
            let start_cmd = tmpl.runtime.start_cmd.as_deref()
                .ok_or_else(|| anyhow::anyhow!("Native template missing runtime.start_cmd"))?;

            let log_path = native_log_path(container_name);
            let pid_path = native_pid_path(container_name);
            vm.shell_run(&format!("mkdir -p {} {}", native_log_dir(), native_pid_dir())).await?;

            let openclaw_exports = if tmpl.name == "OpenClaw" {
                let openclaw_home = openclaw_home_dir(container_name);
                let openclaw_state = openclaw_state_dir(container_name);
                vm.shell_run(&format!(
                    "mkdir -p {} {}",
                    openclaw_home,
                    openclaw_state,
                )).await?;
                format!("{} ", openclaw_shell_exports(container_name))
            } else {
                String::new()
            };

            // Network pre-flight: verify VM can reach the internet before running install_cmd.
            // DNS and outbound networking may need a short warm-up right after first boot.
            verify_vm_internet_connectivity(state, vm, agent_id, 5, 5).await?;

            tracing::info!(cmd = %install_cmd, "Installing native agent in VM");
            match vm.shell_run(install_cmd).await {
                Ok(_) => {
                    tracing::info!("Native agent install completed successfully");
                    ensure_provisioning_active(state, vm, agent_id).await?;
                }
                Err(e) => {
                    tracing::warn!(container_name, error = %e, "Native agent auto-install failed, manual install required");
                    tracing::warn!(
                        "请在终端中手动安装:\n  \
                         1. 运行: limactl shell {vm_name}\n  \
                         2. 执行安装命令: {install_cmd}\n  \
                         3. 安装完成后在界面点击 [启动] 按钮",
                        vm_name = vm.name(),
                    );
                    return Ok(ProvisionResult {
                        needs_manual_install: true,
                    });
                }
            }

            // Start as background process and always create log file first.
            // We also inject common user bin paths to avoid relying solely on shell profiles.
            vm.shell_run(&format!(": > {log_path}")).await?;

            // For OpenClaw: generate a gateway token, save to DB, and write to ~/.openclaw/.env
            // so the gateway uses a known token we can pass when opening the browser.
            let mut extra_env = String::new();
            if tmpl.name == "OpenClaw" {
                let token = uuid::Uuid::new_v4().to_string().replace('-', "");
                // Save token to agent_configs in DB
                sqlx::query(
                    "INSERT INTO agent_configs (agent_id, config_key, config_value, is_secret, updated_at) \
                     VALUES (?, 'gateway_token', ?, 1, datetime('now')) \
                     ON CONFLICT(agent_id, config_key) DO UPDATE SET config_value = excluded.config_value, updated_at = datetime('now')"
                )
                .bind(&agent_id)
                .bind(&token)
                .execute(&*db)
                .await
                .ok();
                // Write token to instance-specific OpenClaw state dir
                let openclaw_state = openclaw_state_dir(container_name);
                let write_env = format!(
                    "mkdir -p {state_dir} && printf '%s\\n' 'OPENCLAW_GATEWAY_TOKEN={token}' > {state_dir}/.env",
                    state_dir = openclaw_state,
                );
                let _ = vm.shell_run(&write_env).await;
                extra_env = format!("OPENCLAW_GATEWAY_TOKEN={token}");
                tracing::info!("Generated OpenClaw gateway token and wrote to instance OpenClaw state dir");
            }
            ensure_provisioning_active(state, vm, agent_id).await?;

            let run_cmd = if extra_env.is_empty() {
                format!(
                    "bash -c 'export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports}export AGENTBOX_PORT={host_port}; nohup {start_cmd} >> {log_path} 2>&1 & pid=$!; printf \"%s\" \"$pid\" > {pid_path}; echo $pid'"
                )
            } else {
                format!(
                    "bash -c 'export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports}export AGENTBOX_PORT={host_port}; export {extra_env}; nohup {start_cmd} >> {log_path} 2>&1 & pid=$!; printf \"%s\" \"$pid\" > {pid_path}; echo $pid'"
                )
            };
            tracing::info!(cmd = %run_cmd, "Starting native agent");
            let pid = vm.shell_run(&run_cmd).await?;
            let pid = pid.trim().to_string();
            tracing::info!(container_name, pid = %pid, "Native agent started");
            ensure_provisioning_active(state, vm, agent_id).await?;

            // Brief pause + verify the process is still alive
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if !pid.is_empty() {
                let alive = vm.shell_run(&format!("kill -0 {pid} 2>/dev/null && echo ALIVE || echo DEAD")).await.unwrap_or_default();
                if alive.contains("DEAD") {
                    let logs = vm.shell_run(&format!("tail -20 {log_path} 2>/dev/null || echo '(no log)'")).await.unwrap_or_default();
                    tracing::error!(container_name, pid, "Native agent process died immediately. Last logs:\n{}", logs);
                    anyhow::bail!("Agent 进程启动后立即退出，请检查日志: {log_path}");
                }
            }

            // Lima's built-in port forwarding (lima-guestagent) will automatically
            // forward the port once the agent starts listening. No manual SSH tunnel needed.
            tracing::info!(host_port, "Agent started — Lima will auto-forward the port when ready");

            // Startup health probe: verify the agent becomes reachable.
            // This catches cases where the process starts but fails to bind the port.
            let health_url = &tmpl.health.url.replace(
                &format!(":{}", tmpl.ports.first().map(|p| p.container).unwrap_or(0)),
                &format!(":{host_port}"),
            );
            if let Err(e) = startup_health_probe(health_url, 60, 3).await {
                let logs = vm.shell_run(&format!("tail -30 {log_path} 2>/dev/null || echo '(no log)'")).await.unwrap_or_default();
                tracing::warn!(container_name, error = %e, "Startup health probe failed. Last logs:\n{}", logs);
                // Don't fail hard — the background health checker will continue monitoring.
                // Log the warning so users can investigate.
            }

            ensure_provisioning_active(state, vm, agent_id).await?;

            return Ok(ProvisionResult {
                needs_manual_install: false,
            });
        }

        other => {
            anyhow::bail!("Unknown install_method: {other}");
        }
    }

    Ok(ProvisionResult { needs_manual_install: false })
}

/// Probe a health URL repeatedly until it returns 2xx or the timeout expires.
/// `max_attempts` × `interval_secs` = effective timeout.
/// Returns Ok(()) on first successful probe, Err on timeout.
async fn startup_health_probe(url: &str, max_attempts: u32, interval_secs: u64) -> Result<(), anyhow::Error> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    for attempt in 1..=max_attempts {
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!(url, attempt, "Startup health probe succeeded");
                return Ok(());
            }
            Ok(resp) => {
                tracing::debug!(url, attempt, status = %resp.status(), "Startup probe: not ready yet");
            }
            Err(e) => {
                tracing::debug!(url, attempt, error = %e, "Startup probe: not reachable yet");
            }
        }
    }

    anyhow::bail!("Agent 启动后未能通过健康检查 ({url})，已尝试 {max_attempts} 次")
}

// pull_image is now handled by ContainerRuntime::pull()

/// Run docker compose up for a template, routed through VM.
async fn run_compose(
    docker: &agentbox_docker::ContainerRuntime,
    compose_path: &str,
    project_name: &str,
    host_port: u16,
    data_dir: &std::path::Path,
) -> Result<(), anyhow::Error> {
    let (ok, _, stderr) = docker.compose(
        &["-f", compose_path, "-p", project_name, "up", "-d"],
        &[
            ("AGENTBOX_PORT_18790", &host_port.to_string()),
            ("AGENTBOX_DATA_DIR", &data_dir.to_string_lossy()),
        ],
    ).await?;

    if !ok {
        anyhow::bail!("docker compose up failed: {stderr}");
    }
    Ok(())
}

/// Get agent data directory: ~/.agentbox/agents/<container_name>/
fn agent_data_dir(container_name: &str) -> Result<std::path::PathBuf, anyhow::Error> {
    let home = dirs_next::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let dir = home.join(".agentbox").join("agents").join(container_name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Start a stopped agent.
#[tauri::command]
pub async fn start_agent(id: String, state: State<'_, AppState>) -> Result<(), String> {
    // Verify VM/Docker is ready
    check_vm_ready(&state)?;

    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(STATUS_STARTING)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    let start_result: Result<(), String> = async {
        let vm = vm_for_agent(&state, &agent);
        vm.ensure_ready(None).await.map_err(|e| e.to_string())?;

        match agent.install_method.as_str() {
        "compose" => {
            let tmpl = template::load_template(&agent.template).map_err(|e| e.to_string())?;
            let compose_file = tmpl.runtime.compose_file.as_deref().unwrap_or("docker-compose.yml");
            let template_dir = template::templates_dir().join(&agent.template);
            let compose_path = template_dir.join(compose_file);

            let docker = docker_for_agent(&state, &agent);
            let (ok, _, stderr) = docker.compose(
                &["-f", &compose_path.to_string_lossy(), "-p", &agent.container_name, "start"],
                &[],
            ).await.map_err(|e| e.to_string())?;

            if !ok {
                return Err(format!("docker compose start failed: {stderr}"));
            }
        }
        "native" => {
            let vm = vm_for_agent(&state, &agent);
            let tmpl = template::load_template(&agent.template).map_err(|e| e.to_string())?;
            let start_cmd = tmpl.runtime.start_cmd.as_deref()
                .ok_or_else(|| "Native template missing runtime.start_cmd".to_string())?;
            let log_path = native_log_path(&agent.container_name);
            let pid_path = native_pid_path(&agent.container_name);
            let _ = vm.shell_run(&format!("mkdir -p {} {}", native_log_dir(), native_pid_dir())).await;
            vm.shell_run(&format!(": > {log_path}")).await.map_err(|e| e.to_string())?;

            // For openclaw: inject gateway token env var
            let mut extra_env = String::new();
            let mut openclaw_exports = String::new();
            if agent.template == "openclaw" {
                let openclaw_home = openclaw_home_dir(&agent.container_name);
                let openclaw_state = openclaw_state_dir(&agent.container_name);
                let _ = vm.shell_run(&format!(
                    "mkdir -p {} {}",
                    openclaw_home,
                    openclaw_state,
                )).await;
                openclaw_exports = format!("{} ", openclaw_shell_exports(&agent.container_name));
                let gw_token: Option<String> = sqlx::query_scalar::<_, String>(
                    "SELECT config_value FROM agent_configs WHERE agent_id = ? AND config_key = 'gateway_token'"
                )
                .bind(&id)
                .fetch_optional(&state.db)
                .await
                .unwrap_or(None);
                if let Some(token) = gw_token {
                    extra_env = format!("export OPENCLAW_GATEWAY_TOKEN={}; ", token);
                }
            }

            let run_cmd = format!(
                "bash -c 'export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports}export AGENTBOX_PORT={}; {extra_env}nohup {start_cmd} >> {log_path} 2>&1 & pid=$!; printf \"%s\" \"$pid\" > {pid_path}; echo $pid'",
                agent.port
            );
            vm.shell_run(&run_cmd).await.map_err(|e| e.to_string())?;
            tracing::info!(port = agent.port, "Native agent started — Lima auto-forwards port when ready");
        }
        _ => {
            let docker = docker_for_agent(&state, &agent);
            docker.start(&agent.container_name).await.map_err(|e| e.to_string())?;
        }
    }

        Ok(())
    }
    .await;

    match start_result {
        Ok(()) => {
            sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(STATUS_RUNNING)
                .bind(&id)
                .execute(&state.db)
                .await
                .map_err(|e| e.to_string())?;

            Ok(())
        }
        Err(e) => {
            let _ = sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(STATUS_START_FAILED)
                .bind(&id)
                .execute(&state.db)
                .await;

            Err(e)
        }
    }
}

/// Stop a running agent.
#[tauri::command]
pub async fn stop_agent(id: String, state: State<'_, AppState>) -> Result<(), String> {
    check_vm_ready(&state)?;

    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    match agent.install_method.as_str() {
        "compose" => {
            let tmpl = template::load_template(&agent.template).map_err(|e| e.to_string())?;
            let compose_file = tmpl.runtime.compose_file.as_deref().unwrap_or("docker-compose.yml");
            let template_dir = template::templates_dir().join(&agent.template);
            let compose_path = template_dir.join(compose_file);

            let docker = docker_for_agent(&state, &agent);
            let (ok, _, stderr) = docker.compose(
                &["-f", &compose_path.to_string_lossy(), "-p", &agent.container_name, "stop"],
                &[],
            ).await.map_err(|e| e.to_string())?;

            if !ok {
                return Err(format!("docker compose stop failed: {stderr}"));
            }
        }
        "native" => {
            let tmpl = template::load_template(&agent.template).map_err(|e| e.to_string())?;
            stop_native_agent_process(&state, &agent, tmpl.runtime.start_cmd.as_deref()).await;
        }
        _ => {
            let docker = docker_for_agent(&state, &agent);
            docker.stop(&agent.container_name).await.map_err(|e| e.to_string())?;
        }
    }

    sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(STATUS_PENDING)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Delete an agent and its container.
#[tauri::command]
pub async fn delete_agent(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let Some(agent): Option<AgentInfo> = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| e.to_string())?
    else {
        // Already deleted (or never existed) — treat as success for idempotency.
        return Ok(());
    };

    if agent.status == STATUS_CREATING {
        state.request_provisioning_cancel(&id).await;
    }

    let vm = vm_for_agent(&state, &agent);

    // Best-effort VM/container/process cleanup — skip if runtime not ready.
    // On macOS this ultimately maps to `limactl delete --force <vm_name>`.
    let vm_ready = state.vm_ready.load(std::sync::atomic::Ordering::Acquire);

    if vm_ready {
        let cleanup = async {
            if vm.docker_cmd_prefix().is_empty() {
                match agent.install_method.as_str() {
                    "compose" => {
                        let docker = docker_for_agent(&state, &agent);
                        if let Ok(tmpl) = template::load_template(&agent.template) {
                            let compose_file = tmpl.runtime.compose_file.as_deref().unwrap_or("docker-compose.yml");
                            let template_dir = template::templates_dir().join(&agent.template);
                            let compose_path = template_dir.join(compose_file);
                            let _ = docker.compose(
                                &["-f", &compose_path.to_string_lossy(), "-p", &agent.container_name, "down", "-v"],
                                &[],
                            ).await;
                        }
                    }
                    "native" => {
                        if let Ok(tmpl) = template::load_template(&agent.template) {
                            stop_native_agent_process(&state, &agent, tmpl.runtime.start_cmd.as_deref()).await;
                        }
                        let log_path = native_log_path(&agent.container_name);
                        let pid_path = native_pid_path(&agent.container_name);
                        let _ = vm.shell_run(&format!("rm -f {log_path} {pid_path}")).await;
                    }
                    _ => {
                        let docker = docker_for_agent(&state, &agent);
                        let _ = docker.remove(&agent.container_name).await;
                    }
                }
            } else {
                let _ = vm.delete().await;
            }
        };
        if let Err(e) = tokio::time::timeout(std::time::Duration::from_secs(10), cleanup).await {
            tracing::warn!(agent_id = %id, error = ?e, "Delete cleanup timed out, continue removing DB record");
        }
    }

    // Remove DB record (cascades to agent_configs and agent_metrics)
    sqlx::query("DELETE FROM agents WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Save config entries for an agent and optionally restart.
#[tauri::command]
pub async fn save_agent_config(
    id: String,
    configs: Vec<AgentConfigEntry>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    for entry in &configs {
        let config_value = if entry.is_secret && is_secret_mask_placeholder(&entry.config_value) {
            let existing = sqlx::query_scalar::<_, String>(
                "SELECT config_value FROM agent_configs WHERE agent_id = ? AND config_key = ?"
            )
            .bind(&id)
            .bind(&entry.config_key)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default();

            if is_secret_mask_placeholder(&existing) {
                String::new()
            } else {
                existing
            }
        } else {
            entry.config_value.clone()
        };

        sqlx::query(
            "INSERT INTO agent_configs (agent_id, config_key, config_value, is_secret, updated_at)
             VALUES (?, ?, ?, ?, datetime('now'))
             ON CONFLICT(agent_id, config_key)
             DO UPDATE SET config_value = excluded.config_value, updated_at = datetime('now')"
        )
        .bind(&id)
        .bind(&entry.config_key)
        .bind(config_value)
        .bind(entry.is_secret)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Apply saved config to a running agent's container as env vars, then restart.
#[tauri::command]
pub async fn apply_agent_config(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    check_vm_ready(&state)?;

    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let vm = vm_for_agent(&state, &agent);
    vm.ensure_ready(None).await.map_err(|e| e.to_string())?;

    // Get real config values (not masked)
    let configs: Vec<AgentConfigEntry> = sqlx::query_as(
        "SELECT config_key, config_value, is_secret FROM agent_configs WHERE agent_id = ?"
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // Load template to know env_name mappings
    let tmpl = template::load_template(&agent.template).map_err(|e| e.to_string())?;

    // Build env map from config
    let mut env_map: HashMap<String, String> = HashMap::new();
    // Collect raw config values by key for template-specific logic
    let config_map: HashMap<String, String> = configs
        .iter()
        .map(|c| (c.config_key.clone(), c.config_value.clone()))
        .collect();

    for conf in &configs {
        // Find the config_schema entry that maps this key to an env var
        if let Some(schema_field) = tmpl.config_schema.iter().find(|f| f.key == conf.config_key) {
            if let Some(env_name) = &schema_field.env_name {
                env_map.insert(env_name.clone(), conf.config_value.clone());
            }
        }
        // Also inject as AGENTBOX_CONFIG_<KEY>
        env_map.insert(
            format!("AGENTBOX_CONFIG_{}", conf.config_key.to_uppercase()),
            conf.config_value.clone(),
        );
    }

    // OpenClaw-specific: dynamically map api_key to correct provider env var
    // and run onboarding to generate openclaw.json config file.
    if agent.template == "openclaw" {
        let provider = config_map.get("llm_provider").map(|s| s.as_str()).unwrap_or("anthropic");
        let api_key = config_map.get("api_key").cloned().unwrap_or_default();
        let model = config_map.get("model").cloned().unwrap_or_default();
        let normalized_model = if provider == "qwen" {
            if model.is_empty() {
                "qwen-api/qwen-plus".to_string()
            } else if let Some(rest) = model.strip_prefix("qwen/") {
                format!("qwen-api/{rest}")
            } else if let Some(rest) = model.strip_prefix("qwen-portal/") {
                format!("qwen-api/{rest}")
            } else if model.contains('/') {
                model.clone()
            } else {
                format!("qwen-api/{model}")
            }
        } else {
            model.clone()
        };

        let desired_model = if normalized_model.is_empty() {
            match provider {
                "openai" => "openai/gpt-4o".to_string(),
                "deepseek" => "deepseek/deepseek-chat".to_string(),
                "ollama" => "ollama/llama3".to_string(),
                "openrouter" => "openrouter/anthropic/claude-sonnet-4-20250514".to_string(),
                "mistral" => "mistral/mistral-large-latest".to_string(),
                "moonshot" => "moonshot/moonshot-v1-8k".to_string(),
                "qwen" => "qwen-api/qwen-plus".to_string(),
                _ => "anthropic/claude-sonnet-4-20250514".to_string(),
            }
        } else {
            normalized_model.clone()
        };
        let is_custom_provider = matches!(provider, "deepseek" | "qwen" | "ollama");

        // Map provider to correct env var name
        let env_var_name = match provider {
            "openai" => "OPENAI_API_KEY",
            "deepseek" => "DEEPSEEK_API_KEY",
            "openrouter" => "OPENROUTER_API_KEY",
            "mistral" => "MISTRAL_API_KEY",
            "moonshot" => "MOONSHOT_API_KEY",
            "qwen" => "QWEN_API_KEY",
            _ => "ANTHROPIC_API_KEY", // anthropic + fallback
        };

        if !api_key.is_empty() {
            env_map.insert(env_var_name.to_string(), api_key.clone());
        }

        // Map provider to onboard --auth-choice
        let auth_choice = match provider {
            "openai" => "openai-api-key",
            "openrouter" => "openrouter-api-key",
            "mistral" => "mistral-api-key",
            "moonshot" => "moonshot-api-key",
            "deepseek" | "qwen" | "ollama" => "custom-api-key",
            _ => "apiKey", // anthropic + fallback
        };

        if tmpl.install_method == "native" && !api_key.is_empty() {
            let provider_id = openclaw_provider_id(provider);
            let openclaw_state = openclaw_state_dir(&agent.container_name);
            let openclaw_home = openclaw_home_dir(&agent.container_name);
            let openclaw_exports = openclaw_shell_exports(&agent.container_name);
            vm.shell_run(&format!(
                "mkdir -p {} {}",
                openclaw_home,
                openclaw_state,
            )).await.map_err(|e| e.to_string())?;

            upsert_openclaw_auth_profile(&state, &agent, provider_id, &api_key).await?;

            // Write instance-specific OpenClaw .env with the API key + gateway token
            let mut dotenv_content = format!("{}={}", env_var_name, api_key);
            let mut gateway_token_export = String::new();
            let mut custom_api_key_export = String::new();

            if is_custom_provider {
                dotenv_content.push_str(&format!("\nCUSTOM_API_KEY={}", api_key));
                custom_api_key_export = format!(
                    " export CUSTOM_API_KEY='{}';",
                    api_key.replace('\'', "'\\''")
                );
            }

            // Read gateway_token from DB and include it
            let gw_token: Option<String> = sqlx::query_scalar::<_, String>(
                "SELECT config_value FROM agent_configs WHERE agent_id = ? AND config_key = 'gateway_token'"
            )
            .bind(&id)
            .fetch_optional(&state.db)
            .await
            .unwrap_or(None);
            if let Some(ref token) = gw_token {
                dotenv_content.push_str(&format!("\nOPENCLAW_GATEWAY_TOKEN={}", token));
                env_map.insert("OPENCLAW_GATEWAY_TOKEN".to_string(), token.clone());
                gateway_token_export = format!(
                    " export OPENCLAW_GATEWAY_TOKEN='{}';",
                    token.replace('\'', "'\\''")
                );
            }

            let write_env_cmd = format!(
                "mkdir -p {state_dir} && printf '%s\\n' '{}' > {state_dir}/.env",
                dotenv_content.replace('\'', "'\\''")
                ,state_dir = openclaw_state
            );
            let _ = vm.shell_run(&write_env_cmd).await;
            tracing::info!("Wrote instance OpenClaw .env with {}", env_var_name);

            // Run openclaw onboard to generate openclaw.json
            let mut onboard_cmd = format!(
                "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; \
                 {openclaw_exports} \
                 export {env_var_name}='{api_key_escaped}'; \
                 {custom_api_key_export} \
                 {gateway_token_export} \
                 openclaw onboard --non-interactive --mode local \
                 --auth-choice {auth_choice} \
                 --gateway-port {port} --gateway-bind loopback \
                 --skip-skills --secret-input-mode ref --accept-risk",
                env_var_name = env_var_name,
                api_key_escaped = api_key.replace('\'', "'\\''"),
                custom_api_key_export = custom_api_key_export,
                gateway_token_export = gateway_token_export,
                auth_choice = auth_choice,
                port = agent.port,
            );

            // For custom providers, add base URL and model flags
            match provider {
                "deepseek" => {
                    let base = config_map.get("base_url").filter(|u| !u.is_empty())
                        .map(|u| u.clone())
                        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
                    let m = if model.is_empty() { "deepseek-chat".to_string() } else {
                        model.split('/').last().unwrap_or(&model).to_string()
                    };
                    onboard_cmd.push_str(&format!(
                        " --custom-base-url '{}' --custom-model-id '{}' --custom-provider-id 'deepseek' --custom-compatibility openai --custom-api-key '{}'",
                        base, m, api_key.replace('\'', "'\\''")
                    ));
                }
                "openrouter" => {
                    onboard_cmd.push_str(&format!(
                        " --openrouter-api-key '{}'",
                        api_key.replace('\'', "'\\''")
                    ));
                }
                "mistral" => {
                    onboard_cmd.push_str(&format!(
                        " --mistral-api-key '{}'",
                        api_key.replace('\'', "'\\''")
                    ));
                }
                "moonshot" => {
                    onboard_cmd.push_str(&format!(
                        " --moonshot-api-key '{}'",
                        api_key.replace('\'', "'\\''")
                    ));
                }
                "qwen" => {
                    let base = "https://dashscope.aliyuncs.com/compatible-mode/v1";
                    let m = if model.is_empty() { "qwen-plus".to_string() } else {
                        normalized_model.split('/').last().unwrap_or(&normalized_model).to_string()
                    };
                    onboard_cmd.push_str(&format!(
                        " --custom-base-url '{}' --custom-model-id '{}' --custom-provider-id 'qwen-api' --custom-compatibility openai --custom-api-key '{}'",
                        base, m, api_key.replace('\'', "'\\''")
                    ));
                }
                "ollama" => {
                    let base = config_map.get("base_url").filter(|u| !u.is_empty())
                        .map(|u| u.clone())
                        .unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string());
                    let m = if model.is_empty() { "llama3".to_string() } else {
                        model.split('/').last().unwrap_or(&model).to_string()
                    };
                    onboard_cmd.push_str(&format!(
                        " --custom-base-url '{}' --custom-model-id '{}' --custom-provider-id 'ollama' --custom-compatibility openai --custom-api-key 'ollama'",
                        base, m
                    ));
                }
                "openai" => {
                    onboard_cmd.push_str(&format!(
                        " --openai-api-key '{}'",
                        api_key.replace('\'', "'\\''")
                    ));
                }
                _ => {
                    // anthropic — key flag
                    onboard_cmd.push_str(&format!(
                        " --anthropic-api-key '{}'",
                        api_key.replace('\'', "'\\''")
                    ));
                }
            }

            let full_onboard = format!("bash -c '{}'", onboard_cmd.replace('\'', "'\\''"));
            tracing::info!("Running OpenClaw onboard for provider={}", provider);
            match vm.shell_run(&full_onboard).await {
                Ok(output) => tracing::info!("OpenClaw onboard completed: {}", output.chars().take(200).collect::<String>()),
                Err(e) => tracing::warn!("OpenClaw onboard failed (non-fatal): {}", e),
            }

            let set_model_cmd = format!(
                "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports} openclaw models set '{}' 2>/dev/null || true",
                desired_model.replace('\'', "'\\''")
            );
            if let Ok(output) = vm.shell_run(&set_model_cmd).await {
                tracing::info!("OpenClaw models set completed: {}", output.chars().take(200).collect::<String>());
            }
        }
    }

    if tmpl.install_method == "native" {
        // For native agents: stop process, restart with env vars
        if let Some(start_cmd) = tmpl.runtime.start_cmd.as_deref() {
            stop_native_agent_process(&state, &agent, Some(start_cmd)).await;

            // Build env prefix
            let env_str: String = env_map.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ");

            let log_path = native_log_path(&agent.container_name);
            let pid_path = native_pid_path(&agent.container_name);
            let _ = vm.shell_run(&format!("mkdir -p {} {}", native_log_dir(), native_pid_dir())).await;
            vm.shell_run(&format!(": > {log_path}")).await.map_err(|e| e.to_string())?;
            let openclaw_exports = if agent.template == "openclaw" {
                format!("{} ", openclaw_shell_exports(&agent.container_name))
            } else {
                String::new()
            };
            let run_cmd = format!(
                "bash -c 'export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports}export AGENTBOX_PORT={}; nohup env {env_str} {start_cmd} >> {log_path} 2>&1 & pid=$!; printf \"%s\" \"$pid\" > {pid_path}; echo $pid'",
                agent.port
            );
            vm.shell_run(&run_cmd).await.map_err(|e| e.to_string())?;
            tracing::info!(port = agent.port, "Native agent restarted with new config");
        }
    } else {
        // Stop, remove, and recreate container with new env vars
        let docker = docker_for_agent(&state, &agent);
        if agent.status == STATUS_RUNNING {
            let _ = docker.stop(&agent.container_name).await;
        }
        let _ = docker.remove(&agent.container_name).await;

        // Recreate with env vars
        let data_dir = agent_data_dir(&agent.container_name).map_err(|e| e.to_string())?;
        let mut ports = HashMap::new();
        if let Some(first_port) = tmpl.ports.first() {
            ports.insert(agent.port, first_port.container);
        }

        let mut extra_args = crate::network::NetworkPolicy::default().docker_args();
        for vol in &tmpl.volumes {
            let host_path = data_dir.join(&vol.host_suffix);
            std::fs::create_dir_all(&host_path).map_err(|e| e.to_string())?;
            extra_args.push("-v".to_string());
            extra_args.push(format!("{}:{}", host_path.display(), vol.container));
        }

        let image = match tmpl.install_method.as_str() {
            "docker" => tmpl.runtime.image.clone().unwrap_or_default(),
            _ => format!("agentbox/{}", agent.container_name),
        };

        let config = ContainerConfig {
            image,
            name: agent.container_name.clone(),
            ports,
            env: env_map,
            cpus: Some(tmpl.resources.cpus as f64),
            memory_mb: Some(tmpl.resources.memory_mb),
        };

        docker.create_with_args(&config, &extra_args).await.map_err(|e| e.to_string())?;
    }

    sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(STATUS_RUNNING)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Toggle auto-start flag for an agent.
#[tauri::command]
pub async fn set_auto_start(
    id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("UPDATE agents SET auto_start = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(enabled)
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Get recent logs from an agent container.
#[tauri::command]
pub async fn get_agent_logs(
    id: String,
    tail: Option<u32>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    if agent.install_method == "native" {
        // Read log file from VM
        let n = tail.unwrap_or(100);
        let log_path = native_log_path(&agent.container_name);
        let vm = vm_for_agent(&state, &agent);
        vm.shell_run(&format!("tail -n {n} {log_path} 2>/dev/null || echo '(暂无日志)'"))
            .await
            .map(|logs| sanitize_log_for_display(&logs))
            .map_err(|e| e.to_string())
    } else {
        let docker = docker_for_agent(&state, &agent);
        docker.logs(&agent.container_name, tail.unwrap_or(100))
            .await
            .map(|logs| sanitize_log_for_display(&logs))
            .map_err(|e| e.to_string())
    }
}

/// Open an interactive shell (terminal window) into an agent container.
#[tauri::command]
pub async fn open_agent_shell(
    id: String,
    shell: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    check_vm_ready(&state)?;

    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    if agent.install_method == "native" {
        // Native agents run inside the VM — open a local terminal connected to VM.
        let vm = vm_for_agent(&state, &agent);
        if agent.template == "openclaw" {
            if let Some(ssh_info) = vm.ssh_info().map_err(|e| e.to_string())? {
                #[cfg(target_os = "macos")]
                {
                    let remote_cmd = format!(
                        "ssh -F '{}' -t {} \"export OPENCLAW_HOME=\\\"{}\\\"; exec bash -l\"",
                        ssh_info.config_file,
                        ssh_info.host,
                        openclaw_home_dir(&agent.container_name),
                    );
                    std::process::Command::new("osascript")
                        .args([
                            "-e",
                            &format!(
                                "tell application \"Terminal\" to do script \"{}\"",
                                remote_cmd.replace('\\', "\\\\").replace('"', "\\\"")
                            ),
                        ])
                        .spawn()
                        .map_err(|e| e.to_string())?;
                    return Ok(());
                }
            }
        }
        vm.open_vm_shell().map_err(|e| e.to_string())?;
        Ok(())
    } else {
        let docker = docker_for_agent(&state, &agent);
        docker.open_shell(&agent.container_name, shell.as_deref())
            .map_err(|e| e.to_string())
    }
}

/// Execute one command for web shell view and return combined output.
#[tauri::command]
pub async fn run_agent_shell_command(
    id: String,
    command: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    check_vm_ready(&state)?;

    let cmd = command.trim();
    if cmd.is_empty() {
        return Ok(String::new());
    }

    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let output = if agent.install_method == "native" {
        let vm = vm_for_agent(&state, &agent);
        // Ensure common user bins are available for native web shell commands.
        let openclaw_exports = if agent.template == "openclaw" {
            format!("{} ", openclaw_shell_exports(&agent.container_name))
        } else {
            String::new()
        };
        let full_cmd = format!(
            "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports}{cmd}"
        );
        vm.shell_run(&full_cmd).await.map_err(|e| e.to_string())?
    } else {
        let docker = docker_for_agent(&state, &agent);
        docker.exec_capture(&agent.container_name, cmd, shell_default_for_agent(&agent))
            .await
            .map_err(|e| e.to_string())?
    };

    Ok(sanitize_log_for_display(&output))
}

/// Open the agent's web UI in the host's default browser.
#[tauri::command]
pub async fn open_agent_browser(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let mut url = format!("http://localhost:{}", agent.port);

    // For OpenClaw: ask the official CLI for the current dashboard URL.
    // This returns the URL format expected by the Control UI, including the
    // current auth token when needed.
    if agent.template == "openclaw" {
        let vm = vm_for_agent(&state, &agent);
        let openclaw_exports = openclaw_shell_exports(&agent.container_name);
        let gateway_token = resolve_openclaw_gateway_token(&state, &agent).await;
        let dashboard_output = vm
            .shell_run(
                &format!(
                    "export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports} openclaw dashboard --no-open 2>/dev/null | grep -Eo 'https?://[^[:space:]]+' | tail -n 1 || true"
                ),
            )
            .await
            .ok();

        if let Some(output) = dashboard_output {
            let parsed_url = output.trim();
            if parsed_url.starts_with("http://") || parsed_url.starts_with("https://") {
                url = parsed_url.to_string();
            }
        }

        if let Some(token) = gateway_token {
            url = append_gateway_token_to_url(&url, &token);
        }
    }

    open::that(&url).map_err(|e| e.to_string())?;
    Ok(())
}

/// SSH connection info returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConnectionInfo {
    /// SSH host alias (e.g. "lima-agentbox")
    pub host: String,
    /// SSH config file path
    pub config_file: String,
    /// Short SSH command (e.g. "ssh lima-agentbox")
    pub command: String,
    /// SSH command with explicit config file
    pub command_with_config: String,
}

/// Get SSH connection info for native agents running in a VM.
/// Returns None for Docker-based agents.
#[tauri::command]
pub async fn get_ssh_info(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<SshConnectionInfo>, String> {
    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    if agent.install_method != "native" {
        return Ok(None);
    }

    let vm = vm_for_agent(&state, &agent);
    vm.ensure_ssh_config().map_err(|e| e.to_string())?;

    let info = vm.ssh_info().map_err(|e| e.to_string())?;
    Ok(info.map(|i| SshConnectionInfo {
        host: i.host,
        config_file: i.config_file,
        command: i.command,
        command_with_config: i.command_with_config,
    }))
}

// ---------------------------------------------------------------------------
// Internal helpers (not exposed as Tauri commands)
// ---------------------------------------------------------------------------

/// Auto-start all agents that have `auto_start = true`.
pub async fn autostart_agents(state: &AppState) -> Result<(), String> {
    let agents: Vec<AgentInfo> = sqlx::query_as(
        "SELECT * FROM agents WHERE auto_start = 1 AND status != 'RUNNING'"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    for agent in agents {
        tracing::info!(id = %agent.id, name = %agent.name, "Auto-starting agent");
        sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(STATUS_STARTING)
            .bind(&agent.id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;

        let vm = vm_for_agent(state, &agent);
        if let Err(e) = vm.ensure_ready(None).await {
            tracing::error!(id = %agent.id, error = %e, "Failed to prepare agent VM for auto-start");
            sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(STATUS_START_FAILED)
                .bind(&agent.id)
                .execute(&state.db)
                .await
                .map_err(|db_err| db_err.to_string())?;
            continue;
        }

        let result = match agent.install_method.as_str() {
            "compose" => {
                let tmpl = template::load_template(&agent.template);
                match tmpl {
                    Ok(t) => {
                        let compose_file = t.runtime.compose_file.as_deref().unwrap_or("docker-compose.yml");
                        let template_dir = template::templates_dir().join(&agent.template);
                        let compose_path = template_dir.join(compose_file);
                        let docker = docker_for_agent(state, &agent);
                        match docker.compose(
                            &["-f", &compose_path.to_string_lossy(), "-p", &agent.container_name, "start"],
                            &[],
                        ).await {
                            Ok((true, _, _)) => Ok(()),
                            Ok((false, _, stderr)) => Err(anyhow::anyhow!("{stderr}")),
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            "native" => {
                match template::load_template(&agent.template) {
                    Ok(tmpl) => {
                        if let Some(start_cmd) = tmpl.runtime.start_cmd.as_deref() {
                            let log_path = native_log_path(&agent.container_name);
                            let pid_path = native_pid_path(&agent.container_name);
                            let _ = vm.shell_run(&format!("mkdir -p {} {}", native_log_dir(), native_pid_dir())).await;
                            let _ = vm.shell_run(&format!(": > {log_path}")).await;

                            // For openclaw: inject gateway token env var
                            let mut extra_env = String::new();
                            let mut openclaw_exports = String::new();
                            if agent.template == "openclaw" {
                                let openclaw_home = openclaw_home_dir(&agent.container_name);
                                let openclaw_state = openclaw_state_dir(&agent.container_name);
                                let _ = vm.shell_run(&format!(
                                    "mkdir -p {} {}",
                                    openclaw_home,
                                    openclaw_state,
                                )).await;
                                openclaw_exports = format!("{} ", openclaw_shell_exports(&agent.container_name));
                                let gw_token: Option<String> = sqlx::query_scalar(
                                    "SELECT config_value FROM agent_configs WHERE agent_id = ? AND config_key = 'gateway_token'"
                                )
                                .bind(&agent.id)
                                .fetch_optional(&state.db)
                                .await
                                .unwrap_or(None);
                                if let Some(token) = gw_token {
                                    extra_env = format!("export OPENCLAW_GATEWAY_TOKEN={}; ", token);
                                }
                            }

                            let run_cmd = format!(
                                "bash -c 'export PATH=\"$HOME/.npm-global/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH\"; {openclaw_exports}export AGENTBOX_PORT={}; {extra_env}nohup {start_cmd} >> {log_path} 2>&1 & pid=$!; printf \"%s\" \"$pid\" > {pid_path}; echo $pid'",
                                agent.port
                            );
                            match vm.shell_run(&run_cmd).await {
                                Ok(_) => {
                                    tracing::info!(port = agent.port, "Native agent auto-started — Lima auto-forwards port");
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            }
                        } else {
                            Err(anyhow::anyhow!("Native template missing start_cmd"))
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            _ => docker_for_agent(state, &agent).start(&agent.container_name).await,
        };

        match result {
            Ok(()) => {
                sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
                    .bind(STATUS_RUNNING)
                    .bind(&agent.id)
                    .execute(&state.db)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Err(e) => {
                tracing::error!(id = %agent.id, error = %e, "Failed to auto-start agent");
                sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
                    .bind(STATUS_START_FAILED)
                    .bind(&agent.id)
                    .execute(&state.db)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Export / Import / Upgrade
// ---------------------------------------------------------------------------

/// Backup info returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentBackup {
    pub id: i64,
    pub agent_id: String,
    pub backup_path: String,
    pub version: String,
    pub created_at: String,
}

/// Export an agent's data directory + config to a backup archive.
/// Returns the backup path.
#[tauri::command]
pub async fn export_agent_data(
    id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    export_agent_data_internal(&agent, &state).await
}

/// Import backup data into an existing agent.
#[tauri::command]
pub async fn import_agent_data(
    id: String,
    backup_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // Stop agent if running
    if agent.status == "RUNNING" {
        let _ = stop_agent_internal(&agent, &state).await;
    }

    restore_backup_into_agent_internal(&agent, &backup_path, &state, true).await
}

async fn restore_backup_into_agent_internal(
    agent: &AgentInfo,
    backup_path: &str,
    state: &AppState,
    restore_configs: bool,
) -> Result<(), String> {

    let temp_base_dir = if agent.install_method == "native" && agent.template == "openclaw" {
        backups_base_dir().map_err(|e| e.to_string())?
    } else {
        let data_dir = agent_data_dir(&agent.container_name).map_err(|e| e.to_string())?;
        data_dir.parent().unwrap_or(&data_dir).to_path_buf()
    };
    let temp_dir = temp_base_dir.join(format!("_import_tmp_{}", agent.container_name));
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let output = tokio::process::Command::new("tar")
        .args([
            "-xzf", &backup_path,
            "-C", &temp_dir.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&temp_dir);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("解压备份失败: {stderr}"));
    }

    // Find the extracted folder (first directory inside temp_dir)
    let extracted = std::fs::read_dir(&temp_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .find(|e| e.path().is_dir())
        .map(|e| e.path())
        .unwrap_or(temp_dir.clone());

    // Import agentbox-export.json if present — restore configs
    let meta_path = extracted.join("agentbox-export.json");
    if restore_configs && meta_path.exists() {
        let meta_content = std::fs::read_to_string(&meta_path).map_err(|e| e.to_string())?;
        if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&meta_content) {
            if let Some(configs) = meta.get("configs").and_then(|v| v.as_array()) {
                for conf in configs {
                    let key = conf.get("key").and_then(|v| v.as_str()).unwrap_or_default();
                    let value = conf.get("value").and_then(|v| v.as_str()).unwrap_or_default();
                    let is_secret = conf.get("is_secret").and_then(|v| v.as_bool()).unwrap_or(false);
                    if !key.is_empty() {
                        let _ = sqlx::query(
                            "INSERT INTO agent_configs (agent_id, config_key, config_value, is_secret, updated_at)
                             VALUES (?, ?, ?, ?, datetime('now'))
                             ON CONFLICT(agent_id, config_key)
                             DO UPDATE SET config_value = excluded.config_value, updated_at = datetime('now')"
                        )
                        .bind(&agent.id)
                        .bind(key)
                        .bind(value)
                        .bind(is_secret)
                        .execute(&state.db)
                        .await;
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&meta_path);
    }

    // Copy remaining data files
    if agent.install_method == "native" && agent.template == "openclaw" {
        let extracted_state_dir = extracted.join(".openclaw");
        if extracted_state_dir.exists() {
            let temp_archive = temp_dir.join("openclaw-import.tar.gz");
            let output = tokio::process::Command::new("tar")
                .args([
                    "-czf",
                    &temp_archive.to_string_lossy(),
                    "-C",
                    &extracted.to_string_lossy(),
                    ".openclaw",
                ])
                .output()
                .await
                .map_err(|e| e.to_string())?;

            if !output.status.success() {
                let _ = std::fs::remove_dir_all(&temp_dir);
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("打包 OpenClaw 导入数据失败: {stderr}"));
            }

            import_native_openclaw_data(&agent, &state, &temp_archive.to_string_lossy()).await?;
        }
    } else {
        let data_dir = agent_data_dir(&agent.container_name).map_err(|e| e.to_string())?;
        copy_dir_contents(&extracted, &data_dir).map_err(|e| e.to_string())?;
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(())
}

/// Upgrade an agent: export data → create new instance → import data → archive old.
#[tauri::command]
pub async fn upgrade_agent(
    id: String,
    state: State<'_, AppState>,
) -> Result<AgentInfo, String> {
    check_vm_ready(&state)?;

    // Reject if another agent is already being provisioned
    if state.provisioning_count.load(std::sync::atomic::Ordering::Acquire) > 0 {
        return Err("已有实例正在创建中，请等待完成后再升级".into());
    }

    let creating_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE status = 'CREATING'")
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    if creating_count > 0 {
        return Err("已有实例正在创建中，请等待完成后再升级".into());
    }

    let provisioning_guard = ProvisioningCounterGuard::new(state.provisioning_count.clone());

    let old_agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // 1. Export current data
    let backup_path = export_agent_data_internal(&old_agent, &state).await?;
    tracing::info!(agent_id = %id, backup = %backup_path, "Exported agent data for upgrade");

    // 2. Stop old agent
    if old_agent.status == STATUS_RUNNING {
        let _ = stop_agent_internal(&old_agent, &state).await;
    }

    // 3. Create new agent with same name and template
    let tmpl = template::load_template(&old_agent.template).map_err(|e| e.to_string())?;

    let new_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let instance_no: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(instance_no), 0) + 1 FROM agents WHERE template = ?"
    )
    .bind(&old_agent.template)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let base_port = tmpl.ports.first().map(|p| p.host).unwrap_or(3000);
    let port = find_available_port(&state.db, base_port).await?;
    let runtime_mode = runtime_prefix_from_vm_name(&old_agent.vm_name);
    let vm_name = vm_name_for_instance(&old_agent.template, instance_no, runtime_mode);
    let container_name = vm_name.clone();
    let health_url = tmpl.health.url.replace(
        &format!(":{}", tmpl.ports.first().map(|p| p.container).unwrap_or(0)),
        &format!(":{port}"),
    );

    let ret_version = tmpl.version.clone();
    let ret_install_method = tmpl.install_method.clone();

    sqlx::query(
        "INSERT INTO agents (id, name, template, instance_no, port, status, health_url,
                            version, install_method, container_name, vm_name, auto_start, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, 'CREATING', ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&new_id).bind(&old_agent.name).bind(&old_agent.template)
    .bind(instance_no).bind(port)
    .bind(&health_url)
    .bind(&tmpl.version)
    .bind(&tmpl.install_method)
    .bind(&container_name)
        .bind(&vm_name)
    .bind(old_agent.auto_start)
    .bind(&now).bind(&now)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    // 4. Provision new container and import data in background
    let db = state.db.clone();
    let new_agent_id = new_id.clone();
    let c_name = container_name.clone();
        let vm_name_clone = vm_name.clone();
    let old_agent_clone = old_agent.clone();
    let backup_path_clone = backup_path.clone();
    let new_health_url = health_url.clone();
    let app_state = state.inner().clone();
    let now_ret = now.clone();
    let ret_version_ret = ret_version.clone();
    let ret_install_method_ret = ret_install_method.clone();
    let prov_lock = state.provisioning_lock.clone();
    let prov_count = provisioning_guard.release_to_spawn();

    tauri::async_runtime::spawn(async move {
        let _guard = prov_lock.lock().await;
        let docker = app_state.docker_for_vm_name(&vm_name_clone);
        let vm = app_state.vm_for_name(&vm_name_clone);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30 * 60),
            provision_agent(&app_state, &docker, &vm, &tmpl, &c_name, port, &db, &new_agent_id),
        ).await;
        let result = match result {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("升级超时（超过30分钟），请删除后重试")),
        };
        let cancelled = provisioning_cancelled_or_deleted(&app_state, &new_agent_id).await;
        match result {
            Ok(prov) => {
                if cancelled {
                    let _ = vm.delete().await;
                } else {
                let _ = sqlx::query(
                    "INSERT INTO agent_configs (agent_id, config_key, config_value, is_secret, updated_at) \
                     SELECT ?, config_key, config_value, is_secret, datetime('now') FROM agent_configs WHERE agent_id = ?"
                )
                .bind(&new_agent_id)
                .bind(&old_agent_clone.id)
                .execute(&db)
                .await;

                let new_agent_for_import = AgentInfo {
                    id: new_agent_id.clone(),
                    name: old_agent_clone.name.clone(),
                    template: old_agent_clone.template.clone(),
                    instance_no,
                    port,
                    status: STATUS_CREATING.into(),
                    auto_start: old_agent_clone.auto_start,
                    health_url: Some(new_health_url.clone()),
                    created_at: now.clone(),
                    version: ret_version.clone(),
                    install_method: ret_install_method.clone(),
                    container_name: c_name.clone(),
                    vm_name: vm_name_clone.clone(),
                    runtime_mode: old_agent_clone.runtime_mode.clone(),
                    ubuntu_image: old_agent_clone.ubuntu_image.clone(),
                };

                if let Err(e) = restore_backup_into_agent_internal(&new_agent_for_import, &backup_path_clone, &app_state, false).await {
                    tracing::warn!(agent_id = %new_agent_id, error = %e, "Failed to import backup into upgraded agent");
                }

                let new_status = if prov.needs_manual_install { STATUS_CREATE_FAILED } else { STATUS_RUNNING };
                let _ = sqlx::query(
                    "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?"
                )
                .bind(new_status)
                .bind(&new_agent_id)
                .execute(&db)
                .await;
                }
            }
            Err(e) => {
                if cancelled {
                    let _ = vm.delete().await;
                    tracing::info!(agent_id = %new_agent_id, "Provisioning cancelled while upgrade create was in progress");
                } else {
                    tracing::error!(agent_id = %new_agent_id, error = %e, "Failed to provision upgrade");
                    let _ = sqlx::query(
                        "UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?"
                    )
                    .bind(STATUS_CREATE_FAILED)
                    .bind(&new_agent_id)
                    .execute(&db)
                    .await;
                }
            }
        }

        app_state.clear_provisioning_cancel(&new_agent_id).await;
        prov_count.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    });

    // 5. Mark old agent as archived
    sqlx::query(
        "UPDATE agents SET status = ?, name = name || ' (旧)', updated_at = datetime('now') WHERE id = ?"
    )
    .bind(STATUS_PENDING)
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(AgentInfo {
        id: new_id,
        name: old_agent.name,
        template: old_agent.template,
        instance_no,
        port,
        status: STATUS_CREATING.into(),
        auto_start: old_agent.auto_start,
        health_url: Some(health_url),
        created_at: now_ret,
        version: ret_version_ret,
        install_method: ret_install_method_ret,
        container_name,
        vm_name,
        runtime_mode: old_agent.runtime_mode,
        ubuntu_image: old_agent.ubuntu_image,
    })
}

/// List backups for an agent.
#[tauri::command]
pub async fn list_agent_backups(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AgentBackup>, String> {
    let backups: Vec<AgentBackup> = sqlx::query_as(
        "SELECT id, agent_id, backup_path, version, created_at FROM agent_backups WHERE agent_id = ? ORDER BY created_at DESC"
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(backups)
}

// ---------------------------------------------------------------------------
// Internal helpers for export/import
// ---------------------------------------------------------------------------

/// Export agent data without Tauri State wrapper (for internal use during upgrade).
async fn export_agent_data_internal(
    agent: &AgentInfo,
    state: &AppState,
) -> Result<String, String> {
    let backups_dir = backups_base_dir().map_err(|e| e.to_string())?;
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let archive_name = format!("{}-{ts}.tar.gz", agent.container_name);
    let archive_path = backups_dir.join(&archive_name);
    let staging_root = backups_dir.join(format!("_export_{}_{}", agent.container_name, ts));
    let staging_dir = staging_root.join(&agent.container_name);
    std::fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;

    let configs: Vec<AgentConfigEntry> = sqlx::query_as(
        "SELECT config_key, config_value, is_secret FROM agent_configs WHERE agent_id = ?"
    )
    .bind(&agent.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let config_export = serde_json::json!({
        "agent_name": agent.name,
        "template": agent.template,
        "version": agent.version,
        "configs": configs.iter().map(|c| serde_json::json!({
            "key": c.config_key,
            "value": c.config_value,
            "is_secret": c.is_secret,
        })).collect::<Vec<_>>(),
    });

    let meta_path = staging_dir.join("agentbox-export.json");
    std::fs::write(&meta_path, serde_json::to_string_pretty(&config_export)
        .map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    if agent.install_method == "native" && agent.template == "openclaw" {
        export_native_openclaw_data(agent, state, &staging_dir).await?;
    } else {
        let data_dir = agent_data_dir(&agent.container_name).map_err(|e| e.to_string())?;
        copy_dir_contents(&data_dir, &staging_dir).map_err(|e| e.to_string())?;
    }

    let output = tokio::process::Command::new("tar")
        .args([
            "-czf",
            &archive_path.to_string_lossy(),
            "-C", &staging_root.to_string_lossy(),
            &agent.container_name,
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("备份打包失败: {stderr}"));
    }

    sqlx::query(
        "INSERT INTO agent_backups (agent_id, backup_path, version, created_at) VALUES (?, ?, ?, datetime('now'))"
    )
    .bind(&agent.id)
    .bind(archive_path.to_string_lossy().as_ref())
    .bind(&agent.version)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let _ = std::fs::remove_dir_all(&staging_root);

    Ok(archive_path.to_string_lossy().to_string())
}

/// Internal stop agent helper (no Tauri State).
async fn stop_agent_internal(agent: &AgentInfo, state: &AppState) -> Result<(), String> {
    match agent.install_method.as_str() {
        "compose" => {
            let tmpl = template::load_template(&agent.template).map_err(|e| e.to_string())?;
            let compose_file = tmpl.runtime.compose_file.as_deref().unwrap_or("docker-compose.yml");
            let template_dir = template::templates_dir().join(&agent.template);
            let compose_path = template_dir.join(compose_file);

            let docker = docker_for_agent(state, agent);
            let _ = docker.compose(
                &["-f", &compose_path.to_string_lossy(), "-p", &agent.container_name, "stop"],
                &[],
            ).await;
        }
        "native" => {
            let tmpl = template::load_template(&agent.template).map_err(|e| e.to_string())?;
            stop_native_agent_process(state, agent, tmpl.runtime.start_cmd.as_deref()).await;
        }
        _ => {
            let docker = docker_for_agent(state, agent);
            let _ = docker.stop(&agent.container_name).await;
        }
    }

    sqlx::query("UPDATE agents SET status = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(STATUS_PENDING)
        .bind(&agent.id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

async fn export_native_openclaw_data(
    agent: &AgentInfo,
    state: &AppState,
    staging_dir: &std::path::Path,
) -> Result<(), String> {
    let target_dir = staging_dir.join(".openclaw");
    let vm = vm_for_agent(state, agent);

    if vm.docker_cmd_prefix().is_empty() {
        let host_state_dir = openclaw_host_home_dir(&agent.container_name)?.join(".openclaw");
        copy_dir_contents(&host_state_dir, &target_dir).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let ssh_info = vm
        .ssh_info()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "当前平台暂不支持导出原生 OpenClaw 备份，请先在 macOS/Linux 验证".to_string())?;

    std::fs::create_dir_all(staging_dir).map_err(|e| e.to_string())?;
    let output = tokio::process::Command::new("scp")
        .args([
            "-F",
            &ssh_info.config_file,
            "-r",
            &format!("{}:{}", ssh_info.host, openclaw_state_dir(&agent.container_name)),
            &staging_dir.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("从 VM 拉取 OpenClaw 备份失败: {stderr}"));
    }

    Ok(())
}

async fn import_native_openclaw_data(
    agent: &AgentInfo,
    state: &AppState,
    backup_path: &str,
) -> Result<(), String> {
    let archive_name = std::path::Path::new(backup_path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无效的备份文件路径".to_string())?;
    let vm_tmp_archive = format!("/tmp/{archive_name}");
    let openclaw_home = openclaw_home_dir(&agent.container_name);
    let openclaw_state = openclaw_state_dir(&agent.container_name);
    let vm = vm_for_agent(state, agent);

    if vm.docker_cmd_prefix().is_empty() {
        let host_state_dir = dirs_next::home_dir()
            .ok_or_else(|| "无法确定主目录".to_string())?
            .join(".agentbox")
            .join("native")
            .join(&agent.container_name)
            .join(".openclaw");
        if host_state_dir.exists() {
            std::fs::remove_dir_all(&host_state_dir).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(host_state_dir.parent().unwrap_or(&host_state_dir)).map_err(|e| e.to_string())?;

        let output = tokio::process::Command::new("tar")
            .args([
                "-xzf",
                backup_path,
                "-C",
                &host_state_dir.parent().unwrap_or(&host_state_dir).to_string_lossy(),
            ])
            .output()
            .await
            .map_err(|e| e.to_string())?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("恢复 OpenClaw 备份失败: {stderr}"));
        }

        return Ok(());
    }

    vm
        .provider()
        .copy_into(vm.name(), backup_path, &vm_tmp_archive)
        .await
        .map_err(|e| e.to_string())?;

    let restore_cmd = format!(
        "mkdir -p {openclaw_home} && rm -rf {openclaw_state} && tar -xzf '{vm_tmp_archive}' -C {openclaw_home} && rm -f '{vm_tmp_archive}'",
        openclaw_home = openclaw_home,
        openclaw_state = openclaw_state,
        vm_tmp_archive = shell_escape(&vm_tmp_archive),
    );
    vm.shell_run(&restore_cmd).await.map_err(|e| e.to_string())?;

    Ok(())
}

/// Get backups base directory: ~/.agentbox/backups/
fn backups_base_dir() -> Result<std::path::PathBuf, anyhow::Error> {
    let home = dirs_next::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let dir = home.join(".agentbox").join("backups");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Recursively copy directory contents from src to dst.
fn copy_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> Result<(), anyhow::Error> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_contents(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Remove common ANSI escape/control sequences from terminal logs.
fn strip_ansi_control_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                // CSI sequence: ESC [ ... <final-byte>
                Some('[') => {
                    let _ = chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                    continue;
                }
                // OSC sequence: ESC ] ... BEL or ST (ESC \\)
                Some(']') => {
                    let _ = chars.next();
                    let mut prev_was_esc = false;
                    for c in chars.by_ref() {
                        if c == '\u{7}' {
                            break;
                        }
                        if prev_was_esc && c == '\\' {
                            break;
                        }
                        prev_was_esc = c == '\u{1b}';
                    }
                    continue;
                }
                _ => {
                    // Drop bare ESC-like control prefix.
                    continue;
                }
            }
        }

        output.push(ch);
    }

    output
}

/// Normalize terminal logs for UI display (strip ANSI + wrap overlong lines).
fn sanitize_log_for_display(input: &str) -> String {
    let stripped = strip_ansi_control_sequences(input);
    wrap_long_lines(&stripped, 240)
}

/// Wrap very long lines to keep the log viewer responsive and readable.
fn wrap_long_lines(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        let mut count = 0usize;
        for ch in line.chars() {
            out.push(ch);
            count += 1;
            if count >= max_chars {
                out.push('\n');
                count = 0;
            }
        }
        out.push('\n');
    }

    // Preserve trailing newline behavior from `lines()` when input doesn't end with '\n'.
    if !input.ends_with('\n') {
        let _ = out.pop();
    }

    out
}

fn shell_default_for_agent(_agent: &AgentInfo) -> Option<&'static str> {
    Some("/bin/sh")
}

// ---------------------------------------------------------------------------
// PTY session commands
// ---------------------------------------------------------------------------

/// Build the command args for spawning a PTY session for an agent.
fn pty_command_for_agent(agent: &AgentInfo, state: &AppState) -> Vec<String> {
    if agent.install_method == "native" {
        let openclaw_home = openclaw_home_dir(&agent.container_name);
        let vm = vm_for_agent(state, agent);
        // Open a shell inside the VM
        let prefix = vm.docker_cmd_prefix();
        if prefix.is_empty() {
            // Linux: direct local shell
            if agent.template == "openclaw" {
                vec![
                    "env".into(),
                    format!("OPENCLAW_HOME={openclaw_home}"),
                    "/bin/bash".into(),
                ]
            } else {
                vec!["/bin/bash".into()]
            }
        } else {
            // macOS (Lima): limactl shell agentbox
            // prefix is ["limactl", "shell", "agentbox", "--", "sudo"]
            // We want interactive login shell without sudo: ["limactl", "shell", "agentbox"]
            let limactl = &prefix[0];
            if agent.template == "openclaw" {
                vec![
                    limactl.clone(),
                    "shell".into(),
                    agent.vm_name.clone(),
                    "--".into(),
                    "env".into(),
                    format!("OPENCLAW_HOME={openclaw_home}"),
                    "/bin/bash".into(),
                ]
            } else {
                vec![limactl.clone(), "shell".into(), agent.vm_name.clone()]
            }
        }
    } else {
        // Docker container: docker exec -it <container> /bin/sh
        let docker = docker_for_agent(state, agent);
        let prefix = docker.cmd_prefix();
        let mut cmd: Vec<String> = Vec::new();
        if prefix.is_empty() {
            cmd.push("docker".into());
        } else {
            // VM prefix without sudo (last element); we need the raw prefix for interactive usage
            // prefix is ["limactl", "shell", "agentbox", "--", "sudo"]
            // For interactive docker exec we skip the "sudo" — docker exec handles it
            let limactl = &prefix[0];
            cmd.push(limactl.clone());
            cmd.push("shell".into());
            cmd.push(agent.vm_name.clone());
            cmd.push("--".into());
            cmd.push("docker".into());
        }
        cmd.extend_from_slice(&[
            "exec".into(),
            "-it".into(),
            agent.container_name.clone(),
            "/bin/sh".into(),
        ]);
        cmd
    }
}

/// Open a PTY session for an agent. Output is streamed via Tauri events.
#[tauri::command]
pub async fn pty_spawn(
    session_id: String,
    agent_id: String,
    rows: u16,
    cols: u16,
    state: State<'_, AppState>,
    pty: State<'_, PtySessionManager>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    check_vm_ready(&state)?;

    let agent: AgentInfo = sqlx::query_as("SELECT * FROM agents WHERE id = ?")
    .bind(&agent_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let command = pty_command_for_agent(&agent, &state);
    tracing::info!(session_id = %session_id, agent_id = %agent_id, ?command, "Spawning PTY session");

    pty.spawn(session_id, agent_id, command, rows, cols, app).await
}

/// Write input data to a PTY session.
#[tauri::command]
pub async fn pty_write(
    session_id: String,
    data: String,
    pty: State<'_, PtySessionManager>,
) -> Result<(), String> {
    pty.write(&session_id, data.as_bytes()).await
}

/// Resize a PTY session.
#[tauri::command]
pub async fn pty_resize(
    session_id: String,
    rows: u16,
    cols: u16,
    pty: State<'_, PtySessionManager>,
) -> Result<(), String> {
    pty.resize(&session_id, rows, cols).await
}

/// Close a PTY session.
#[tauri::command]
pub async fn pty_close(
    session_id: String,
    pty: State<'_, PtySessionManager>,
) -> Result<(), String> {
    pty.close(&session_id).await;
    Ok(())
}
