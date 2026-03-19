---
description: "Use when writing or modifying Rust backend code in Tauri commands, state management, database operations, health checks, or Docker/VM runtime. Covers error handling, AppState injection, and Tauri IPC patterns."
applyTo: "apps/desktop/src-tauri/**/*.rs"
---
# Rust Backend Conventions

## Tauri Commands
- All IPC handlers live in `commands.rs`, annotated with `#[tauri::command]`.
- Return `Result<T, String>` — use `anyhow` internally, convert at boundary:
  ```rust
  .map_err(|e| e.to_string())?
  ```
- Inject shared state via `state: State<'_, AppState>`.

## AppState
```rust
pub struct AppState {
    pub docker: Arc<ContainerRuntime>,
    pub db: SqlitePool,
    pub health: Arc<HealthChecker>,
}
```
- Arc-wrapped fields for thread safety.
- Registered in `lib.rs` via `.manage(state)`.

## Setup Hook Pitfall
Tauri setup runs **outside** Tokio context. Use `tauri::async_runtime::block_on()` for any async operations in setup.

## Database
- SQLite via `sqlx` — async queries only.
- Migrations in `migrations/` dir, applied with `sqlx::migrate!()`.
- Agent IDs are UUID v4 (`uuid::Uuid::new_v4().to_string()`).
- Timestamps are RFC3339 UTC strings.

## Port & Instance Assignment
- Port: base `3000 + total_agent_count` (note: no collision detection yet).
- Instance number: `max(instance_no) + 1` for a given template.

## Logging
- Use `tracing` crate: `tracing::info!`, `tracing::error!`, etc.
- Env filter: `RUST_LOG=agentbox=debug,info`.

## Security
- Never mount host home directories into containers.
- API keys only via container env vars, never in SQLite.
- `NET_ADMIN` capability only for iptables setup.
