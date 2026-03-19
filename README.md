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

## 文档

- [使用指南](docs/GUIDE.md)
- [常见问题](docs/FAQ.md)
- [开发文档](docs/develop/)（内部）

## License

MIT
