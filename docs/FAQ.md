# AgentClawBox 常见问题（FAQ）

## 目录

- [安装相关](#安装相关)
- [Windows 专题：WSL / QEMU / 图形界面](#windows-专题wsl--qemu--图形界面)
- [环境初始化](#环境初始化)
- [部署问题](#部署问题)
- [运行问题](#运行问题)
- [配置问题](#配置问题)
- [网络问题](#网络问题)
- [数据与备份](#数据与备份)
- [性能与资源](#性能与资源)

---

## 安装相关

### Q: macOS 打开应用提示"无法验证开发者"怎么办？

前往 **系统设置 → 隐私与安全性**，在页面底部找到被阻止的应用，点击「仍要打开」。

### Q: macOS 需要安装什么依赖？

需要安装 Lima（虚拟机管理工具）：

```bash
brew install lima
```

### Q: Windows 需要启用 WSL 吗？

**推荐但不强制**。AgentClawBox 启动时会自动检测：

| 情况 | 行为 |
|------|------|
| 系统已有 WSL 2 | 自动使用 WSL 2（性能更好） |
| 未启用 WSL 2 | 自动切换到 QEMU 模式（无需任何手动配置） |

详细安装步骤请参阅下方 [Windows 专题：WSL 安装](#q-如何在-windows-上安装-wsl-2) 章节。

### Q: QEMU 模式是什么？需要额外安装吗？

QEMU 是一种轻量级虚拟机方案，用于在**未启用 WSL 2 的 Windows**（如 Windows Home、公司禁用 Hyper-V 的环境）上运行 Agent。

详细安装步骤请参阅下方 [Windows 专题：QEMU 安装](#q-如何安装-qemu) 章节。

> QEMU 模式首次使用需要下载约 150 MB 的 Alpine Linux 基础镜像，之后每次启动无需重新下载。

### Q: Linux 需要安装 Docker 吗？

是的。Agent 直接在本地 Docker 中运行：

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER
```

注销并重新登录后生效。

---

## Windows 专题：WSL / QEMU / 图形界面

### Q: 云服务器（Windows Server / 阿里云 / 腾讯云等）提示"WSL2 无法创建虚拟机"怎么办？

> **常见场景**：使用云电脑、Windows Server 2022、阿里云 ECS、腾讯云 CVM、AWS EC2 等云主机时遇到此报错。

云服务器的宿主机本身运行在 Hyper-V 上，但**默认不对租户开放嵌套虚拟化**，导致 WSL2 无法启动内层 VM。

**解决方案（按优先级）：**

| 方案 | 操作 | 说明 |
|------|------|------|
| ✅ 推荐：改用 QEMU | 删除当前实例 → 重新部署 → 运行时选「QEMU」 | QEMU 无需 Hyper-V，直接软件模拟 |
| 可选：开启嵌套虚拟化 | 联系云厂商（需重建实例或更换机型） | 阿里云 ECS 可选"弹性裸金属"；AWS 可用 Bare Metal 机型 |

> **快速切换到 QEMU**：在部署实例时，展开「高级选项」，将「运行时」从「自动」改为「QEMU」即可。QEMU 在没有硬件加速的情况下首次启动会较慢（约 5-10 分钟），属正常现象，不影响后续使用。

---

### Q: 如何在 Windows 上安装 WSL 2？

> **适用系统**：Windows 10 版本 2004（Build 19041）及以上 / Windows 11

**步骤 1：以管理员身份打开 PowerShell**

右键点击「开始菜单」→「Windows Terminal（管理员）」或「PowerShell（管理员）」。

**步骤 2：一键安装**

```powershell
wsl --install
```

此命令会自动完成：
- 启用"适用于 Linux 的 Windows 子系统"功能
- 启用"虚拟机平台"功能
- 下载并安装 WSL 2 Linux 内核
- 安装默认的 Ubuntu 发行版

**步骤 3：重启电脑**

安装完成后**必须重启**，重启后 Ubuntu 会自动完成初始化，设置 Linux 用户名和密码即可。

**验证安装**

```powershell
wsl --list --verbose
```

看到 `Ubuntu` 且版本显示为 `2` 即表示安装成功。

> **遇到问题？**
> - 报错"虚拟化未启用"：进入 BIOS/UEFI，开启 **VT-x**（Intel）或 **AMD-V**（AMD）。
> - 公司设备可能禁用了 Hyper-V，此时请改用 **QEMU 模式**，无需 WSL。
> - 参考微软官方文档：[https://learn.microsoft.com/zh-cn/windows/wsl/install](https://learn.microsoft.com/zh-cn/windows/wsl/install)

---

### Q: 如何安装 QEMU？

> QEMU 用于在**未开启 WSL 2** 的 Windows 上运行 Agent，或在需要完整 VM 隔离的场景下使用。

**方法一：通过 winget 安装（推荐）**

以管理员身份打开 PowerShell，执行：

```powershell
winget install --id QEMU.QEMU -e
```

安装完成后**重启 AgentClawBox** 即自动识别。

**方法二：手动下载安装包**

1. 前往 [https://www.qemu.org/download/#windows](https://www.qemu.org/download/#windows)
2. 下载最新版 64-bit Windows 安装包（`.exe`）
3. 按向导安装，安装路径建议保持默认（`C:\Program Files\qemu`）
4. 安装完成后重启 AgentClawBox

**验证安装**

```powershell
qemu-system-x86_64.exe --version
```

能看到版本号即安装成功。

> **注意**：有硬件虚拟化（VT-x / AMD-V / WHPX）时 QEMU 性能较好；若在云服务器（无嵌套虚拟化）环境下，QEMU 会自动回退到纯软件模拟（TCG），**首次启动约需 5-10 分钟**，请耐心等待，之后正常使用不受影响。

---

### Q: Windows 11 如何使用图形界面（GUI）运行 Ubuntu Desktop？

> **适用系统**：Windows 11（Build 22000 及以上）
> **技术**：WSLg（Linux GUI 应用原生支持）
> **无需**：VNC、RDP、xrdp 等额外远程桌面协议

Windows 11 内置了 **WSLg**，可以让 WSL 2 中的 Linux 图形应用直接在 Windows 桌面上弹出窗口，无需任何额外配置。

#### 前提条件

1. 系统为 **Windows 11**（右键「此电脑」→「属性」确认版本 Build ≥ 22000）
2. WSL 2 已正确安装（见上方 WSL 安装教程）
3. 已安装支持 WSLg 的显卡驱动：
   - NVIDIA：驱动版本 ≥ 470.76
   - AMD：驱动版本 ≥ 21.10
   - Intel：驱动版本 ≥ 30.0.100.9684

#### 步骤 1：更新 WSL 到最新版本

```powershell
wsl --update
```

#### 步骤 2：在 AgentClawBox 中部署 Ubuntu Desktop 实例

1. 进入「应用市场」，选择模板后点击「部署」
2. 在部署选项中，**Ubuntu 镜像** 选择「Ubuntu Desktop 22.04（WSL 图形界面）」
3. 点击「确认部署」

> ⚠️ 此选项仅在检测到 **Windows 11** 时才显示，Windows 10 不会出现该选项。

#### 步骤 3：在实例内安装桌面环境

部署完成后，点击实例卡片的「本地终端」进入该实例，执行：

```bash
# 更新软件源
sudo apt update

# 安装轻量桌面（推荐 XFCE，资源占用少）
sudo apt install -y xfce4 xfce4-goodies

# 或安装完整 GNOME 桌面（需要更多内存）
# sudo apt install -y ubuntu-desktop
```

安装过程根据网速约需 5-15 分钟。

#### 步骤 4：启动桌面

安装完成后，在同一终端执行：

```bash
startxfce4
```

XFCE 桌面窗口会直接出现在 Windows 桌面上，和普通 Windows 程序窗口一样操作。

#### 常见问题

| 问题 | 解决方法 |
|------|----------|
| 部署选项中没有「Ubuntu Desktop」 | 系统不是 Windows 11，不支持 WSLg |
| 启动报错 `cannot connect to X server` | 执行 `wsl --update` 更新 WSL，然后重启 WSL |
| 桌面卡顿或花屏 | 更新显卡驱动到支持 WSLg 的版本 |
| `xfce4` 安装失败 | 先运行 `sudo apt update && sudo apt upgrade -y`，再重试 |
| 窗口中文显示方块 | 安装字体：`sudo apt install -y fonts-noto-cjk` |

#### 参考资料

- 微软官方教程：[在 WSL 上运行 Linux GUI 应用](https://learn.microsoft.com/zh-cn/windows/wsl/tutorials/gui-apps)
- WSLg GitHub 项目：[https://github.com/microsoft/wslg](https://github.com/microsoft/wslg)

---

## 环境初始化

### Q: 首次启动很慢？

首次启动需要创建虚拟机并下载 Docker 镜像，通常需要 3-5 分钟，取决于网速。这是一次性操作。

### Q: 提示"运行环境尚未就绪"？

VM 仍在初始化中。请等待状态栏显示"环境就绪"后再操作。

### Q: macOS 上 Lima VM 启动失败怎么办？

1. 检查 Lima 版本：`limactl --version`
2. 尝试手动启动：`limactl start agentbox`
3. 查看日志：`limactl shell agentbox -- journalctl -u docker`
4. 如上述无效，删除并重建：`limactl delete agentbox -f` 然后重启 AgentClawBox

### Q: Windows 上 WSL 无法启动？

1. 确认 WSL 2 已安装：`wsl --list --verbose`
2. 确认系统开启了虚拟化（BIOS 中启用 VT-x/AMD-V）
3. 更新 WSL：`wsl --update`

> 如果仍无法解决，可直接使用 **QEMU 模式**：卸载 WSL 或跳过安装，AgentClawBox 会自动切换。

### Q: QEMU 模式下如何确认虚拟化已开启？

QEMU 也需要 CPU 虚拟化支持（VT-x / AMD-V）。在 BIOS / UEFI 中启用后重启即可。

验证方式（Windows 10/11）：
- 打开任务管理器 → 性能 → CPU，确认「虚拟化: 已启用」

---

## 部署问题

### Q: 部署按钮显示"等待中…"？

已有另一个实例正在部署中。AgentClawBox 为保证稳定性，同一时刻只允许部署一个实例。请等待当前部署完成。

### Q: 部署超时（超过 30 分钟）？

可能原因：
- 网络不稳定导致依赖下载失败
- 代理或防火墙阻断了下载

解决方法：
1. 删除失败的实例
2. 检查网络连接
3. 重新部署

### Q: 部署时提示"VM 无法访问互联网"？

AgentClawBox 部署前会检测 VM 网络连通性。如不通：

1. 检查主机网络连接
2. 如使用代理，确保代理设置正确
3. 在终端检查 VM 网络：
   ```bash
   limactl shell agentbox -- curl -v https://openclaw.ai/
   ```

### Q: 部署失败，状态变为"已停止"？

自动安装可能失败。可以手动安装：

1. 点击 Agent 的 **Web Shell** 或 **本地终端** 进入 VM
2. 手动执行安装命令（查看日志了解失败原因）
3. 安装完成后在界面点击 **启动**

### Q: 部署失败，状态变为"异常"？

查看日志排查错误原因：
1. 点击 **日志** 按钮查看最近日志
2. 常见原因：磁盘空间不足、内存不足、依赖下载失败

---

## 运行问题

### Q: Agent 无法启动？

1. 点击 **日志** 查看错误信息
2. 常见问题：
   - 端口被占用 → 系统会自动分配新端口，但在极端情况下可能失败
   - 依赖缺失 → 可能需要手动重新安装
3. 尝试先 **停止** 再 **启动**

### Q: 点击"打开"无反应？

1. 确认 Agent 状态为 `运行中`
2. 等几秒钟让服务初始化完成
3. 手动在浏览器访问 `http://localhost:<端口号>`（端口号见 Agent 卡片）

### Q: Agent 健康检查持续显示离线？

Agent 服务可能仍在启动中。部分 Agent（如 OpenClaw）首次启动需要执行初始化配置，可能需要 30 秒以上。如果超过 1 分钟仍为离线，查看日志排查。

---

## 配置问题

### Q: 修改配置后不生效？

修改配置后需要点击 **保存并应用**。系统会自动重启 Agent 以加载新配置。

### Q: API Key 填写后提示错误？

1. 确认 API Key 正确无多余空格
2. 确认选择的 LLM 服务商与 Key 匹配
3. 确认 API Key 有余额或已激活

### Q: 如何切换 LLM 服务商？

在配置中更改 **LLM 服务商** 下拉选项并填写对应的 API Key，点击 **保存并应用** 即可。

### Q: 多个实例可以用不同的 API Key 吗？

可以。每个实例的配置完全独立，可以分别设置不同的服务商和 Key。

---

## 网络问题

### Q: Agent 能访问互联网吗？

能。Agent 默认可以正常访问互联网、调用外部 API。

### Q: Agent 能访问我的局域网设备吗？

不能。AgentClawBox 通过网络策略限制了对主机和局域网的访问，这是安全隔离的一部分。Agent 无法：
- 访问宿主机服务
- 扫描局域网
- 连接其他内网设备

### Q: Agent 能访问宿主机文件吗？

不能。Agent 运行在独立的 VM 沙盒中，无法访问宿主机的 `$HOME`、`/Users` 或 `C:\Users` 目录。

---

## 数据与备份

### Q: Agent 数据存储在哪里？

| 数据 | 位置 |
|------|------|
| 应用数据库 | `~/.agentbox/data.db` |
| Agent 实例数据 | `~/.agentbox/native/<实例名>/` |
| 日志 | `~/agentbox-logs/<实例名>.log` |
| 备份 | `~/.agentbox/backups/` |

### Q: 如何备份 Agent 数据？

点击 Agent 卡片上的 **导出** 按钮，选择保存位置，生成 `.tar.gz` 备份文件。

### Q: 如何恢复备份？

点击 Agent 卡片上的 **导入** 按钮，选择备份文件即可恢复。

### Q: 升级会丢失数据吗？

不会。升级过程会自动导出当前数据并导入到新实例，原实例也会保留。

### Q: 删除 Agent 后数据还能恢复吗？

不能。删除操作会清除该实例的所有数据。建议删除前先导出备份。

---

## 性能与资源

### Q: AgentClawBox 占用多少资源？

| 资源 | 用量 |
|------|------|
| 内存 | VM 基础约 1 GB + 每个 Agent 约 500 MB |
| 磁盘 | VM 镜像约 2 GB + 每个 Agent 约 1-3 GB |
| CPU | 按需使用，空闲时几乎不占 |

### Q: 可以同时运行多少个 Agent？

取决于系统资源。推荐：
- 8 GB 内存：1-2 个
- 16 GB 内存：2-4 个
- 32 GB 内存：4-8 个

### Q: 如何减少资源占用？

停止不使用的 Agent 实例即可释放该实例占用的内存和 CPU。

---

## 故障排查

### 日志位置

- **应用日志**：启动应用时在终端查看（`cargo tauri dev` 开发模式）
- **Agent 运行日志**：点击 Agent 卡片的 **日志** 按钮
- **VM 日志**（macOS）：`~/.lima/agentbox/`

### 重置环境

如果遇到无法解决的问题，可以重置整个环境：

1. 在 AgentClawBox 中删除所有 Agent
2. 退出应用
3. macOS：`limactl delete agentbox -f`
4. 删除数据：`rm -rf ~/.agentbox`
5. 重新启动 AgentClawBox

> ⚠️ 此操作会清除所有 Agent 数据，请先导出重要备份。
