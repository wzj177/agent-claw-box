//! AgentBox Desktop — Tauri application library.

mod commands;
mod db;
mod health;
mod metrics;
mod network;
mod pty;
mod state;
mod system;
mod template;

use tauri::{Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;

use state::AppState;

/// Configure and build the Tauri application.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agentbox=debug,info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Initialize shared state with DB pool + health checker.
            // Must use tauri::async_runtime — setup() does NOT run in a Tokio context.
            let state = tauri::async_runtime::block_on(async { AppState::init().await })?;

            // Background: ensure VM is ready → update docker prefix → auto-start agents → collect metrics
            let state_clone = state.clone();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // 1. Ensure VM + Docker are ready
                match state_clone.ensure_vm_ready().await {
                    Ok(()) => {
                        tracing::info!("VM environment ready");
                        let _ = app_handle.emit("setup-progress", serde_json::json!({
                            "stage": "ready",
                            "message": "环境就绪",
                            "done": true,
                        }));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.starts_with("NEEDS_BREW:") {
                            tracing::warn!("Homebrew not found, prompting user to install");
                            let _ = app_handle.emit("setup-progress", serde_json::json!({
                                "stage": "needs-brew",
                                "message": "需要安装 Homebrew",
                                "done": true,
                            }));
                        } else {
                            tracing::error!(error = %e, "VM setup failed");
                            let _ = app_handle.emit("setup-progress", serde_json::json!({
                                "stage": "error",
                                "message": format!("环境初始化失败: {e}"),
                                "done": true,
                            }));
                        }
                        return;
                    }
                }

                // 1.5 Recover interrupted in-flight states from previous app processes,
                // then normalize legacy statuses and reconcile against actual runtime.
                if let Err(e) = commands::recover_interrupted_agent_statuses(&state_clone).await {
                    tracing::warn!("Failed to recover interrupted agent statuses: {e}");
                }

                if let Err(e) = commands::normalize_agent_statuses(&state_clone).await {
                    tracing::warn!("Failed to normalize agent statuses: {e}");
                }

                if let Err(e) = commands::reconcile_agent_statuses(&state_clone).await {
                    tracing::warn!("Failed to reconcile agent statuses: {e}");
                }

                // 2. Auto-start agents
                if let Err(e) = commands::autostart_agents(&state_clone).await {
                    tracing::error!("Failed to auto-start agents: {e}");
                }

                // 3. Start metrics collection
                metrics::spawn_collector(state_clone.clone(), 30);
            });

            app.manage(state);
            app.manage(pty::PtySessionManager::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::is_provisioning,
            commands::create_agent,
            commands::start_agent,
            commands::stop_agent,
            commands::delete_agent,
            commands::get_agent_logs,
            commands::open_agent_browser,
            commands::set_auto_start,
            commands::get_health_reports,
            commands::get_agent_metrics,
            commands::open_agent_shell,
            commands::run_agent_shell_command,
            commands::list_templates,
            commands::get_system_info,
            commands::get_agent_config,
            commands::save_agent_config,
            commands::apply_agent_config,
            commands::export_agent_data,
            commands::import_agent_data,
            commands::upgrade_agent,
            commands::list_agent_backups,
            commands::get_ssh_info,
            commands::pty_spawn,
            commands::pty_write,
            commands::pty_resize,
            commands::pty_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AgentBox");
}
