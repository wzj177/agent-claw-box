//! Application shared state.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::Result;
use sqlx::SqlitePool;

use agentbox_docker::ContainerRuntime;
use agentbox_vm::VmManager;
use crate::health::HealthChecker;

/// Shared state accessible from all Tauri commands.
/// Wrapped in Arc so it can be cloned into background tasks.
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub health: Arc<HealthChecker>,
    pub vm: Arc<VmManager>,
    /// Set to true once the VM (or native Docker on Linux) is fully ready.
    /// All agent mutation commands must check this before proceeding.
    pub vm_ready: Arc<AtomicBool>,
    /// Serialises agent provisioning so only one create/upgrade runs at a time.
    pub provisioning_lock: Arc<tokio::sync::Mutex<()>>,
    /// Number of create/upgrade operations currently in flight.
    pub provisioning_count: Arc<AtomicUsize>,
    /// Agent IDs currently being cancelled while provisioning is in flight.
    pub provisioning_cancellations: Arc<tokio::sync::Mutex<HashSet<String>>>,
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

        Ok(Self {
            db,
            health,
            vm,
            vm_ready,
            provisioning_lock: Arc::new(tokio::sync::Mutex::new(())),
            provisioning_count: Arc::new(AtomicUsize::new(0)),
            provisioning_cancellations: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
        })
    }

    pub fn vm_for_name(&self, name: &str) -> Arc<VmManager> {
        Arc::new(VmManager::named(name))
    }

    pub fn docker_for_vm_name(&self, name: &str) -> ContainerRuntime {
        let vm = self.vm_for_name(name);
        ContainerRuntime::with_prefix(vm.docker_cmd_prefix())
    }

    /// Ensure the platform runtime is ready.
    /// Called from a background task on startup.
    pub async fn ensure_vm_ready(&self) -> Result<()> {
        self.vm.ensure_runtime_ready(None).await?;

        // Mark runtime as ready — instance VMs are created on demand per agent.
        self.vm_ready.store(true, Ordering::Release);

        Ok(())
    }

    pub async fn request_provisioning_cancel(&self, agent_id: &str) {
        self.provisioning_cancellations
            .lock()
            .await
            .insert(agent_id.to_string());
    }

    pub async fn clear_provisioning_cancel(&self, agent_id: &str) {
        self.provisioning_cancellations.lock().await.remove(agent_id);
    }

    pub async fn is_provisioning_cancelled(&self, agent_id: &str) -> bool {
        self.provisioning_cancellations
            .lock()
            .await
            .contains(agent_id)
    }
}
