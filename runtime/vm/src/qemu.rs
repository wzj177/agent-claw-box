//! QEMU-based VM provider for Windows (no WSL required).
//!
//! # 架构说明
//!
//! 当 Windows 系统未启用 WSL2 时，AgentBox 使用 QEMU 作为底层虚拟机。
//! QEMU 需要以旁加载（sidecar）方式放置于应用安装目录下的 `qemu\` 子目录中，
//! 或者用户已通过 `winget install qemu` / 官方安装包将其添加到系统 PATH。
//!
//! # 端口映射
//!
//! | 用途          | 宿主端口 (动态)            | 虚拟机内端口 |
//! |---------------|--------------------------|------------|
//! | SSH 访问      | 127.0.0.1:22xx           | 22         |
//! | Docker Socket | 127.0.0.1:24xx           | 2375       |
//!
//! 端口根据 VM 名称哈希计算，确保多实例不冲突。
//!
//! # 使用的镜像
//!
//! Alpine Linux virt 镜像（约 150 MB），首次使用时自动下载。
//! 启动后通过 SSH + cloud-init 完成 Docker 安装。

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::provider::{VmProvider, VmStatus};
use crate::VmConfig;

/// Alpine Linux virt ISO（x86_64），约 150 MB。
const ALPINE_ISO_URL: &str =
    "https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/x86_64/alpine-virt-3.19.1-x86_64.iso";

/// 默认分配给每个 QEMU VM 的内存（MB）。
const DEFAULT_MEMORY_MB: u64 = 2048;

/// QEMU VM 提供者——无需 WSL2，直接在 Windows 上运行 Linux 虚拟机。
pub struct QemuProvider {
    /// qemu-system-x86_64.exe 的路径（旁加载或 PATH 中）。
    qemu_bin: PathBuf,
    /// qemu-img.exe 的路径（与 qemu_bin 同目录）。
    qemu_img_bin: PathBuf,
    /// VM 磁盘与 PID 文件存放目录：`~/.agentbox/vms/`。
    vms_dir: PathBuf,
}

