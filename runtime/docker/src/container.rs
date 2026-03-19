//! Docker container lifecycle management.
//!
//! All docker commands are routed through a configurable command prefix,
//! allowing execution inside a VM (via `limactl shell` / `wsl -d`).

use anyhow::Result;
use std::collections::HashMap;

/// Status of a Docker container.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContainerStatus {
    Created,
    Running,
    Stopped,
    Removing,
    Error(String),
}

/// Configuration for creating a container.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContainerConfig {
    /// Docker image name (e.g. "agentbox/openclaw:latest").
    pub image: String,
    /// Container name.
    pub name: String,
    /// Port mappings: host_port -> container_port.
    pub ports: HashMap<u16, u16>,
    /// Environment variables passed to the container.
    pub env: HashMap<String, String>,
    /// CPU limit (number of cores).
    pub cpus: Option<f64>,
    /// Memory limit in megabytes.
    pub memory_mb: Option<u64>,
}

/// Manages Docker containers for agents.
///
/// Holds a command prefix that routes all `docker` calls through a VM shell.
/// - macOS: `["limactl", "shell", "agentbox", "--"]`
/// - Windows: `["wsl.exe", "-d", "agentbox", "--"]`
/// - Linux: `[]` (direct execution)
pub struct ContainerRuntime {
    /// Command prefix for reaching docker inside the VM.
    cmd_prefix: Vec<String>,
}

impl ContainerRuntime {
    /// Create a ContainerRuntime that runs docker directly on the host.
    pub fn new() -> Self {
        Self {
            cmd_prefix: Vec::new(),
        }
    }

    /// Create a ContainerRuntime that routes docker through a VM.
    pub fn with_prefix(prefix: Vec<String>) -> Self {
        Self { cmd_prefix: prefix }
    }

    /// Update the command prefix (e.g., after VM setup completes).
    pub fn set_prefix(&mut self, prefix: Vec<String>) {
        self.cmd_prefix = prefix;
    }

    /// Build a `tokio::process::Command` for `docker <args>`.
    fn docker_cmd(&self, docker_args: &[String]) -> tokio::process::Command {
        if self.cmd_prefix.is_empty() {
            let mut cmd = tokio::process::Command::new("docker");
            cmd.args(docker_args);
            cmd
        } else {
            let mut cmd = tokio::process::Command::new(&self.cmd_prefix[0]);
            cmd.args(&self.cmd_prefix[1..]);
            cmd.arg("docker");
            cmd.args(docker_args);
            cmd
        }
    }

    /// Run a docker command and return (success, stdout, stderr).
    async fn run_docker(&self, docker_args: &[String]) -> Result<(bool, String, String)> {
        let output = self.docker_cmd(docker_args).output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok((output.status.success(), stdout, stderr))
    }

    /// Build the full shell command string for opening a terminal.
    fn shell_command_string(&self, docker_args: &str) -> String {
        if self.cmd_prefix.is_empty() {
            docker_args.to_string()
        } else {
            format!("{} {}", self.cmd_prefix.join(" "), docker_args)
        }
    }

    /// Build a Docker image from a template directory.
    pub async fn build_image(&self, tag: &str, dockerfile_dir: &str) -> Result<()> {
        let args: Vec<String> = vec![
            "build".into(), "-t".into(), tag.into(), dockerfile_dir.into(),
        ];
        let (ok, _, stderr) = self.run_docker(&args).await?;
        if !ok {
            anyhow::bail!("docker build failed: {stderr}");
        }
        Ok(())
    }

    /// Create and start a container with extra docker args (e.g., network, volumes).
    pub async fn create_with_args(&self, config: &ContainerConfig, extra_args: &[String]) -> Result<String> {
        let mut args: Vec<String> = vec![
            "run".into(), "-d".into(),
            "--name".into(), config.name.clone(),
        ];

        for (host, container) in &config.ports {
            args.push("-p".into());
            args.push(format!("{host}:{container}"));
        }
        for (key, value) in &config.env {
            args.push("-e".into());
            args.push(format!("{key}={value}"));
        }
        if let Some(cpus) = config.cpus {
            args.push("--cpus".into());
            args.push(format!("{cpus}"));
        }
        if let Some(mem) = config.memory_mb {
            args.push("--memory".into());
            args.push(format!("{mem}m"));
        }
        args.extend_from_slice(extra_args);
        args.push(config.image.clone());

        let (ok, stdout, stderr) = self.run_docker(&args).await?;
        if !ok {
            anyhow::bail!("docker run failed: {stderr}");
        }
        Ok(stdout)
    }

