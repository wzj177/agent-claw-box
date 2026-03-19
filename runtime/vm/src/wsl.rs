//! WSL2 VM provider for Windows.
//!
//! Uses WSL (Windows Subsystem for Linux) to create a lightweight Linux
//! environment. AgentBox auto-enables WSL2 if needed, imports an Ubuntu image,
//! and installs Docker inside.

use anyhow::{Context, Result};
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

        // Install a new Ubuntu-based distro
        // Use `wsl --install -d Ubuntu` and then rename, or import a rootfs
        let output = tokio::process::Command::new("wsl.exe")
            .args(["--install", "-d", "Ubuntu", "--name", &config.name])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                info!(name = %config.name, "WSL distro created");
            }
            _ => {
                // Fallback: try import with Ubuntu rootfs download
                info!("Trying import-based distro creation...");
                let appdata = std::env::var("LOCALAPPDATA")
                    .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".into());
                let install_dir = format!("{}\\AgentBox\\{}", appdata, config.name);
                let rootfs_url = "https://cloud-images.ubuntu.com/wsl/noble/current/ubuntu-noble-wsl-amd64-wsl.rootfs.tar.gz";
                let rootfs_path = format!("{}\\ubuntu-rootfs.tar.gz", install_dir);

                // Download rootfs
                std::fs::create_dir_all(&install_dir)?;
                let dl = tokio::process::Command::new("powershell.exe")
                    .args([
                        "-Command",
                        &format!(
                            "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
                            rootfs_url, rootfs_path
                        ),
                    ])
                    .output()
                    .await
                    .context("Failed to download Ubuntu rootfs")?;

                if !dl.status.success() {
                    let stderr = String::from_utf8_lossy(&dl.stderr);
                    anyhow::bail!("Failed to download rootfs: {stderr}");
                }

                // Import
                let import = tokio::process::Command::new("wsl.exe")
                    .args(["--import", &config.name, &install_dir, &rootfs_path, "--version", "2"])
                    .output()
                    .await
                    .context("Failed to import WSL distro")?;

                if !import.status.success() {
                    let stderr = String::from_utf8_lossy(&import.stderr);
                    anyhow::bail!("WSL import failed: {stderr}");
                }

                let _ = std::fs::remove_file(&rootfs_path);
            }
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
