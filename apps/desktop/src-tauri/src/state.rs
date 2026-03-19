//! Application shared state.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use agentbox_docker::ContainerRuntime;
use agentbox_vm::VmManager;
use crate::health::HealthChecker;

/// Shared state accessible from all Tauri commands.
/// Wrapped in Arc so it can be cloned into background tasks.
#[derive(Clone)]
pub struct AppState {
    pub docker: Arc<RwLock<ContainerRuntime>>,
    pub db: SqlitePool,
    pub health: Arc<HealthChecker>,
    pub vm: Arc<VmManager>,
    /// Set to true once the VM (or native Docker on Linux) is fully ready.
    /// All agent mutation commands must check this before proceeding.
    pub vm_ready: Arc<AtomicBool>,
    /// Serialises agent provisioning so only one create/upgrade runs at a time.
    pub provisioning_lock: Arc<tokio::sync::Mutex<()>>,
}

impl AppState {
    /// Initialize state: connect DB, run migrations, create VM manager.
    /// Does NOT ensure VM is ready — that happens in a background task
    /// so the UI can show setup progress.
    pub async fn init() -> Result<Self> {
        let db = crate::db::init_pool().await?;
        let health = Arc::new(HealthChecker::new(30));

        // Create VM manager (auto-detects platform)
        let vm = Arc::new(VmManager::with_defaults());

        // Docker runtime starts with no prefix;
        // will be updated after VM is ready.
        let docker = Arc::new(RwLock::new(ContainerRuntime::new()));

        // On Linux, Docker runs natively — no VM setup needed, mark ready immediately.
        // On macOS/Windows, will be set true after ensure_vm_ready() succeeds.
        let vm_ready = Arc::new(AtomicBool::new(cfg!(target_os = "linux")));

        // Start background health checks
        let db_clone = db.clone();
        health.spawn(move || {
            let db = db_clone.clone();
            Box::pin(async move {
                let rows = sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT id, health_url FROM agents WHERE status = 'RUNNING'"
                )
                .fetch_all(&db)
                .await
                .unwrap_or_default();
                rows
            })
        });

        Ok(Self { docker, db, health, vm, vm_ready, provisioning_lock: Arc::new(tokio::sync::Mutex::new(())) })
    }

    /// Ensure the VM is fully ready and update the Docker runtime prefix.
    /// Called from a background task on startup.
    pub async fn ensure_vm_ready(&self) -> Result<()> {
        self.vm.ensure_ready(None).await?;

        // Update Docker runtime to route through VM
        let prefix = self.vm.docker_cmd_prefix();
        tracing::info!(prefix = ?prefix, "Docker commands will route through VM");
        self.docker.write().await.set_prefix(prefix);

        // Mark environment as ready — unblocks all agent mutation commands
        self.vm_ready.store(true, Ordering::Release);

        Ok(())
    }
}
