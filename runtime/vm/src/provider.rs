//! VM provider abstraction — platform-specific implementations.

use anyhow::Result;

/// Status of a managed VM.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VmStatus {
    /// VM does not exist yet.
    NotCreated,
    Stopped,
    Starting,
    Running,
    Error(String),
}

/// SSH connection info for connecting to the VM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SshInfo {
    /// SSH host alias (e.g. "lima-agentbox")
    pub host: String,
    /// SSH config file path (e.g. "~/.lima/agentbox/ssh.config")
    pub config_file: String,
    /// Full SSH command to connect (e.g. "ssh lima-agentbox")
    pub command: String,
    /// SSH command with explicit config file
    pub command_with_config: String,
}

/// Trait that each platform-specific VM backend must implement.
#[async_trait::async_trait]
pub trait VmProvider: Send + Sync {
    /// Check if the VM runtime tool itself is installed (e.g. lima, wsl).
    async fn is_runtime_installed(&self) -> bool;

    /// Check any prerequisites before installing the runtime (e.g. Homebrew on macOS).
    /// Default is a no-op; platforms override where needed.
    /// Return an error with prefix `NEEDS_BREW:` (or similar) to trigger a dedicated UI guide.
    async fn check_prerequisites(&self) -> Result<()> {
        Ok(())
    }

    /// Install the VM runtime tool (e.g. download lima, enable wsl).
    async fn install_runtime(&self) -> Result<()>;

    /// Check if Docker is available inside the VM.
    async fn is_docker_ready(&self, name: &str) -> bool;

    /// Install Docker inside the VM.
    async fn install_docker(&self, name: &str) -> Result<()>;

    /// Create a new VM with the given config.
    async fn create(&self, config: &super::VmConfig) -> Result<()>;

    /// Start the VM.
    async fn start(&self, name: &str) -> Result<()>;

    /// Stop the VM.
    async fn stop(&self, name: &str) -> Result<()>;

    /// Delete the VM and its resources.
    async fn delete(&self, name: &str) -> Result<()>;

    /// Query current status.
    async fn status(&self, name: &str) -> Result<VmStatus>;

    /// Get the command prefix to execute commands inside the VM.
    /// e.g. `["limactl", "shell", "agentbox", "--"]` on macOS.
    /// Returns empty vec on Linux (direct execution).
    fn exec_prefix(&self, name: &str) -> Vec<String>;

    /// Copy a file/directory from host into the VM.
    async fn copy_into(&self, name: &str, host_path: &str, vm_path: &str) -> Result<()>;

    /// Run an arbitrary shell command inside the VM (or directly on host for Linux).
    /// Returns stdout on success.
    async fn shell_run(&self, name: &str, cmd: &str) -> Result<String>;

    /// Get the IP address of the VM reachable from the host.
    /// Linux (native) and WSL2 return "127.0.0.1" since they share host networking.
    /// Lima (macOS) returns the guest VM IP obtained via `hostname -I`.
    async fn vm_ip(&self, _name: &str) -> String {
        "127.0.0.1".to_string()
    }

    /// Open an interactive SSH shell to the VM in a new OS terminal window.
    /// Uses SSH protocol so it's compatible with VS Code Remote SSH etc.
    fn open_vm_shell(&self, name: &str) -> Result<()> {
        let _ = name;
        Ok(())
    }

    /// Get SSH connection info for the VM.
    /// Returns None on platforms where SSH isn't needed (e.g. Linux native).
    fn ssh_info(&self, name: &str) -> Result<Option<SshInfo>> {
        let _ = name;
        Ok(None)
    }

    /// Ensure the host's ~/.ssh/config includes the VM SSH config
    /// so `ssh lima-<name>` works without `-F`.
    fn ensure_ssh_config(&self, name: &str) -> Result<()> {
        let _ = name;
        Ok(())
    }

    /// Wait until the VM is fully ready to accept connections (e.g. SSH port open).
    /// Called by the manager when the VM is already in Running state but connectivity
    /// is not yet guaranteed. Default is a no-op — override on SSH-based providers.
    async fn wait_ready(&self, name: &str) -> Result<()> {
        let _ = name;
        Ok(())
    }
}
