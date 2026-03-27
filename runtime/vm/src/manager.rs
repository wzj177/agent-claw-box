//! VM lifecycle manager — auto-detect platform, auto-install, auto-create.
//!
//! `VmManager::ensure_ready()` is the single entry point called on app startup.
//! It handles the full chain: check runtime → install → create VM → start → ensure Docker.

use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use crate::provider::{VmProvider, VmStatus};
use crate::VmConfig;

/// The default VM name used by AgentBox across all platforms.
pub const VM_NAME: &str = "agentbox";

/// Platform-aware VM manager. Wraps the correct provider for the current OS.
pub struct VmManager {
    provider: Arc<dyn VmProvider>,
    config: VmConfig,
    runtime_notice: Option<String>,
}

/// Progress callback type for UI updates during setup.
pub type ProgressFn = Box<dyn Fn(&str) + Send + Sync>;

/// Setup stages reported to the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SetupStage {
    CheckRuntime,
    InstallRuntime,
    CreateVm,
    StartVm,
    InstallDocker,
    Ready,
    Error(String),
}

impl VmManager {
    #[cfg(target_os = "windows")]
    fn runtime_mode_from_name(name: &str) -> Option<String> {
        if name.starts_with("wsl-") {
            Some("wsl".to_string())
        } else if name.starts_with("qemu-") {
            Some("qemu".to_string())
        } else {
            None
        }
    }

