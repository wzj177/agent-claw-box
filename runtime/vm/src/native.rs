//! Native Docker provider for Linux.
//!
//! On Linux there's no VM — Docker runs directly on the host.
//! AgentBox auto-installs Docker via `get.docker.com` if missing.

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::info;

use crate::provider::{VmProvider, VmStatus};
use crate::VmConfig;

pub struct NativeProvider;

impl NativeProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl VmProvider for NativeProvider {
    async fn is_runtime_installed(&self) -> bool {
        // On Linux the "runtime" is Docker itself
        true
    }

    async fn install_runtime(&self) -> Result<()> {
        // Nothing to install — Docker will be handled in install_docker
        Ok(())
    }

    async fn is_docker_ready(&self, _name: &str) -> bool {
        tokio::process::Command::new("docker")
            .arg("info")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn install_docker(&self, _name: &str) -> Result<()> {
        info!("Installing Docker on Linux...");
        let output = tokio::process::Command::new("sh")
            .args(["-c", "curl -fsSL https://get.docker.com | sudo sh && sudo usermod -aG docker $(whoami)"])
            .output()
            .await
            .context("Failed to install Docker")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Docker installation failed: {stderr}");
        }

        // Start docker daemon
        let _ = tokio::process::Command::new("sudo")
            .args(["systemctl", "enable", "--now", "docker"])
            .output()
            .await;

        info!("Docker installed");
        Ok(())
    }

    async fn create(&self, config: &VmConfig) -> Result<()> {
        info!(name = %config.name, "Linux native mode — no VM to create");
        // Just ensure Docker is available
        if !self.is_docker_ready(&config.name).await {
            self.install_docker(&config.name).await?;
        }
        Ok(())
    }

    async fn start(&self, _name: &str) -> Result<()> {
        // Ensure Docker daemon is running
        let _ = tokio::process::Command::new("sudo")
            .args(["systemctl", "start", "docker"])
            .output()
            .await;
        Ok(())
    }

    async fn stop(&self, _name: &str) -> Result<()> {
        // Don't actually stop Docker on Linux — other things may use it
        Ok(())
    }

    async fn delete(&self, _name: &str) -> Result<()> {
        // No VM to delete on Linux
        Ok(())
    }

    async fn status(&self, _name: &str) -> Result<VmStatus> {
        if self.is_docker_ready("").await {
            Ok(VmStatus::Running)
        } else {
            Ok(VmStatus::Stopped)
        }
    }

    fn exec_prefix(&self, _name: &str) -> Vec<String> {
        // Direct execution — no prefix needed
        Vec::new()
    }

    async fn copy_into(&self, _name: &str, host_path: &str, vm_path: &str) -> Result<()> {
        // Direct copy — no VM boundary
        let output = tokio::process::Command::new("cp")
            .args(["-r", host_path, vm_path])
            .output()
            .await
            .context("Failed to copy files")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Copy failed: {stderr}");
        }
        Ok(())
    }

    async fn shell_run(&self, _name: &str, cmd: &str) -> Result<String> {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn command")?;

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
            anyhow::bail!("Command failed (exit {:?}): {}", status.code(), stderr_text);
        }
        Ok(collected.trim().to_string())
    }

    fn open_vm_shell(&self, _name: &str) -> Result<()> {
        let terminals = ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"];
        for term in &terminals {
            let spawned = if *term == "gnome-terminal" {
                std::process::Command::new(term)
                    .args(["--", "bash", "-l"])
                    .spawn()
            } else if *term == "konsole" {
                std::process::Command::new(term)
                    .args(["-e", "bash", "-l"])
                    .spawn()
            } else {
                std::process::Command::new(term)
                    .args(["-e", "bash", "-l"])
                    .spawn()
            };

            if spawned.is_ok() {
                return Ok(());
            }
        }

        anyhow::bail!("No supported terminal emulator found");
    }
}
