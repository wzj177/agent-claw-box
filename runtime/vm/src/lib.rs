//! AgentBox VM runtime — manages Lima (macOS), WSL (Windows), native Docker (Linux).

mod provider;
mod manager;

#[cfg(target_os = "macos")]
mod lima;
#[cfg(target_os = "windows")]
mod wsl;
#[cfg(target_os = "windows")]
mod qemu;
#[cfg(target_os = "linux")]
mod native;

pub use provider::{VmProvider, VmStatus, SshInfo};
pub use manager::{VmManager, SetupStage, VM_NAME};

/// VM configuration for an agent sandbox.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VmConfig {
    /// Unique name for this VM instance.
    pub name: String,
    /// Number of CPU cores allocated.
    pub cpus: u32,
    /// Memory in megabytes.
    pub memory_mb: u64,
    /// Disk size in gigabytes.
    pub disk_gb: u64,
    /// Preferred runtime on Windows: auto | wsl | qemu.
    /// On non-Windows platforms this is ignored.
    #[serde(default)]
    pub runtime_mode: Option<String>,
    /// Preferred Ubuntu image for WSL provisioning, e.g. noble | jammy | ubuntu-22.04-desktop.
    #[serde(default)]
    pub ubuntu_image: Option<String>,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            name: String::from("agentbox"),
            cpus: 2,
            memory_mb: 4096,
            disk_gb: 20,
            runtime_mode: None,
            ubuntu_image: None,
        }
    }
}