    #[cfg(target_os = "windows")]
    fn decode_windows_output(bytes: &[u8]) -> String {
        // Some Windows CLIs return UTF-16LE. Detect by frequent NUL bytes in odd positions.
        let odd_nul_count = bytes
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count();
        let looks_utf16 = bytes.len() >= 4 && odd_nul_count > bytes.len() / 8;

        if looks_utf16 {
            let mut u16s = Vec::with_capacity(bytes.len() / 2);
            for chunk in bytes.chunks_exact(2) {
                u16s.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            String::from_utf16_lossy(&u16s)
        } else {
            String::from_utf8_lossy(bytes).to_string()
        }
    }

    /// Create a new VmManager with the platform-appropriate provider.
    pub fn new(config: VmConfig) -> Self {
        let (provider, runtime_notice) = Self::detect_provider(&config);
        Self {
            provider,
            config,
            runtime_notice,
        }
    }

    /// Create a manager for a specific VM name using default resource sizing.
    pub fn named(name: impl Into<String>) -> Self {
        Self::new(VmConfig {
            name: name.into(),
            ..VmConfig::default()
        })
    }

    /// Create with default VM config.
    pub fn with_defaults() -> Self {
        Self::new(VmConfig {
            name: VM_NAME.to_string(),
            ..VmConfig::default()
        })
    }

    /// Detect the correct VM provider for the current platform.
    fn detect_provider(config: &VmConfig) -> (Arc<dyn VmProvider>, Option<String>) {
        #[cfg(target_os = "macos")]
        {
            let _ = config;
            (Arc::new(crate::lima::LimaProvider::new()), None)
        }
        #[cfg(target_os = "windows")]
        {
            let preferred_mode = config
                .runtime_mode
                .clone()
                .or_else(|| Self::runtime_mode_from_name(&config.name))
                .map(|m| m.to_lowercase());

            if preferred_mode.as_deref() == Some("qemu") {
                tracing::info!("Windows 运行时：用户选择 QEMU 提供者");
                return (
                    Arc::new(crate::qemu::QemuProvider::new()),
                    Some("当前运行模式：QEMU（手动选择）".to_string()),
                );
            }

            // 优先使用 WSL2；若系统未启用 WSL 或缺少 Hyper-V/虚拟化能力，降级到 QEMU。
            let wsl_installed = std::process::Command::new("wsl.exe")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            // 用 `wsl --list --quiet` 而非 `wsl --status` 来检测 HCS 可用性。
            // `wsl --status` 不触发 HCS（Hyper-V Container Service）层，即使嵌套虚拟化
            // 不可用（如云服务器 / 禁用 Hyper-V 的主机）也会返回退出码 0，导致误判为可用。
            // `wsl --list --quiet` 会真正访问 HCS，在不支持嵌套虚拟化的环境下会在
            // stdout/stderr 中输出 HCS_E_HYPERV_NOT_INSTALLED 等关键词。
            // 注意：不检查 exit code，因为无 distro 时也会返回非 0，但 HCS 是正常的。
            let wsl_can_create_vm = std::process::Command::new("wsl.exe")
                .args(["--list", "--quiet"])
                .output()
                .ok()
                .map(|o| {
                    let mut all = String::new();
                    all.push_str(&Self::decode_windows_output(&o.stdout));
                    all.push('\n');
                    all.push_str(&Self::decode_windows_output(&o.stderr));
                    let lower = all.to_lowercase();

                    // 任何 HCS / 嵌套虚拟化不可用信号
                    let hyperv_missing = lower.contains("hcs_e_hyperv_not_installed")
                        || lower.contains("enablevirtualization")
                        || lower.contains("registerdistro/createvm")
                        || lower.contains("不支持 wsl");

                    !hyperv_missing
                })
                .unwrap_or(false);

            if preferred_mode.as_deref() == Some("wsl") {
                if wsl_installed && wsl_can_create_vm {
                    tracing::info!("Windows 运行时：用户选择 WSL2 提供者");
                    return (
                        Arc::new(crate::wsl::WslProvider::new()),
                        Some("当前运行模式：WSL2（手动选择）".to_string()),
                    );
                }

                tracing::warn!("用户选择 WSL2，但当前系统不可用，自动降级到 QEMU");
                return (
                    Arc::new(crate::qemu::QemuProvider::new()),
                    Some(
                        "你选择了 WSL2，但当前系统不可用，已自动切换到 QEMU 模式"
                            .to_string(),
                    ),
                );
            }

            if wsl_installed && wsl_can_create_vm {
                tracing::info!("Windows 运行时：使用 WSL2 提供者");
                (
                    Arc::new(crate::wsl::WslProvider::new()),
                    Some("当前运行模式：WSL2（默认）".to_string()),
                )
            } else {
                tracing::info!(
                    "Windows 运行时：WSL2 不可用于创建虚拟机（可能缺少 Hyper-V/虚拟化），降级到 QEMU 提供者"
                );
                (
                    Arc::new(crate::qemu::QemuProvider::new()),
                    Some(
                        "检测到 WSL2 当前不可用于创建虚拟机，已自动切换到 QEMU 模式（无需 WSL）"
                            .to_string(),
                    ),
                )
            }
        }
        #[cfg(target_os = "linux")]
        {
            let _ = config;
            (Arc::new(crate::native::NativeProvider::new()), None)
        }
    }

    /// Get reference to the underlying provider.
    pub fn provider(&self) -> &dyn VmProvider {
        self.provider.as_ref()
    }

    /// Get the command prefix for running commands inside the VM.
    /// Returns empty vec on Linux (direct execution).
    pub fn docker_cmd_prefix(&self) -> Vec<String> {
        self.provider.exec_prefix(&self.config.name)
    }

    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Build a full command to run `docker <args>` inside the VM.
    pub fn docker_command(&self, docker_args: &[&str]) -> (String, Vec<String>) {
        let prefix = self.docker_cmd_prefix();
        if prefix.is_empty() {
            // Linux: direct
            ("docker".to_string(), docker_args.iter().map(|s| s.to_string()).collect())
        } else {
            // macOS/Windows: route through VM
            let program = prefix[0].clone();
            let mut args: Vec<String> = prefix[1..].to_vec();
            args.push("docker".to_string());
            args.extend(docker_args.iter().map(|s| s.to_string()));
            (program, args)
        }
    }

    /// Ensure the VM runtime itself is installed.
    /// This does not create or start any instance VM.
    pub async fn ensure_runtime_ready(&self, on_progress: Option<&ProgressFn>) -> Result<()> {
        let report = |msg: &str| {
            info!(vm_name = %self.config.name, "{}", msg);
            if let Some(f) = on_progress {
                f(msg);
            }
        };

        if let Some(notice) = &self.runtime_notice {
            report(notice);
        }

        report("检查运行环境...");
        if !self.provider.is_runtime_installed().await {
            report("正在检查前置依赖（Homebrew）...");
            if let Err(e) = self.provider.check_prerequisites().await {
                return Err(e);
            }
            report("正在安装运行环境，首次使用需要几分钟...");
            self.provider.install_runtime().await?;
        }

        Ok(())
    }

    /// Ensure the VM is fully ready (runtime installed, VM created, Docker available).
    /// Returns a vec of stages completed for the frontend to display.
    pub async fn ensure_ready(&self, on_progress: Option<&ProgressFn>) -> Result<()> {
        let report = |msg: &str| {
            info!(vm_name = %self.config.name, "{}", msg);
            if let Some(f) = on_progress {
                f(msg);
            }
        };

        self.ensure_runtime_ready(on_progress).await?;

        // 2. Check VM status
        report("检查虚拟环境...");
        let status = self.provider.status(&self.config.name).await?;

        match status {
            VmStatus::NotCreated => {
                report("正在创建虚拟环境，首次使用需要几分钟...");
                self.provider.create(&self.config).await?;

                // 新创建的 VM 通常处于 Stopped，需要显式启动后才能继续 provisioning。
                report("正在启动虚拟环境...");
                self.provider.start(&self.config.name).await?;
            }
            VmStatus::Stopped => {
                report("正在启动虚拟环境...");
                self.provider.start(&self.config.name).await?;
            }
            VmStatus::Running => {
                report("虚拟环境已就绪，等待连接可用...");
                // QEMU 等进程可能刚启动，SSH 端口尚未就绪；其他 provider 此方法为 no-op。
                self.provider.wait_ready(&self.config.name).await?;
                report("虚拟环境已就绪");
            }
            VmStatus::Starting => {
                report("虚拟环境启动中，请稍候...");
                // Wait for it to finish starting
                for _ in 0..30 {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if let Ok(VmStatus::Running) = self.provider.status(&self.config.name).await {
                        break;
                    }
                }
            }
            VmStatus::Error(e) => {
                warn!(error = %e, "VM in error state, recreating...");
                let _ = self.provider.delete(&self.config.name).await;
                report("正在重新创建虚拟环境...");
                self.provider.create(&self.config).await?;
            }
        }

        // 统一兜底：某些 provider 在 start() 返回后仍需要短暂时间进入 Running。
        if !matches!(self.provider.status(&self.config.name).await?, VmStatus::Running) {
            report("虚拟环境启动中，请稍候...");
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                if let Ok(VmStatus::Running) = self.provider.status(&self.config.name).await {
                    break;
                }
            }
        }

        // 3. Docker is installed on-demand when a Docker-based agent is deployed.
        //    Just check if it's already there and skip if not.
        if self.provider.is_docker_ready(&self.config.name).await {
            report("Docker 环境已就绪");
        } else {
            report("Docker 将在部署 Docker 类 Agent 时自动安装");
        }

        report("环境就绪");
        Ok(())
    }