    /// Create and start a container from the given config.
    pub async fn create(&self, config: &ContainerConfig) -> Result<String> {
        self.create_with_args(config, &[]).await
    }

    /// Stop a running container.
    pub async fn stop(&self, name: &str) -> Result<()> {
        let args: Vec<String> = vec!["stop".into(), name.into()];
        let (ok, _, stderr) = self.run_docker(&args).await?;
        if !ok {
            anyhow::bail!("docker stop failed: {stderr}");
        }
        Ok(())
    }

    /// Start an existing stopped container.
    pub async fn start(&self, name: &str) -> Result<()> {
        let args: Vec<String> = vec!["start".into(), name.into()];
        let (ok, _, stderr) = self.run_docker(&args).await?;
        if !ok {
            anyhow::bail!("docker start failed: {stderr}");
        }
        Ok(())
    }

    /// Inspect whether a container is currently running.
    pub async fn status(&self, name: &str) -> Result<ContainerStatus> {
        let args: Vec<String> = vec![
            "inspect".into(),
            "-f".into(),
            "{{.State.Status}}".into(),
            name.into(),
        ];
        let (ok, stdout, stderr) = self.run_docker(&args).await?;
        if !ok {
            if stderr.contains("No such object") {
                return Ok(ContainerStatus::Stopped);
            }
            return Ok(ContainerStatus::Error(stderr));
        }

        Ok(match stdout.trim() {
            "running" => ContainerStatus::Running,
            "created" => ContainerStatus::Created,
            "removing" => ContainerStatus::Removing,
            "exited" | "dead" | "paused" | "restarting" => ContainerStatus::Stopped,
            other => ContainerStatus::Error(format!("unknown container state: {other}")),
        })
    }

    /// Remove a container (force).
    pub async fn remove(&self, name: &str) -> Result<()> {
        let args: Vec<String> = vec!["rm".into(), "-f".into(), name.into()];
        let (ok, _, stderr) = self.run_docker(&args).await?;
        if !ok {
            anyhow::bail!("docker rm failed: {stderr}");
        }
        Ok(())
    }

    /// Get container logs.
    pub async fn logs(&self, name: &str, tail: u32) -> Result<String> {
        let args: Vec<String> = vec![
            "logs".into(), "--tail".into(), tail.to_string(), name.into(),
        ];
        let (_, stdout, _) = self.run_docker(&args).await?;
        Ok(stdout)
    }

