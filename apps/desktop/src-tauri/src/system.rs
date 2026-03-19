//! System resource detection — CPU, memory, disk.

use serde::{Deserialize, Serialize};
use sysinfo::System;

/// Host machine resource summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemResources {
    /// Total CPU cores (logical).
    pub cpu_cores: u32,
    /// Total RAM in MB.
    pub total_memory_mb: u64,
    /// Available RAM in MB.
    pub available_memory_mb: u64,
    /// Free disk space on home partition in GB.
    pub free_disk_gb: u64,
}

/// Detect current system resources.
pub fn detect() -> SystemResources {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_cores = sys.cpus().len() as u32;
    let total_memory_mb = sys.total_memory() / 1024 / 1024;
    let available_memory_mb = sys.available_memory() / 1024 / 1024;

    // Disk space for home directory
    let home = dirs_next::home_dir().unwrap_or_default();
    let free_disk_gb = sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .filter(|d| home.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space() / 1024 / 1024 / 1024)
        .unwrap_or(0);

    SystemResources {
        cpu_cores,
        total_memory_mb,
        available_memory_mb,
        free_disk_gb,
    }
}

/// Calculate max instances and max concurrent running instances based on resources and template requirements.
pub fn calculate_limits(
    resources: &SystemResources,
    cpus_per_agent: u32,
    memory_per_agent_mb: u64,
    disk_per_agent_gb: u64,
) -> (u32, u32) {
    let by_cpu = if cpus_per_agent > 0 {
        resources.cpu_cores / cpus_per_agent
    } else {
        u32::MAX
    };

    let by_mem = if memory_per_agent_mb > 0 {
        (resources.total_memory_mb / memory_per_agent_mb) as u32
    } else {
        u32::MAX
    };

    let by_disk = if disk_per_agent_gb > 0 {
        (resources.free_disk_gb / disk_per_agent_gb) as u32
    } else {
        u32::MAX
    };

    let max_instances = by_cpu.min(by_mem).min(by_disk).max(1);

    // Running concurrently is more conservative: use available memory instead of total
    let by_avail_mem = if memory_per_agent_mb > 0 {
        (resources.available_memory_mb / memory_per_agent_mb) as u32
    } else {
        u32::MAX
    };

    let max_running = by_cpu.min(by_avail_mem).min(by_disk).max(1);

    (max_instances, max_running)
}
