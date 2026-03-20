import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Types matching Rust side
// ---------------------------------------------------------------------------

export interface AgentInfo {
  id: string;
  name: string;
  template: string;
  instance_no: number;
  port: number;
  status: AgentStatus;
  auto_start: boolean;
  health_url: string | null;
  created_at: string;
  version: string;
  install_method: string;
  container_name: string;
  vm_name: string;
  runtime_mode?: string | null;
  ubuntu_image?: string | null;
}

export type AgentStatus =
  | "CREATING"
  | "CREATE_FAILED"
  | "PENDING"
  | "STARTING"
  | "RUNNING"
  | "START_FAILED";

export interface AgentMetrics {
  cpu_percent: number;
  memory_mb: number;
  net_rx_kb: number;
  net_tx_kb: number;
  healthy: boolean;
  recorded_at: string;
}

export interface HealthReport {
  agent_id: string;
  healthy: boolean;
  last_check: string;
  detail: string;
}

export interface ResourceConfig {
  cpus: number;
  memory_mb: number;
  disk_gb: number;
}

export interface ConfigField {
  key: string;
  label: string;
  type: string;
  required: boolean;
  default: string | null;
  options: string[];
  env_name: string | null;
}

export interface TemplateInfo {
  id: string;
  name: string;
  description: string;
  version: string;
  install_method: string;
  resources: ResourceConfig;
  config_schema: ConfigField[];
}

export interface SystemInfo {
  cpu_cores: number;
  total_memory_mb: number;
  available_memory_mb: number;
  free_disk_gb: number;
  max_instances: number;
  max_running: number;
}

export interface AgentConfigEntry {
  config_key: string;
  config_value: string;
  is_secret: boolean;
}

export interface AgentBackup {
  id: number;
  agent_id: string;
  backup_path: string;
  version: string;
  created_at: string;
}

export interface SshConnectionInfo {
  host: string;
  config_file: string;
  command: string;
  command_with_config: string;
}

export interface CreateAgentOptions {
  runtime_mode?: "auto" | "wsl" | "qemu";
  ubuntu_image?: "noble" | "jammy" | "ubuntu-22.04-desktop";
  qemu_iso_path?: string;
}

// ---------------------------------------------------------------------------
// API calls (thin wrappers around Tauri invoke)
// ---------------------------------------------------------------------------

export const api = {
  listAgents: () => invoke<AgentInfo[]>("list_agents"),

  isProvisioning: () => invoke<boolean>("is_provisioning"),

  createAgent: (name: string, template: string, options?: CreateAgentOptions) =>
    invoke<AgentInfo>("create_agent", { name, template, options }),

  startAgent: (id: string) => invoke<void>("start_agent", { id }),

  stopAgent: (id: string) => invoke<void>("stop_agent", { id }),

  deleteAgent: (id: string) => invoke<void>("delete_agent", { id }),

  getAgentLogs: (id: string, tail?: number) =>
    invoke<string>("get_agent_logs", { id, tail }),

  openAgentShell: (id: string, shell?: string) =>
    invoke<void>("open_agent_shell", { id, shell }),

  runAgentShellCommand: (id: string, command: string) =>
    invoke<string>("run_agent_shell_command", { id, command }),

  openAgentBrowser: (id: string) =>
    invoke<void>("open_agent_browser", { id }),

  setAutoStart: (id: string, enabled: boolean) =>
    invoke<void>("set_auto_start", { id, enabled }),

  getHealthReports: () => invoke<HealthReport[]>("get_health_reports"),

  getAgentMetrics: (id: string, limit?: number) =>
    invoke<AgentMetrics[]>("get_agent_metrics", { id, limit }),

  listTemplates: () => invoke<TemplateInfo[]>("list_templates"),

  getSystemInfo: () => invoke<SystemInfo>("get_system_info"),

  getAgentConfig: (id: string) =>
    invoke<AgentConfigEntry[]>("get_agent_config", { id }),

  saveAgentConfig: (id: string, configs: AgentConfigEntry[]) =>
    invoke<void>("save_agent_config", { id, configs }),

  applyAgentConfig: (id: string) =>
    invoke<void>("apply_agent_config", { id }),

  exportAgentData: (id: string) =>
    invoke<string>("export_agent_data", { id }),

  importAgentData: (id: string, backupPath: string) =>
    invoke<void>("import_agent_data", { id, backupPath }),

  upgradeAgent: (id: string) =>
    invoke<AgentInfo>("upgrade_agent", { id }),

  listAgentBackups: (id: string) =>
    invoke<AgentBackup[]>("list_agent_backups", { id }),

  getSshInfo: (id: string) =>
    invoke<SshConnectionInfo | null>("get_ssh_info", { id }),

  // PTY session management
  ptySpawn: (sessionId: string, agentId: string, rows: number, cols: number) =>
    invoke<void>("pty_spawn", { sessionId, agentId, rows, cols }),

  ptyWrite: (sessionId: string, data: string) =>
    invoke<void>("pty_write", { sessionId, data }),

  ptyResize: (sessionId: string, rows: number, cols: number) =>
    invoke<void>("pty_resize", { sessionId, rows, cols }),

  ptyClose: (sessionId: string) =>
    invoke<void>("pty_close", { sessionId }),
};