    /// Copy templates directory into the VM.
    pub async fn sync_templates(&self, host_templates_dir: &str) -> Result<()> {
        self.provider
            .copy_into(&self.config.name, host_templates_dir, "/home/agentbox/templates")
            .await
    }

    /// Run an arbitrary shell command inside the VM (or directly on Linux).
    pub async fn shell_run(&self, cmd: &str) -> Result<String> {
        self.provider.shell_run(&self.config.name, cmd).await
    }

    /// Get the IP address of the VM as seen from the host.
    /// On macOS (Lima) this is the guest VM's LAN IP; elsewhere it's 127.0.0.1.
    pub async fn vm_ip(&self) -> String {
        self.provider.vm_ip(&self.config.name).await
    }

    /// Ensure Docker is installed and running inside the VM.
    /// No-op on Linux (Docker checked directly). Reports progress via optional callback.
    pub async fn ensure_docker(&self, on_progress: Option<&ProgressFn>) -> Result<()> {
        let report = |msg: &str| {
            info!("{}", msg);
            if let Some(f) = on_progress {
                f(msg);
            }
        };

        if self.provider.is_docker_ready(&self.config.name).await {
            return Ok(());
        }
        report("正在安装 Docker，首次部署 Docker 类应用需要几分钟...");
        self.provider.install_docker(&self.config.name).await?;
        report("Docker 安装完成");
        Ok(())
    }

