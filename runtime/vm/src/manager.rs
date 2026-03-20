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
    /// Create a new VmManager with the platform-appropriate provider.
    pub fn new(config: VmConfig) -> Self {
        Self {
            provider: Self::detect_provider(),
            config,
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
    fn detect_provider() -> Arc<dyn VmProvider> {
        #[cfg(target_os = "macos")]
        {
            Arc::new(crate::lima::LimaProvider::new())
        }
        #[cfg(target_os = "windows")]
        {
            // 优先使用 WSL2；若系统未启用 WSL，降级到 QEMU（适用于 Windows Home
            // 以及禁用 Hyper-V 的企业环境）。
            let wsl_available = std::process::Command::new("wsl.exe")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if wsl_available {
                tracing::info!("Windows 运行时：使用 WSL2 提供者");
                Arc::new(crate::wsl::WslProvider::new())
            } else {
                tracing::info!("Windows 运行时：WSL2 不可用，降级到 QEMU 提供者");
                Arc::new(crate::qemu::QemuProvider::new())
            }
        }
        #[cfg(target_os = "linux")]
        {
            Arc::new(crate::native::NativeProvider::new())
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
            }
            VmStatus::Stopped => {
                report("正在启动虚拟环境...");
                self.provider.start(&self.config.name).await?;
            }
            VmStatus::Running => {
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