impl QemuProvider {
    fn well_known_qemu_dirs() -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = vec![PathBuf::from(r"C:\Program Files\qemu")];
        for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Ok(v) = std::env::var(key) {
                dirs.push(PathBuf::from(v).join("qemu"));
            }
        }
        if let Ok(v) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(&v).join("Microsoft").join("WinGet").join("Packages"));
            dirs.push(PathBuf::from(v).join("Programs").join("qemu"));
        }
        dirs
    }

    pub fn new() -> Self {
        let (qemu_bin, qemu_img_bin) = Self::locate_qemu_bins();
        let vms_dir = dirs_next::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".agentbox")
            .join("vms");
        Self {
            qemu_bin,
            qemu_img_bin,
            vms_dir,
        }
    }

    fn qemu_pair_from_dir(dir: &Path) -> Option<(PathBuf, PathBuf)> {
        let sys = dir.join("qemu-system-x86_64.exe");
        let img = dir.join("qemu-img.exe");
        if sys.exists() && img.exists() {
            Some((sys, img))
        } else {
            None
        }
    }

    fn find_with_where() -> Option<(PathBuf, PathBuf)> {
        let output = std::process::Command::new("where.exe")
            .arg("qemu-system-x86_64.exe")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let first = stdout
            .lines()
            .map(|s| s.trim())
            .find(|s| !s.is_empty())?;
        let sys = PathBuf::from(first);
        let dir = sys.parent()?;
        let img = dir.join("qemu-img.exe");
        if img.exists() {
            Some((sys, img))
        } else {
            None
        }
    }

    /// 定位 QEMU 二进制文件。优先应用旁加载，其次 PATH(where)，再尝试常见安装目录。
    fn locate_qemu_bins() -> (PathBuf, PathBuf) {
        if let Ok(exe) = std::env::current_exe() {
            let dir = exe
                .parent()
                .unwrap_or(Path::new("."))
                .join("qemu");
            if let Some(pair) = Self::qemu_pair_from_dir(&dir) {
                return pair;
            }
        }

        if let Some(pair) = Self::find_with_where() {
            return pair;
        }

        let dirs = Self::well_known_qemu_dirs();

        for dir in dirs {
            if dir.ends_with("Packages") {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for e in entries.flatten() {
                        let p = e.path();
                        let is_qemu_pkg = p
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.to_lowercase().contains("qemu"))
                            .unwrap_or(false);
                        if is_qemu_pkg {
                            // winget 包目录下通常还有版本子目录
                            if let Ok(subs) = std::fs::read_dir(&p) {
                                for s in subs.flatten() {
                                    let candidate = s.path();
                                    if let Some(pair) = Self::qemu_pair_from_dir(&candidate) {
                                        return pair;
                                    }
                                }
                            }
                            if let Some(pair) = Self::qemu_pair_from_dir(&p) {
                                return pair;
                            }
                        }
                    }
                }
            } else if let Some(pair) = Self::qemu_pair_from_dir(&dir) {
                return pair;
            }
        }

        // 最后降级：依赖 PATH（命令名）
        (
            PathBuf::from("qemu-system-x86_64.exe"),
            PathBuf::from("qemu-img.exe"),
        )
    }

    // ── 路径辅助 ──────────────────────────────────────────────────────────────

    fn disk_path(&self, name: &str) -> PathBuf {
        self.vms_dir.join(format!("{name}.qcow2"))
    }

    fn pid_path(&self, name: &str) -> PathBuf {
        self.vms_dir.join(format!("{name}.pid"))
    }

    fn base_iso_path(&self) -> PathBuf {
        self.vms_dir.join("alpine-virt-base.iso")
    }

    fn ssh_key_path(&self) -> PathBuf {
        self.vms_dir.join("agentbox_qemu_ed25519")
    }

    /// 根据 VM 名称哈希计算 SSH 宿主端口（范围 2200-2299）。
    fn ssh_port(name: &str) -> u16 {
        let sum: u32 = name
            .bytes()
            .fold(0u32, |acc, b| acc.wrapping_add(b as u32));
        2200 + (sum % 100) as u16
    }

    /// Docker Socket 宿主端口（SSH 端口 + 200，范围 2400-2499）。
    fn docker_port(name: &str) -> u16 {
        Self::ssh_port(name) + 200
    }

    // ── 下载辅助（PowerShell，无需额外依赖）──────────────────────────────────

    async fn download_file(url: &str, dest: &Path) -> Result<()> {
        info!("下载: {} → {}", url, dest.display());
        let script = format!(
            "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
            url,
            dest.display()
        );
        let mut child = tokio::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .spawn()
            .context("PowerShell 下载失败")?;

        let status = match tokio::time::timeout(Duration::from_secs(10 * 60), child.wait()).await {
            Ok(result) => result.context("PowerShell 下载失败")?,
            Err(_) => {
                let _ = child.kill().await;
                anyhow::bail!("下载超时（超过10分钟），请检查网络或在部署时选择本地 ISO 文件");
            }
        };
        if !status.success() {
            anyhow::bail!("下载失败（PowerShell 返回非 0 状态）");
        }
        info!("下载完成: {}", dest.display());
        Ok(())
    }

    // ── SSH 密钥管理 ──────────────────────────────────────────────────────────

    /// 确保 SSH 密钥对存在，若不存在则生成。
    async fn ensure_ssh_key(&self) -> Result<()> {
        let key = self.ssh_key_path();
        if key.exists() {
            return Ok(());
        }
        tokio::fs::create_dir_all(&self.vms_dir)
            .await
            .context("创建 vms 目录失败")?;
        let output = tokio::process::Command::new("ssh-keygen")
            .args([
                "-t", "ed25519",
                "-f", key.to_str().unwrap_or("agentbox_qemu_ed25519"),
                "-N", "",        // 无密码
                "-C", "agentbox-qemu",
            ])
            .output()
            .await
            .context("ssh-keygen 执行失败，请确认 OpenSSH 客户端已安装")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("SSH 密钥生成失败: {stderr}");
        }
        info!("SSH 密钥已生成: {}", key.display());
        Ok(())
    }

    // ── PID 管理 ──────────────────────────────────────────────────────────────

    async fn read_pid(&self, name: &str) -> Option<u32> {
        tokio::fs::read_to_string(self.pid_path(name))
            .await
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    /// 检查 PID 对应的 QEMU 进程是否仍在运行（Windows tasklist）。
    async fn is_pid_running(pid: u32) -> bool {
        let output = tokio::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => {
                let s = String::from_utf8_lossy(&o.stdout).to_lowercase();
                s.contains(&pid.to_string()) && s.contains("qemu")
            }
            _ => false,
        }
    }

    // ── SSH 命令执行 ──────────────────────────────────────────────────────────

    async fn ssh_run(&self, name: &str, cmd: &str) -> Result<String> {
        let port = Self::ssh_port(name).to_string();
        let key = self.ssh_key_path();
        let output = tokio::process::Command::new("ssh")
            .args([
                "-p", &port,
                "-i", key.to_str().unwrap_or(""),
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=15",
                "agentbox@127.0.0.1",
                cmd,
            ])
            .output()
            .await
            .context("SSH 命令执行失败")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("SSH 命令失败: {stderr}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[async_trait::async_trait]
impl VmProvider for QemuProvider {
    async fn is_runtime_installed(&self) -> bool {
        // 显式兜底：即使 locate_qemu_bins 早期未命中，也直接检查常见目录。
        for dir in Self::well_known_qemu_dirs() {
            if let Some((sys, img)) = Self::qemu_pair_from_dir(&dir) {
                if sys.exists() && img.exists() {
                    return true;
                }
            }
        }

        if self.qemu_bin.exists() && self.qemu_img_bin.exists() {
            return true;
        }

        let sys_ok = tokio::process::Command::new("qemu-system-x86_64.exe")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        let img_ok = tokio::process::Command::new("qemu-img.exe")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        sys_ok && img_ok
    }

    async fn install_runtime(&self) -> Result<()> {
        // QEMU 本身体积较大，不在应用内自动安装，引导用户手动安装。
        anyhow::bail!(
            "NEEDS_QEMU: 未检测到 QEMU。\n\
             推荐安装方式：\n\
             • winget install qemu（Windows 10/11 推荐）\n\
             • 或从官网下载安装包：https://www.qemu.org/download/#windows\n\
             安装完成后请重启 AgentClawBox。"
        );
    }

    async fn is_docker_ready(&self, name: &str) -> bool {
        self.ssh_run(name, "docker info > /dev/null 2>&1 && echo READY")
            .await
            .map(|s| s.trim() == "READY")
            .unwrap_or(false)
    }

    async fn install_docker(&self, name: &str) -> Result<()> {
        info!(name, "在 QEMU VM (Alpine Linux) 中安装 Docker...");
        // Alpine Linux 使用 apk 包管理器
        let steps = [
            "apk add --no-cache docker docker-compose",
            "rc-update add docker boot",
            "rc-service docker start",
            "addgroup agentbox docker",
        ];
        for step in &steps {
            self.ssh_run(name, step)
                .await
                .with_context(|| format!("安装 Docker 步骤失败: {step}"))?;
        }
        info!(name, "Docker 安装完成");
        Ok(())
    }

    async fn create(&self, config: &VmConfig) -> Result<()> {
        tokio::fs::create_dir_all(&self.vms_dir)
            .await
            .context("创建 vms 目录失败")?;

        self.ensure_ssh_key().await?;

        // 1. 确保基础 Alpine ISO 存在
        let base_iso = self.base_iso_path();
        if let Some(custom_iso) = config.qemu_iso_path.as_ref().filter(|s| !s.trim().is_empty()) {
            let custom_iso_path = PathBuf::from(custom_iso);
            if !custom_iso_path.exists() {
                anyhow::bail!("指定的 ISO 文件不存在: {}", custom_iso);
            }
            if custom_iso_path != base_iso {
                tokio::fs::copy(&custom_iso_path, &base_iso)
                    .await
                    .with_context(|| format!("复制本地 ISO 失败: {}", custom_iso))?;
            }
            info!("已使用本地 ISO: {}", custom_iso);
        } else if !base_iso.exists() {
            info!("首次使用：下载 Alpine Linux 基础镜像（~150 MB，超时10分钟）...");
            Self::download_file(ALPINE_ISO_URL, &base_iso)
                .await
                .context("下载 Alpine Linux 镜像失败")?;
        }

        // 2. 创建可写的 qcow2 覆盖磁盘
        let disk = self.disk_path(&config.name);
        if !disk.exists() {
            let size = format!("{}G", config.disk_gb);
            let output = tokio::process::Command::new(&self.qemu_img_bin)
                .args([
                    "create", "-f", "qcow2",
                    disk.to_str().unwrap_or(""),
                    &size,
                ])
                .output()
                .await
                .context("qemu-img create 执行失败")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("创建 VM 磁盘失败: {stderr}");
            }
        }

        // TODO: 生成 cloud-init nocloud seed ISO 以自动完成：
        //   - 创建 agentbox 用户 + 注入 SSH 公钥
        //   - 配置 dockerd 监听 0.0.0.0:2375
        //   - 设置 APK 镜像源（加速国内访问）
        // 当前需要用户首次进入 VM 手动完成初始化。

        info!(
            name = %config.name,
            disk = %disk.display(),
            ssh_port = Self::ssh_port(&config.name),
            docker_port = Self::docker_port(&config.name),
            "QEMU VM 磁盘已创建"
        );
        Ok(())
    }

    async fn start(&self, name: &str) -> Result<()> {
        // 如果进程已在运行则跳过
        if let Some(pid) = self.read_pid(name).await {
            if Self::is_pid_running(pid).await {
                info!(name, pid, "QEMU VM 已在运行");
                return Ok(());
            }
        }

        let disk = self.disk_path(name);
        let base_iso = self.base_iso_path();
        let ssh_port = Self::ssh_port(name);
        let docker_port = Self::docker_port(name);
        let memory = DEFAULT_MEMORY_MB.to_string();

        let net_arg = format!(
            "user,hostfwd=tcp:127.0.0.1:{ssh_port}-:22,\
             hostfwd=tcp:127.0.0.1:{docker_port}-:2375"
        );

        let qemu = self.qemu_bin.to_str().unwrap_or("qemu-system-x86_64");
        let child = tokio::process::Command::new(qemu)
            .args([
                "-m", &memory,
                "-smp", "2",
                "-cdrom", base_iso.to_str().unwrap_or(""),
                "-drive", &format!("file={},format=qcow2", disk.display()),
                "-net", "nic,model=virtio",
                "-net", &net_arg,
                "-nographic",
                "-serial", "none",
                "-parallel", "none",
                "-enable-kvm",   // 需要 Windows 开启 HAXM 或 WHPX
            ])
            .spawn()
            .context("启动 QEMU 失败，请确认 QEMU 已正确安装")?;

        let pid = child.id().unwrap_or(0);
        tokio::fs::write(self.pid_path(name), pid.to_string())
            .await
            .context("写入 PID 文件失败")?;

        // 让子进程在后台运行
        tokio::spawn(async move {
            let mut child = child;
            let _ = child.wait().await;
        });

        info!(name, pid, ssh_port, docker_port, "QEMU VM 已启动");
        Ok(())
    }

    async fn stop(&self, name: &str) -> Result<()> {
        if let Some(pid) = self.read_pid(name).await {
            let _ = tokio::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output()
                .await;
            let _ = tokio::fs::remove_file(self.pid_path(name)).await;
            info!(name, pid, "QEMU VM 已停止");
        } else {
            warn!(name, "QEMU VM 未运行（无 PID 文件）");
        }
        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<()> {
        self.stop(name).await?;
        let disk = self.disk_path(name);
        if disk.exists() {
            tokio::fs::remove_file(&disk)
                .await
                .context("删除 VM 磁盘文件失败")?;
        }
        info!(name, "QEMU VM 已删除");
        Ok(())
    }

    async fn status(&self, name: &str) -> Result<VmStatus> {
        if let Some(pid) = self.read_pid(name).await {
            if Self::is_pid_running(pid).await {
                return Ok(VmStatus::Running);
            }
            // PID 文件残留（上次异常退出），清理掉
            let _ = tokio::fs::remove_file(self.pid_path(name)).await;
        }
        if self.disk_path(name).exists() {
            Ok(VmStatus::Stopped)
        } else {
            Ok(VmStatus::NotCreated)
        }
    }

    fn exec_prefix(&self, name: &str) -> Vec<String> {
        let port = Self::ssh_port(name).to_string();
        let key = self.ssh_key_path().to_string_lossy().to_string();
        vec![
            "ssh".to_string(),
            "-p".to_string(),
            port,
            "-i".to_string(),
            key,
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "agentbox@127.0.0.1".to_string(),
            "--".to_string(),
        ]
    }

    async fn copy_into(&self, name: &str, host_path: &str, vm_path: &str) -> Result<()> {
        let port = Self::ssh_port(name).to_string();
        let key = self.ssh_key_path();
        let dest = format!("agentbox@127.0.0.1:{vm_path}");
        let output = tokio::process::Command::new("scp")
            .args([
                "-P", &port,
                "-i", key.to_str().unwrap_or(""),
                "-o", "StrictHostKeyChecking=no",
                "-r",
                host_path,
                &dest,
            ])
            .output()
            .await
            .context("scp 执行失败")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("文件拷贝失败: {stderr}");
        }
        Ok(())
    }

    async fn shell_run(&self, name: &str, cmd: &str) -> Result<String> {
        self.ssh_run(name, cmd).await
    }

    fn open_vm_shell(&self, name: &str) -> Result<()> {
        let port = Self::ssh_port(name).to_string();
        let key = self.ssh_key_path().to_string_lossy().to_string();
        // 在 Windows Terminal 或 cmd 中打开 SSH 会话
        let ssh_cmd = format!(
            "ssh -p {port} -i \"{key}\" -o StrictHostKeyChecking=no agentbox@127.0.0.1"
        );
        std::process::Command::new("cmd.exe")
            .args(["/C", "start", "cmd.exe", "/K", &ssh_cmd])
            .spawn()
            .context("打开终端失败")?;
        Ok(())
    }

    async fn vm_ip(&self, _name: &str) -> String {
        // QEMU 使用用户模式网络，从宿主机看始终是 127.0.0.1
        "127.0.0.1".to_string()
    }
}
