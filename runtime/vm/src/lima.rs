//! Lima VM provider for macOS.
//!
//! Lima creates lightweight Linux VMs on macOS using QEMU/VZ.
//! AgentBox auto-downloads lima if missing, creates a VM named "agentbox",
//! and installs Docker inside it.

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::info;

use crate::provider::{VmProvider, VmStatus};
use crate::VmConfig;

pub struct LimaProvider;

impl LimaProvider {
    pub fn new() -> Self {
        Self
    }

    /// Path to limactl binary. Checks common locations.
    fn limactl() -> &'static str {
        // Homebrew default locations
        if std::path::Path::new("/opt/homebrew/bin/limactl").exists() {
            return "/opt/homebrew/bin/limactl";
        }
        if std::path::Path::new("/usr/local/bin/limactl").exists() {
            return "/usr/local/bin/limactl";
        }
        "limactl"
    }

    /// Run a limactl command and return stdout.
    async fn run_limactl(args: &[&str]) -> Result<String> {
        let output = tokio::process::Command::new(Self::limactl())
            .args(args)
            .output()
            .await
            .context("Failed to run limactl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("limactl {} failed: {}", args.join(" "), stderr);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a command inside the VM via limactl shell.
    async fn shell_exec(name: &str, cmd: &str) -> Result<String> {
        let output = tokio::process::Command::new(Self::limactl())
            .args(["shell", name, "sh", "-c", cmd])
            .output()
            .await
            .context("Failed to run command in VM")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("VM command failed: {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Like `shell_exec` but streams stdout/stderr to tracing in real-time.
    /// Used by `shell_run` so callers (e.g. long-running install scripts)
    /// can see live progress in logs.
    async fn shell_exec_streaming(name: &str, cmd: &str) -> Result<String> {
        let mut child = tokio::process::Command::new(Self::limactl())
            .args(["shell", name, "sh", "-c", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn command in VM")?;

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
            // Include both stdout and stderr so callers (e.g. long install scripts that
            // write errors to stdout rather than stderr) can see the actual failure reason.
            let output_info = match (collected.is_empty(), stderr_text.is_empty()) {
                (true, true) => String::new(),
                (true, false) => stderr_text,
                (false, true) => collected,
                (false, false) => format!("stdout:\n{collected}\nstderr:\n{stderr_text}"),
            };
            anyhow::bail!("VM command failed (exit {:?}): {}", status.code(), output_info);
        }

        Ok(collected.trim().to_string())
    }

    /// Install Docker on yum/dnf-based distros (CentOS / RHEL / Fedora / Rocky / Alma).
    /// Tries official repo first, auto-falls back to Aliyun mirror.
    async fn install_docker_yum(name: &str, pkg_mgr: &str) -> Result<()> {
        let repo_file = "/etc/yum.repos.d/docker-ce.repo";

        // Install plugin first (best-effort)
        if pkg_mgr == "dnf" {
            let _ = Self::shell_exec(name, "sudo dnf install -y dnf-plugins-core").await;
        } else {
            let _ = Self::shell_exec(name, "sudo yum install -y yum-utils").await;
        }

        // Try official repo; if that fails use Aliyun
        let add_official = if pkg_mgr == "dnf" {
            format!("sudo dnf config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo")
        } else {
            format!("sudo yum-config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo")
        };

        let pkg_install = format!(
            "sudo {pkg_mgr} install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin"
        );

        let aliyun_repo = format!(
            "sudo curl -fsSL https://mirrors.aliyun.com/docker-ce/linux/centos/docker-ce.repo -o {repo_file}"
        );

        if Self::shell_exec(name, &add_official).await.is_ok()
            && Self::shell_exec(name, &pkg_install).await.is_ok()
        {
            info!(vm = name, "Docker installed via official yum repo");
            return Ok(());
        }

        tracing::warn!(vm = name, "Official yum repo failed, trying Aliyun mirror...");
        Self::shell_exec(name, &aliyun_repo).await?;
        let _ = Self::shell_exec(name, &format!("sudo {pkg_mgr} makecache")).await;
        Self::shell_exec(name, &pkg_install)
            .await
            .context("Docker install via Aliyun yum mirror failed")?;

        info!(vm = name, "Docker installed via Aliyun yum mirror");
        Ok(())
    }

    /// Install Docker on apt-based distros (Debian / Ubuntu).
    /// Tries official repo; auto-falls back to Aliyun, Tsinghua, USTC mirrors.
    async fn install_docker_apt(name: &str) -> Result<()> {
        let distro_id = Self::shell_exec(name, ". /etc/os-release && echo $ID")
            .await
            .unwrap_or_else(|_| "ubuntu".into())
            .trim()
            .to_string();
        let codename = Self::shell_exec(name, ". /etc/os-release && echo $VERSION_CODENAME")
            .await
            .unwrap_or_else(|_| "focal".into())
            .trim()
            .to_string();
        let arch_cmd = "dpkg --print-architecture";

        let _ = Self::shell_exec(name, "sudo install -d /etc/apt/keyrings").await;

        let result: Result<()> = Self::try_apt_mirror(name, "download.docker.com", &distro_id, &codename, arch_cmd).await;
        if result.is_ok() {
            info!(vm = name, "Docker installed via official apt repo");
            return Ok(());
        }

        // Try Aliyun, Tsinghua, USTC in order
        for mirror in &[
            "mirrors.aliyun.com",
            "mirrors.tuna.tsinghua.edu.cn",
            "mirrors.ustc.edu.cn",
        ] {
            tracing::warn!(vm = name, mirror, "Official apt repo failed, trying mirror...");
            if Self::try_apt_mirror(name, mirror, &distro_id, &codename, arch_cmd).await.is_ok() {
                info!(vm = name, mirror, "Docker installed via apt mirror");
                return Ok(());
            }
        }

        anyhow::bail!("Docker apt install failed with all mirrors")
    }

    async fn try_apt_mirror(
        name: &str,
        mirror: &str,
        distro_id: &str,
        codename: &str,
        arch_cmd: &str,
    ) -> Result<()> {
        let gpg_path = "/etc/apt/keyrings/docker.gpg";
        let gpg_url = format!("https://{mirror}/linux/{distro_id}/gpg");
        let add_gpg = format!(
            "curl -fsSL {gpg_url} | sudo gpg --dearmor -o {gpg_path} --batch --yes"
        );
        let add_source = format!(
            r#"echo "deb [arch=$({arch_cmd}) signed-by={gpg_path}] https://{mirror}/linux/{distro_id} {codename} stable" | sudo tee /etc/apt/sources.list.d/docker.list >/dev/null"#
        );
        let update = "sudo apt-get update -y".to_string();
        let install =
            "sudo DEBIAN_FRONTEND=noninteractive apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin".to_string();

        Self::shell_exec(name, &add_gpg).await?;
        Self::shell_exec(name, &add_source).await?;
        Self::shell_exec(name, &update).await?;
        Self::shell_exec(name, &install).await?;
        Ok(())
    }

}

// generate_config removed: we now use Lima's official template:docker

#[async_trait::async_trait]
impl VmProvider for LimaProvider {
    async fn is_runtime_installed(&self) -> bool {
        tokio::process::Command::new(Self::limactl())
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn check_prerequisites(&self) -> Result<()> {
        let brew_ok = tokio::process::Command::new("brew")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if brew_ok {
            info!("Homebrew 已就绪");
            Ok(())
        } else {
            // Sentinel prefix so lib.rs can emit a dedicated 'needs-brew' event.
            anyhow::bail!(
                "NEEDS_BREW: Homebrew 未安装。\n\
                 Lima (AgentBox VM 运行环境) 需要通过 Homebrew 安装。\n\
                 请先安装 Homebrew，然后重启 AgentBox:\n\
                 /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
            )
        }
    }

    async fn install_runtime(&self) -> Result<()> {
        info!("正在通过 Homebrew 安装 Lima...");
        let output = tokio::process::Command::new("brew")
            .args(["install", "lima"])
            .output()
            .await
            .context("Failed to run brew install lima")?;

        if output.status.success() {
            info!("Lima 安装成功");
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("brew install lima failed: {stderr}");
    }

    async fn is_docker_ready(&self, name: &str) -> bool {
        // Use sudo to bypass docker group permission issues on fresh VM start
        Self::shell_exec(name, "sudo docker info")
            .await
            .is_ok()
    }

    async fn install_docker(&self, name: &str) -> Result<()> {
        // Check if Docker is already available
        if Self::shell_exec(name, "sudo docker info").await.is_ok() {
            info!(vm = name, "Docker already installed and ready");
            return Ok(());
        }

        info!(vm = name, "Detecting package manager inside VM...");

        // Detect package manager
        let pkg_mgr = if Self::shell_exec(name, "command -v dnf").await.is_ok() {
            "dnf"
        } else if Self::shell_exec(name, "command -v yum").await.is_ok() {
            "yum"
        } else {
            "apt"
        };
        info!(vm = name, pkg_mgr, "Package manager detected");

        let install_ok = match pkg_mgr {
            "dnf" | "yum" => Self::install_docker_yum(name, pkg_mgr).await.is_ok(),
            _ => Self::install_docker_apt(name).await.is_ok(),
        };

        if !install_ok {
            // Final fallback: official convenience script
            info!(vm = name, "Package-manager install failed, trying get.docker.com...");
            Self::shell_exec(name, "curl -fsSL https://get.docker.com | sudo sh")
                .await
                .context("All Docker install methods failed")?;
        }

        // Ensure daemon is enabled and started
        let _ = Self::shell_exec(name, "sudo systemctl enable docker && sudo systemctl start docker").await;

        // Wait for daemon (up to 60 s)
        for attempt in 0..20u32 {
            if Self::shell_exec(name, "sudo docker info").await.is_ok() {
                info!(vm = name, "Docker daemon is ready");
                return Ok(());
            }
            if attempt < 19 {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }

        anyhow::bail!("Docker daemon not ready after install. Run: limactl shell {name} -- sudo docker info");
    }

    async fn create(&self, config: &VmConfig) -> Result<()> {
        let template = match config.ubuntu_image.as_deref() {
            Some("jammy") | Some("ubuntu-22.04") | Some("ubuntu-22.04-desktop") => "template:ubuntu-22.04",
            Some("ubuntu-lts") | Some("noble") | Some("ubuntu-24.04") | None => "template:ubuntu-lts",
            Some(other) => {
                tracing::warn!(name = %config.name, requested = %other, "Unknown Ubuntu image for Lima, falling back to ubuntu-lts");
                "template:ubuntu-lts"
            }
        };

        info!(name = %config.name, template, "Creating Lima VM (Ubuntu LTS)");

        // Use Lima's default template (plain Ubuntu) — lightweight, no cloud-init Docker.
        // Docker will be installed on-demand when a Docker-based agent is deployed.
        let cpus = config.cpus.to_string();
        let memory = format!("{}GiB", config.memory_mb / 1024);
        let disk = format!("{}GiB", config.disk_gb);

        let output = tokio::process::Command::new(Self::limactl())
            .args([
                "start",
                "--name", &config.name,
                "--tty=false",
                &format!("--set=.cpus={cpus}"),
                &format!("--set=.memory=\"{memory}\""),
                &format!("--set=.disk=\"{disk}\""),
                template,
            ])
            .output()
            .await
            .context("Failed to create Lima VM")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to create VM: {stderr}");
        }

        info!(name = %config.name, "Lima VM created and running");
        Ok(())
    }

    async fn start(&self, name: &str) -> Result<()> {
        // Must use --tty=false to avoid blocking on interactive prompts
        let output = tokio::process::Command::new(Self::limactl())
            .args(["start", "--tty=false", name])
            .output()
            .await
            .context("Failed to start Lima VM")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("limactl start failed: {stderr}");
        }
        info!(vm = name, "Lima VM started");
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        Self::run_limactl(&["stop", name]).await?;
        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<()> {
        Self::run_limactl(&["delete", "--force", name]).await?;
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<VmStatus> {
        let output = tokio::process::Command::new(Self::limactl())
            .args(["list", "--json", name])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    let status_str = val.get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    match status_str {
                        "Running" => Ok(VmStatus::Running),
                        "Stopped" => Ok(VmStatus::Stopped),
                        _ => Ok(VmStatus::Error(status_str.to_string())),
                    }
                } else {
                    Ok(VmStatus::NotCreated)
                }
            }
            _ => Ok(VmStatus::NotCreated),
        }
    }

    fn exec_prefix(&self, name: &str) -> Vec<String> {
        // Use "sudo" so docker commands work regardless of docker group membership
        vec![
            Self::limactl().to_string(),
            "shell".to_string(),
            name.to_string(),
            "--".to_string(),
            "sudo".to_string(),
        ]
    }

    async fn copy_into(&self, name: &str, host_path: &str, vm_path: &str) -> Result<()> {
        Self::run_limactl(&["copy", host_path, &format!("{name}:{vm_path}")]).await?;
        Ok(())
    }

    async fn shell_run(&self, name: &str, cmd: &str) -> Result<String> {
        Self::shell_exec_streaming(name, cmd).await
    }

    /// Return the first non-loopback IPv4 address inside the Lima VM so the
    /// host can reach services bound to the VM's LAN interface (e.g. native
    /// agents started with `--bind lan`).
    async fn vm_ip(&self, name: &str) -> String {
        Self::shell_exec(name, "hostname -I | awk '{print $1}'")
            .await
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// Open a local terminal and connect to Lima VM via SSH.
    fn open_vm_shell(&self, name: &str) -> Result<()> {
        self.ensure_ssh_config(name)?;
        let ssh_host = format!("lima-{name}");

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("osascript")
                .args([
                    "-e",
                    &format!(
                        "tell application \"Terminal\" to do script \"ssh {ssh_host}\""
                    ),
                ])
                .spawn()?;
        }

        Ok(())
    }

    /// Get SSH connection info for the Lima VM.
    fn ssh_info(&self, name: &str) -> Result<Option<crate::SshInfo>> {
        let lima_dir = dirs_next::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
            .join(".lima")
            .join(name);
        let ssh_config = lima_dir.join("ssh.config");

        if !ssh_config.exists() {
            anyhow::bail!(
                "Lima SSH config not found at {}. Is the VM running?",
                ssh_config.display()
            );
        }

        let ssh_host = format!("lima-{name}");
        let config_path = ssh_config.to_string_lossy().to_string();

        Ok(Some(crate::SshInfo {
            host: ssh_host.clone(),
            config_file: config_path.clone(),
            command: format!("ssh {ssh_host}"),
            command_with_config: format!("ssh -F {config_path} {ssh_host}"),
        }))
    }

    /// Add `Include ~/.lima/*/ssh.config` to ~/.ssh/config so `ssh lima-<name>` works.
    fn ensure_ssh_config(&self, _name: &str) -> Result<()> {
        let home = dirs_next::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        let ssh_dir = home.join(".ssh");
        let ssh_config = ssh_dir.join("config");
        let include_line = "Include ~/.lima/*/ssh.config";

        // Create ~/.ssh if it doesn't exist
        if !ssh_dir.exists() {
            std::fs::create_dir_all(&ssh_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&ssh_dir, std::fs::Permissions::from_mode(0o700))?;
            }
        }

        // Check if Include line already exists
        if ssh_config.exists() {
            let content = std::fs::read_to_string(&ssh_config)?;
            if content.contains(include_line) {
                return Ok(());
            }
            // Prepend the Include line (must be at the top of ssh config)
            let new_content = format!("{include_line}\n\n{content}");
            std::fs::write(&ssh_config, new_content)?;
        } else {
            std::fs::write(&ssh_config, format!("{include_line}\n"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&ssh_config, std::fs::Permissions::from_mode(0o644))?;
            }
        }

        info!("Added Lima SSH include to ~/.ssh/config");
        Ok(())
    }
}