    /// Stop the VM.
    pub async fn stop(&self) -> Result<()> {
        self.provider.stop(&self.config.name).await
    }

    /// Delete the VM/runtime instance.
    pub async fn delete(&self) -> Result<()> {
        self.provider.delete(&self.config.name).await
    }

    /// Get current VM status.
    pub async fn status(&self) -> Result<VmStatus> {
        self.provider.status(&self.config.name).await
    }

    /// Open an interactive SSH shell to the VM in a new OS terminal window.
    pub fn open_vm_shell(&self) -> Result<()> {
        self.provider.open_vm_shell(&self.config.name)
    }

    /// Get SSH connection info for the VM.
    pub fn ssh_info(&self) -> Result<Option<crate::SshInfo>> {
        self.provider.ssh_info(&self.config.name)
    }

    /// Ensure the host's ~/.ssh/config includes the VM SSH config.
    pub fn ensure_ssh_config(&self) -> Result<()> {
        self.provider.ensure_ssh_config(&self.config.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VmConfig;

    #[test]
    fn vm_name_constant() {
        assert_eq!(VM_NAME, "agentbox");
    }

    #[test]
    fn default_config_uses_agentbox() {
        let cfg = VmConfig::default();
        assert_eq!(cfg.name, "agentbox");
        assert_eq!(cfg.cpus, 2);
        assert_eq!(cfg.memory_mb, 4096);
        assert_eq!(cfg.disk_gb, 20);
    }

    #[test]
    fn with_defaults_creates_manager() {
        let mgr = VmManager::with_defaults();
        assert_eq!(mgr.config.name, "agentbox");
    }

    #[test]
    fn docker_command_builds_correctly() {
        let mgr = VmManager::with_defaults();
        let (program, args) = mgr.docker_command(&["ps", "-a"]);
        // On the current platform, verify it's either "docker" or a VM prefix
        #[cfg(target_os = "linux")]
        {
            assert_eq!(program, "docker");
            assert_eq!(args, vec!["ps", "-a"]);
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(program, "limactl");
            assert!(args.contains(&"docker".to_string()));
            assert!(args.contains(&"ps".to_string()));
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(program, "wsl.exe");
            assert!(args.contains(&"docker".to_string()));
        }
    }

    #[test]
    fn docker_cmd_prefix_matches_platform() {
        let mgr = VmManager::with_defaults();
        let prefix = mgr.docker_cmd_prefix();

        #[cfg(target_os = "linux")]
        assert!(prefix.is_empty(), "Linux should have empty prefix");

        #[cfg(target_os = "macos")]
        {
            assert!(!prefix.is_empty(), "macOS should have lima prefix");
            assert_eq!(prefix[0], "limactl");
        }

        #[cfg(target_os = "windows")]
        {
            assert!(!prefix.is_empty(), "Windows should have wsl prefix");
            assert_eq!(prefix[0], "wsl.exe");
        }
    }

    #[test]
    fn setup_stage_serialize() {
        let stage = SetupStage::Ready;
        let json = serde_json::to_string(&stage).unwrap();
        assert!(json.contains("Ready"));

        let err_stage = SetupStage::Error("test error".into());
        let json = serde_json::to_string(&err_stage).unwrap();
        assert!(json.contains("test error"));
    }

    #[test]
    fn vm_status_variants() {
        assert_eq!(VmStatus::NotCreated, VmStatus::NotCreated);
        assert_ne!(VmStatus::Running, VmStatus::Stopped);
        assert_eq!(
            VmStatus::Error("x".into()),
            VmStatus::Error("x".into())
        );
    }
}
