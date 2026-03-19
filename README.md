# AgentClawBox

> AI Agent 的本地运行平台 —— OpenClaw 龙虾的一键部署桌面应用

AgentClawBox 是一个跨平台桌面应用，用于**一键创建和运行 OpenClaw 等龙虾虚拟环境（Agent VM）**。
用户无需复杂环境配置，即可快速部署和运行主流龙虾项目。

---

## 功能特性

- **一键部署**：选择模板 → 点击部署 → 自动安装运行，无需手动配置环境
- **跨平台支持**：macOS、Windows（WSL 2）、Linux
- **沙盒隔离**：Agent 运行在独立 VM + Docker 容器中，双层隔离保障安全
- **网络管控**：允许互联网访问，禁止内网扫描和主机访问
- **可视化管理**：启动、停止、日志、配置、Web Shell 一站式操作
- **多实例支持**：同一模板可创建多个独立实例，端口和数据互不干扰
- **数据管理**：支持导出备份、导入恢复、一键升级

## 系统要求

| 平台 | 要求 |
|------|------|
| macOS | macOS 12+ / Apple Silicon 或 Intel / 安装 [Lima](https://github.com/lima-vm/lima) |
| Windows | Windows 10/11 / 启用 WSL 2 |
| Linux | Ubuntu 20.04+ / 安装 Docker |

所有平台需要：
- 内存 ≥ 8 GB（推荐 16 GB）
- 磁盘可用空间 ≥ 10 GB
- 网络连接（首次安装需下载 Agent 依赖）

## 快速开始

### 1. 安装

从 [Releases](https://github.com/user/agentclawbox/releases) 下载对应平台安装包：

| 平台 | 安装包 |
|------|--------|
| macOS | `AgentClawBox_x.x.x_aarch64.dmg` 或 `_x64.dmg` |
| Windows | `AgentClawBox_x.x.x_x64-setup.exe` |
| Linux | `AgentClawBox_x.x.x_amd64.deb` |

### 2. 首次启动

启动后 AgentClawBox 会自动检测并初始化运行环境（VM / Docker），首次可能需要几分钟。

### 3. 部署 Agent

1. 点击左侧 **应用市场**
2. 选择需要的 Agent 模板（如 OpenClaw）
3. 点击 **部署**
4. 等待安装完成（状态从 `创建中` 变为 `运行中`）

### 4. 配置 API Key

1. 在 **我的 Agent** 页面找到刚部署的实例
2. 点击 **配置** 按钮
3. 选择 LLM 服务商并填入 API Key
4. 点击 **保存并应用**

### 5. 使用

- 点击 **打开** 按钮在浏览器中访问 Agent Dashboard
- 点击 **终端** 按钮打开 Web Shell 或本地终端

## 支持的 Agent 模板

| 模板 | 说明 | 安装方式 |
|------|------|----------|
| [OpenClaw](https://openclaw.ai/) | 全平台 AI 代理，支持 WhatsApp / Telegram / Discord 等通道 | 原生安装 |

## 项目结构

```
agentclawbox/
├── apps/desktop/          # Tauri + React 桌面应用
│   ├── src/               # React 前端
│   └── src-tauri/         # Rust 后端
├── runtime/
│   ├── docker/            # Docker 容器运行时
│   └── vm/                # VM 管理（Lima / WSL / Native）
├── templates/             # Agent 模板
├── registry/              # 模板市场元数据
└── docs/                  # 文档
```

## 开发

### 前置条件

- Rust (stable)
- Node.js v22.12.0（推荐使用 nvm）
- pnpm
- Tauri CLI

### 本地开发

```bash
nvm use v22.12.0
cd apps/desktop && pnpm install
cargo tauri dev
```

### 构建

```bash
cd apps/desktop && pnpm install
cargo tauri build
```

### 测试

```bash
cargo test                       # Rust 单元测试
cd apps/desktop && pnpm test     # 前端测试
```

## 常见问题

### Q：首次安装时间很长，是否正常？

是的，首次运行 AgentClawBox 需要完成以下准备工作，耗时较长属正常现象：

1. **初始化 VM 环境**（Lima / WSL）：首次创建虚拟机需要下载基础镜像并完成初始化，通常需要 **5～15 分钟**，具体取决于网络速度。
2. **拉取 Docker 基础镜像**：根据网络情况，可能额外需要 **5～10 分钟**。

> 💡 建议在网络良好的环境下首次启动，后续重启速度会大幅缩短（30 秒以内）。

---

### Q：部署 OpenClaw 时间很长，是否正常？

是的。OpenClaw 安装过程包含以下步骤，整体耗时约 **10～30 分钟**：

1. **系统依赖安装**：apt 安装 Python、Node.js、iptables 等依赖包。
2. **Python 环境构建**：创建虚拟环境并通过 pip 安装全量依赖。
3. **Node.js 前端构建**（如适用）：编译前端静态资源。
4. **网络连通性检测**：应用内置 5 次重试机制，若网络不稳定会自动等待后重试，无需手动干预。

> 💡 安装期间状态显示为「创建中」，请耐心等待，不要关闭应用。可在实例详情页查看实时安装日志。

---

### Q：安装失败了怎么办？

- 检查网络是否可以访问国际互联网（GitHub、PyPI）。
- 点击实例卡片上的「取消创建」可清理半成品 VM，重新部署时会从头开始。
- 如问题持续，请前往 [GitHub Issues](https://github.com/wzj177/agent-claw-box/issues) 反馈，附上日志信息。

---

## 文档

- [使用指南](docs/GUIDE.md)
- [软件截图](https://github.com/wzj177/agent-claw-box/wiki/Preview#%E6%95%88%E6%9E%9C%E5%9B%BE)
-

## License

MIT
