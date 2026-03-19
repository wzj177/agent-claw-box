# AgentBox — Copilot Instructions

## Project Overview

AgentBox is a cross-platform desktop application ("Docker Desktop for AI Agents") that lets users deploy and run AI Agent projects inside sandboxed VMs with one click. Same template supports multiple instances. See [docs/AgentBox 项目立项文档.md](../docs/AgentBox%20项目立项文档.md) for full PRD.

---

## Architecture

```
apps/desktop/              # Tauri (Rust) + React + Tailwind — main desktop app
  src/                     # React frontend (Vite + Tailwind, DingTalk-style light UI)
    components/Layout.tsx  # Sidebar + content shell
    pages/AgentsPage.tsx   # Agent instance list with controls
    pages/MarketplacePage.tsx # Template marketplace
    lib/api.ts             # Typed Tauri invoke wrappers
  src-tauri/
    src/main.rs            # Minimal entry; calls lib::run()
    src/lib.rs             # App init, Tauri builder, plugin registration, state setup
    src/commands.rs        # All #[tauri::command] handlers
    src/state.rs           # AppState (Docker runtime, DB pool, HealthChecker)
    src/health.rs          # Background health checker (HTTP probe + docker inspect)
    src/network.rs         # Cross-platform network isolation policy
    src/db.rs              # SQLite pool + migration init
    migrations/            # sqlx migrations
runtime/
  vm/                      # VM management: VmProvider trait (Lima/WSL/native) — NOT YET IMPLEMENTED
  docker/                  # Docker runtime — build/create/start/stop/remove/logs/shell
templates/                 # Agent templates (agent.yaml, Dockerfile, install.sh, run.sh)
registry/                  # Agent Marketplace metadata
scripts/                   # Dev & CI helper scripts
```

Double-isolation model: **Agent → Docker Container → VM (Lima/WSL)**

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop shell | [Tauri 2](https://tauri.app) (Rust backend + WebView frontend) |
| Frontend | React 19 + Tailwind CSS 3 + Vite 6 + Vitest |
| UI style | DingTalk-inspired light theme, PingFang SC / Microsoft YaHei, primary `#1677ff` |
| VM runtime (macOS) | [Lima](https://github.com/lima-vm/lima) |
| VM runtime (Windows) | WSL 2 |
| VM runtime (Linux) | Docker (direct, no VM wrapper) |
| Container runtime | Docker |
| Local database | SQLite via `sqlx` (async, `sqlx::migrate!()`) |
| Package manager | pnpm |
| Node version | v22.12.0 (via nvm) |

---

## Build & Test

> Prerequisites: Rust (stable), Node.js v22.12.0 (via `nvm use v22.12.0`), `pnpm`, Tauri CLI.

```bash
nvm use v22.12.0
cargo install tauri-cli          # first time only
cd apps/desktop && pnpm install  # JS dependencies

cargo tauri dev                  # Development (hot reload)
cargo tauri build                # Production build
cargo test                       # Rust unit tests (workspace)
cd apps/desktop && pnpm test     # Frontend tests (Vitest)
```

---

## Database Schema

SQLite at `~/.agentbox/data.db`. Migrations under `apps/desktop/src-tauri/migrations/`.

- **`agents`** — id (UUID TEXT PK), name, template, instance_no (INT), port (INT), status (`CREATING|RUNNING|STOPPED|ERROR`), auto_start (BOOL), health_url (TEXT nullable), created_at, updated_at (RFC3339). Indexes on template, status.
- **`agent_metrics`** — time-series: agent_id (FK CASCADE), cpu_percent, memory_mb, net_rx_kb, net_tx_kb (REAL), healthy (BOOL), recorded_at. Indexed on (agent_id, recorded_at). **Note**: no pruning policy yet — table grows unbounded.

---

## Key Conventions

### Rust Backend
- **Tauri commands** in `commands.rs`: annotate with `#[tauri::command]`, return `Result<T, String>`. Use `anyhow` internally, convert at boundary via `.map_err(|e| e.to_string())`.
- **AppState** shared via `State<'_, AppState>`: wraps `Arc<ContainerRuntime>`, `SqlitePool`, `Arc<HealthChecker>`.
- **Tauri setup** runs outside Tokio context — use `tauri::async_runtime::block_on()` for async ops in the setup hook.
- **Port assignment**: base 3000 + total agent count offset. **Caveat**: doesn't detect occupied ports.
- **Instance numbering**: max `instance_no` for given template + 1.
- **Logging**: `tracing` crate, configure via `RUST_LOG=agentbox=debug,info`.

### Frontend
- **All UI strings are simplified Chinese** (zh-CN).
- **Tauri invoke wrappers** in `lib/api.ts` — TypeScript interfaces must match Rust serialized structs exactly.
- **Polling model**: AgentsPage refreshes every 15s via `setInterval` (no WebSocket).
- **CSS component classes** in `tailwind.css` `@layer components`: `btn-primary`, `btn-default`, `btn-text`, `btn-danger-text`.
- **Routing**: React Router v7 — `/` (Agents), `/marketplace` (Marketplace).
- **Icons**: `lucide-react`.

### Agent Templates
Each under `templates/<name>/` with four required files:
- **`agent.yaml`** — metadata: name, description, version, runtime config, ports, env vars, resources (cpus/memory_mb/disk_gb), health check URL.
- **`Dockerfile`** — Python 3.11 base, installs iptables/curl, runs `install.sh` then `run.sh`.
- **`install.sh`** — Dependency installation.
- **`run.sh`** — Entrypoint: applies iptables rules if available, then execs main service.

### Agent States: `CREATING → RUNNING ↔ STOPPED → ERROR`

---

## Incomplete Features (TODOs)

These areas have scaffolding but are not fully implemented — be aware when working nearby:

1. **Container provisioning**: `create_agent` in commands.rs stores DB record but has a `// TODO` for actual VM + container provisioning.
2. **VM providers**: `VmProvider` trait in `runtime/vm/` is defined but has no Lima/WSL/Linux implementations.
3. **Metrics pruning**: `agent_metrics` grows unbounded.

---

## Integration Points

- **Docker CLI**: invoked via `std::process::Command` in `runtime/docker/`. Returns parsed stdout/stderr strings.
- **Lima CLI** (`limactl`): invoked via `std::process::Command` on macOS; parse stdout JSON.
- **WSL CLI** (`wsl.exe`): same approach on Windows.
- **SQLite**: `sqlx` with `sqlx::migrate!()` macro; single file at `~/.agentbox/data.db`, pool size 5.

---

## Security

- Agent containers must **never** mount host `$HOME`, `C:\Users`, or `/home` paths.
- Network isolation via iptables: blocks host gateway (172.17.0.1) and private LAN (10/8, 172.16/12, 192.168/16), allows internet. Rules in `network.rs`, injected in container entrypoint.
- API keys passed only as container env vars, never stored in plaintext in SQLite.
- Containers run with `NET_ADMIN` capability only for iptables setup; no other elevated permissions.
- **CSP is currently disabled** (`null`) in `tauri.conf.json` — needs hardening before release.