    /// Execute a non-interactive command inside a running container and capture combined output.
    pub async fn exec_capture(&self, name: &str, cmd: &str, shell: Option<&str>) -> Result<String> {
        let sh = shell.unwrap_or("/bin/sh");
        let args: Vec<String> = vec![
            "exec".into(),
            name.into(),
            sh.into(),
            "-lc".into(),
            cmd.to_string(),
        ];
        let (ok, stdout, stderr) = self.run_docker(&args).await?;
        if !ok {
            anyhow::bail!("docker exec failed: {stderr}");
        }
        if stderr.is_empty() {
            Ok(stdout)
        } else if stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(format!("{stdout}\n{stderr}"))
        }
    }

    /// Spawn an interactive shell inside a running container.
    /// Opens a new OS terminal window.
    pub fn open_shell(&self, name: &str, shell: Option<&str>) -> Result<()> {
        let sh = shell.unwrap_or("/bin/sh");
        let docker_part = format!("docker exec -it {} {}", name, sh);
        let full_cmd = self.shell_command_string(&docker_part);

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("osascript")
                .args([
                    "-e",
                    &format!(
                        "tell application \"Terminal\" to do script \"{}\"",
                        full_cmd
                    ),
                ])
                .spawn()?;
        }

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/c", "start", "cmd", "/k", &full_cmd])
                .spawn()?;
        }

        #[cfg(target_os = "linux")]
        {
            let terminals = ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"];
            let mut launched = false;
            for term in &terminals {
                let result = if *term == "gnome-terminal" {
                    std::process::Command::new(term)
                        .args(["--", "sh", "-c", &full_cmd])
                        .spawn()
                } else {
                    std::process::Command::new(term)
                        .args(["-e", "sh", "-c", &full_cmd])
                        .spawn()
                };
                if result.is_ok() {
                    launched = true;
                    break;
                }
            }
            if !launched {
                anyhow::bail!("No supported terminal emulator found");
            }
        }

        Ok(())
    }

    /// Pull a Docker image.
    pub async fn pull(&self, image: &str) -> Result<()> {
        let args: Vec<String> = vec!["pull".into(), image.into()];
        let (ok, _, stderr) = self.run_docker(&args).await?;
        if !ok {
            anyhow::bail!("docker pull failed: {stderr}");
        }
        Ok(())
    }

    /// Run a `docker compose` subcommand with optional env vars.
    pub async fn compose(
        &self,
        compose_args: &[&str],
        envs: &[(&str, &str)],
    ) -> Result<(bool, String, String)> {
        let mut args: Vec<String> = vec!["compose".into()];
        args.extend(compose_args.iter().map(|s| s.to_string()));

        let mut cmd = self.docker_cmd(&args);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let output = cmd.output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok((output.status.success(), stdout, stderr))
    }

    /// Ensure a Docker network exists.
    pub async fn ensure_network(&self, name: &str) -> Result<()> {
        let args: Vec<String> = vec!["network".into(), "inspect".into(), name.into()];
        let (ok, _, _) = self.run_docker(&args).await?;
        if !ok {
            let create_args: Vec<String> = vec![
                "network".into(), "create".into(),
                "--driver".into(), "bridge".into(),
                "--internal=false".into(),
                name.into(),
            ];
            let (ok, _, stderr) = self.run_docker(&create_args).await?;
            if !ok {
                anyhow::bail!("Failed to create network {name}: {stderr}");
            }
        }
        Ok(())
    }

    /// Get docker stats for all running containers.
    pub async fn stats(&self) -> Result<String> {
        let args: Vec<String> = vec![
            "stats".into(), "--no-stream".into(),
            "--format".into(), "{{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}".into(),
        ];
        let (ok, stdout, _) = self.run_docker(&args).await?;
        if !ok {
            return Ok(String::new());
        }
        Ok(stdout)
    }

    /// Expose the command prefix for external use.
    pub fn cmd_prefix(&self) -> &[String] {
        &self.cmd_prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_runtime_has_empty_prefix() {
        let rt = ContainerRuntime::new();
        assert!(rt.cmd_prefix().is_empty());
    }

    #[test]
    fn with_prefix_stores_prefix() {
        let prefix = vec!["limactl".into(), "shell".into(), "agentbox".into(), "--".into()];
        let rt = ContainerRuntime::with_prefix(prefix.clone());
        assert_eq!(rt.cmd_prefix(), &prefix);
    }

    #[test]
    fn set_prefix_updates_prefix() {
        let mut rt = ContainerRuntime::new();
        assert!(rt.cmd_prefix().is_empty());

        let prefix = vec!["wsl.exe".into(), "-d".into(), "agentbox".into(), "--".into()];
        rt.set_prefix(prefix.clone());
        assert_eq!(rt.cmd_prefix(), &prefix);
    }

    #[test]
    fn docker_cmd_no_prefix_uses_docker_directly() {
        let rt = ContainerRuntime::new();
        let cmd = rt.docker_cmd(&["ps".into(), "-a".into()]);
        // Command's program is "docker"
        let prog = format!("{:?}", cmd);
        assert!(prog.contains("docker"), "Expected 'docker' in command: {prog}");
    }

    #[test]
    fn docker_cmd_with_prefix_prepends() {
        let rt = ContainerRuntime::with_prefix(vec![
            "limactl".into(), "shell".into(), "agentbox".into(), "--".into(),
        ]);
        let cmd = rt.docker_cmd(&["ps".into()]);
        let prog = format!("{:?}", cmd);
        assert!(prog.contains("limactl"), "Expected 'limactl' in command: {prog}");
    }

    #[test]
    fn shell_command_string_no_prefix() {
        let rt = ContainerRuntime::new();
        let s = rt.shell_command_string("docker exec -it foo /bin/sh");
        assert_eq!(s, "docker exec -it foo /bin/sh");
    }

    #[test]
    fn shell_command_string_with_prefix() {
        let rt = ContainerRuntime::with_prefix(vec![
            "limactl".into(), "shell".into(), "agentbox".into(), "--".into(),
        ]);
        let s = rt.shell_command_string("docker exec -it foo /bin/sh");
        assert_eq!(s, "limactl shell agentbox -- docker exec -it foo /bin/sh");
    }

    #[test]
    fn container_config_serialize_roundtrip() {
        let config = ContainerConfig {
            image: "nginx:latest".into(),
            name: "test-agent".into(),
            ports: HashMap::from([(8080, 80)]),
            env: HashMap::from([("KEY".into(), "val".into())]),
            cpus: Some(2.0),
            memory_mb: Some(1024),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: ContainerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test-agent");
        assert_eq!(back.image, "nginx:latest");
        assert_eq!(back.cpus, Some(2.0));
        assert_eq!(back.memory_mb, Some(1024));
    }
}
