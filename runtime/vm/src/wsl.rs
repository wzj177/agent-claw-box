//! WSL2 VM provider for Windows.
//!
//! Uses WSL (Windows Subsystem for Linux) to create a lightweight Linux
//! environment. AgentBox auto-enables WSL2 if needed, imports an Ubuntu image,
//! and installs Docker inside.

use anyhow::{Context, Result};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{info, warn};

use crate::provider::{VmProvider, VmStatus};
use crate::VmConfig;

pub struct WslProvider;

impl WslProvider {
    pub fn new() -> Self {
        Self
    }

    /// Run a wsl.exe command and return stdout.
    async fn run_wsl(args: &[&str]) -> Result<String> {
        let output = tokio::process::Command::new("wsl.exe")
            .args(args)
            .output()
            .await
            .context("Failed to run wsl.exe")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("wsl {} failed: {}", args.join(" "), stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a command inside the WSL distro.
    async fn shell_exec(name: &str, cmd: &str) -> Result<String> {
        let output = tokio::process::Command::new("wsl.exe")
            .args(["-d", name, "--", "sh", "-c", cmd])
            .output()
            .await
            .context("Failed to run command in WSL")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("WSL command failed: {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Check if a named distro exists in wsl --list output.
    async fn distro_exists(name: &str) -> bool {
        let output = tokio::process::Command::new("wsl.exe")
            .args(["--list", "--quiet"])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.lines().any(|line| line.trim() == name)
            }
            _ => false,
        }
    }

    /// 获取当前可用的 Ubuntu 发行版名称（优先 22.04）。
    async fn detect_ubuntu_distro_name() -> Option<String> {
        let output = tokio::process::Command::new("wsl.exe")
            .args(["--list", "--quiet"])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let distros: Vec<String> = stdout
            .lines()
            .map(|l| l.trim().trim_start_matches('*').trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        for preferred in ["Ubuntu-22.04", "Ubuntu", "Ubuntu-24.04"] {
            if distros.iter().any(|d| d == preferred) {
                return Some(preferred.to_string());
            }
        }

        distros.into_iter().find(|d| d.starts_with("Ubuntu"))
    }

    /// 下载 Ubuntu WSL rootfs，依次尝试多个候选 URL，任一成功即返回。
    ///
    /// Ubuntu 官方 WSL rootfs 的 URL 格式历史上曾多次变动，
    /// 使用候选列表可以对抗 CDN 路径调整导致的 404。
    async fn download_ubuntu_rootfs(dest_path: &str) -> Result<()> {
        // 候选 URL：按优先级排列，依次尝试
        let candidates = [
            // 中科大镜像（推荐）
            "https://mirrors.ustc.edu.cn/ubuntu-cloud-images/wsl/noble/current/ubuntu-noble-wsl-amd64-wsl.rootfs.tar.gz",
            "https://mirrors.ustc.edu.cn/ubuntu-cloud-images/wsl/jammy/current/ubuntu-jammy-wsl-amd64-wsl.rootfs.tar.gz",
            // Ubuntu 24.04 LTS (noble) — 当前推荐
            "https://cloud-images.ubuntu.com/wsl/noble/current/ubuntu-noble-wsl-amd64-wsl.rootfs.tar.gz",
            // 备用路径格式（部分镜像站使用）
            "https://cloud-images.ubuntu.com/wsl/releases/noble/release/ubuntu-noble-wsl-amd64-wsl.rootfs.tar.gz",
            // Ubuntu 22.04 LTS (jammy) 降级兜底
            "https://cloud-images.ubuntu.com/wsl/jammy/current/ubuntu-jammy-wsl-amd64-wsl.rootfs.tar.gz",
        ];

        let mut last_err = String::new();
        for url in &candidates {
            info!("下载 Ubuntu WSL rootfs: {}", url);
            let script = format!(
                "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
                url, dest_path
            );
            let output = tokio::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .output()
                .await
                .context("powershell 执行失败")?;

            if output.status.success() {
                info!("rootfs 下载成功: {}", url);
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            warn!("下载失败 ({}): {}", url, stderr.trim());
            last_err = format!("URL: {url}\n{}", stderr.trim());
        }

        anyhow::bail!(
            "所有候选 URL 均下载失败，请检查网络连接。\n最后一次错误：\n{last_err}"
        )
    }

    /// 当 rootfs URL 全部失败时，回退：安装 Ubuntu-22.04 后导出再导入为实例名。
    async fn import_from_ubuntu_2204_fallback(name: &str, install_dir: &str) -> Result<()> {
        info!("rootfs 下载失败，尝试回退流程：wsl --install -d Ubuntu-22.04");

        let install = tokio::process::Command::new("wsl.exe")
            .args(["--install", "-d", "Ubuntu-22.04"])
            .output()
            .await
            .context("执行 wsl --install -d Ubuntu-22.04 失败")?;

        if !install.status.success() {
            let stderr = String::from_utf8_lossy(&install.stderr);
            anyhow::bail!(
                "rootfs 下载失败，且 Ubuntu-22.04 自动安装失败: {stderr}\n请手动执行: wsl --install -d Ubuntu-22.04"
            );
        }

        let source = Self::detect_ubuntu_distro_name()
            .await
            .ok_or_else(|| anyhow::anyhow!("未检测到可导出的 Ubuntu 发行版，请先手动执行 wsl --install -d Ubuntu-22.04"))?;

        let parent_dir = Path::new(install_dir)
            .parent()
            .ok_or_else(|| anyhow::anyhow!("无效的 install_dir: {install_dir}"))?;
        std::fs::create_dir_all(parent_dir)?;
        let export_tar = parent_dir
            .join(format!("{}-ubuntu-2204-export.tar", name))
            .to_string_lossy()
            .to_string();
        let export = tokio::process::Command::new("wsl.exe")
            .args(["--export", &source, &export_tar])
            .output()
            .await
            .context("导出 Ubuntu 发行版失败")?;

        if !export.status.success() {
            let stderr = String::from_utf8_lossy(&export.stderr);
            anyhow::bail!("导出 Ubuntu 发行版失败: {stderr}");
        }

        // --import 的目标目录必须为空（或不存在）
        if Path::new(install_dir).exists() {
            std::fs::remove_dir_all(install_dir)
                .with_context(|| format!("清理旧安装目录失败: {install_dir}"))?;
        }

        let import = tokio::process::Command::new("wsl.exe")
            .args(["--import", name, install_dir, &export_tar, "--version", "2"])
            .output()
            .await
            .context("导入 fallback Ubuntu 到实例失败")?;

        if !import.status.success() {
            let stderr = String::from_utf8_lossy(&import.stderr);
            let stdout = String::from_utf8_lossy(&import.stdout);
            let _ = Self::run_wsl(&["--unregister", name]).await;
            anyhow::bail!("导入 fallback Ubuntu 到实例失败:\nstdout: {stdout}\nstderr: {stderr}");
        }

        let _ = std::fs::remove_file(&export_tar);
        info!(name = %name, source = %source, "Fallback import succeeded");
        Ok(())
    }
}

#[async_trait::async_trait]
impl VmProvider for WslProvider {
    async fn is_runtime_installed(&self) -> bool {
        tokio::process::Command::new("wsl.exe")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn install_runtime(&self) -> Result<()> {
        info!("Enabling WSL2...");
        // Modern Windows 10/11: wsl --install enables everything
        let output = tokio::process::Command::new("wsl.exe")
            .args(["--install", "--no-distribution"])
            .output()
            .await
            .context("Failed to enable WSL2")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // May require reboot
            if stderr.contains("reboot") || stderr.contains("restart") {
                anyhow::bail!("WSL2 已启用，请重启电脑后再打开 AgentBox");
            }
            anyhow::bail!("WSL2 安装失败: {stderr}");
        }

        // Set WSL 2 as default
        let _ = tokio::process::Command::new("wsl.exe")
            .args(["--set-default-version", "2"])
            .output()
            .await;

        info!("WSL2 enabled");
        Ok(())
    }

    async fn is_docker_ready(&self, name: &str) -> bool {
        Self::shell_exec(name, "docker info 2>/dev/null")
            .await
            .is_ok()
    }

    async fn install_docker(&self, name: &str) -> Result<()> {
        info!(distro = name, "Installing Docker inside WSL distro...");
        Self::shell_exec(
            name,
            "curl -fsSL https://get.docker.com | sudo sh && sudo usermod -aG docker $(whoami) && sudo service docker start",
        )
        .await?;
        info!(distro = name, "Docker installed in WSL");
        Ok(())
    }

    async fn create(&self, config: &VmConfig) -> Result<()> {
        info!(name = %config.name, "Creating WSL2 distro");

        if Self::distro_exists(&config.name).await {
            info!(name = %config.name, "WSL distro already exists");
            return self.start(&config.name).await;
        }

        // Always import a fresh Ubuntu rootfs as the target distro name.
        info!("Downloading Ubuntu rootfs for import...");
        let appdata = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".into());
        let install_dir = format!("{}\\AgentBox\\{}", appdata, config.name);
        let workspace_dir = format!("{}\\AgentBox", appdata);
        std::fs::create_dir_all(&workspace_dir)?;
        let rootfs_path = format!("{}\\{}-ubuntu-rootfs.tar.gz", workspace_dir, config.name);

        let downloaded = Self::download_ubuntu_rootfs(&rootfs_path).await;
        if downloaded.is_ok() {
            // --import 的目标目录必须为空（或不存在）
            if Path::new(&install_dir).exists() {
                std::fs::remove_dir_all(&install_dir)
                    .with_context(|| format!("清理旧安装目录失败: {}", install_dir))?;
            }

            let import = tokio::process::Command::new("wsl.exe")
                .args(["--import", &config.name, &install_dir, &rootfs_path, "--version", "2"])
                .output()
                .await
                .context("Failed to import WSL distro")?;

            if !import.status.success() {
                let stderr = String::from_utf8_lossy(&import.stderr);
                let stdout = String::from_utf8_lossy(&import.stdout);
                let _ = Self::run_wsl(&["--unregister", &config.name]).await;
                let lower = format!("{}\n{}", stdout, stderr).to_lowercase();
                if lower.contains("hcs_e_hyperv_not_installed")
                    || lower.contains("enablevirtualization")
                    || lower.contains("registerdistro/createvm")
                {
                    anyhow::bail!(
                        "WSL2 无法创建虚拟机（缺少 Hyper-V / 未开启 BIOS 虚拟化）。\n请二选一：\n1) 在 BIOS 启用 VT-x/AMD-V 并开启 Windows 虚拟化组件后重试；\n2) 直接使用 QEMU 模式（无需 WSL）。\n原始输出:\nstdout: {stdout}\nstderr: {stderr}"
                    );
                }
                anyhow::bail!("WSL import failed:\nstdout: {stdout}\nstderr: {stderr}");
            }

            let _ = std::fs::remove_file(&rootfs_path);
            info!(name = %config.name, "WSL distro imported successfully");
        } else {
            warn!(
                "rootfs download failed, trying fallback Ubuntu-22.04 install/import: {}",
                downloaded
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown error".to_string())
            );
            Self::import_from_ubuntu_2204_fallback(&config.name, &install_dir).await?;
        }

        // Set resource limits via .wslconfig
        let wslconfig_path = dirs_next::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot find home dir"))?
            .join(".wslconfig");
        let config_content = format!(
            "[wsl2]\nmemory={}GB\nprocessors={}\nswap=0\n",
            config.memory_mb / 1024,
            config.cpus,
        );
        std::fs::write(&wslconfig_path, config_content)?;

        // Install Docker inside
        self.install_docker(&config.name).await?;

        Ok(())
    }

    async fn start(&self, name: &str) -> Result<()> {
        // WSL distros auto-start when you run a command in them
        Self::shell_exec(name, "echo started").await?;
        // Ensure docker daemon is running
        let _ = Self::shell_exec(name, "sudo service docker start 2>/dev/null").await;
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::run_wsl(&["--terminate", name]).await?;
        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<()> {
        Self::run_wsl(&["--unregister", name]).await?;
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<VmStatus> {
        if !Self::distro_exists(name).await {
            return Ok(VmStatus::NotCreated);
        }

        let output = tokio::process::Command::new("wsl.exe")
            .args(["--list", "--verbose"])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                for line in stdout.lines() {
                    if line.contains(name) {
                        if line.contains("Running") {
                            return Ok(VmStatus::Running);
                        } else if line.contains("Stopped") {
                            return Ok(VmStatus::Stopped);
                        }
                    }
                }
                Ok(VmStatus::Stopped)
            }
            _ => Ok(VmStatus::NotCreated),
        }
    }

    fn exec_prefix(&self, name: &str) -> Vec<String> {
        vec![
            "wsl.exe".to_string(),
            "-d".to_string(),
            name.to_string(),
            "--".to_string(),
        ]
    }

    async fn copy_into(&self, name: &str, host_path: &str, vm_path: &str) -> Result<()> {
        // WSL can access Windows filesystem via /mnt/c/... so we convert paths
        let wsl_host_path = host_path
            .replace('\\', "/")
            .replacen("C:", "/mnt/c", 1)
            .replacen("D:", "/mnt/d", 1);

        Self::shell_exec(
            name,
            &format!("mkdir -p $(dirname {vm_path}) && cp -r {wsl_host_path} {vm_path}"),
        )
        .await?;
        Ok(())
    }

    async fn shell_run(&self, name: &str, cmd: &str) -> Result<String> {
        let mut child = tokio::process::Command::new("wsl.exe")
            .args(["-d", name, "--", "sh", "-c", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn command in WSL")?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            let mut collected = String::new();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::info!(target: "vm_shell", "{}", line);
                if !collected.is_empty() {
                    collected.push('\n');
                }
                collected.push_str(&line);
            }
            collected
        });

        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            let mut collected = String::new();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::warn!(target: "vm_shell", "{}", line);
                if !collected.is_empty() {
                    collected.push('\n');
                }
                collected.push_str(&line);
            }
            collected
        });

        let (stdout_result, stderr_result) = tokio::join!(stdout_task, stderr_task);
        let collected = stdout_result.unwrap_or_default();
        let stderr_text = stderr_result.unwrap_or_default();

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("WSL command failed (exit {:?}): {}", status.code(), stderr_text);
        }

        Ok(collected.trim().to_string())
    }

    fn open_vm_shell(&self, name: &str) -> Result<()> {
        // Prefer Windows Terminal if available; fallback to plain wsl.exe session.
        let wt_try = std::process::Command::new("cmd")
            .args(["/c", "start", "wt.exe", "wsl", "-d", name])
            .spawn();

        if wt_try.is_err() {
            std::process::Command::new("cmd")
                .args(["/c", "start", "wsl.exe", "-d", name])
                .spawn()?;
        }

        Ok(())
    }
}
