# AgentClawBox 使用指南

## 目录

- [安装](#安装)
- [首次启动与环境初始化](#首次启动与环境初始化)
- [部署 Agent](#部署-agent)
- [配置 Agent](#配置-agent)
- [管理 Agent](#管理-agent)
- [终端操作](#终端操作)
- [数据管理](#数据管理)
- [多实例管理](#多实例管理)

---

## 安装

### macOS

1. 下载 `.dmg` 安装包（Apple Silicon 选择 `aarch64`，Intel 选择 `x64`）
2. 打开 `.dmg`，将 AgentClawBox 拖入「应用程序」文件夹
3. 首次打开若提示"无法验证开发者"，前往 **系统设置 → 隐私与安全性** 点击「仍要打开」

**依赖**：需要安装 [Lima](https://github.com/lima-vm/lima)（用于虚拟机管理）

```bash
brew install lima
```

### Windows

1. 下载 `.exe` 安装包并运行
2. 按向导安装
3. 确保已启用 WSL 2：

```powershell
wsl --install
```

重启后 WSL 2 即可使用。

### Linux（Ubuntu/Debian）

1. 下载 `.deb` 安装包
2. 安装：

```bash
sudo dpkg -i AgentClawBox_*.deb
```

3. 确保已安装 Docker：

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER
# 注销并重新登录
```

---

## 首次启动与环境初始化

启动 AgentClawBox 后，应用会自动检测和初始化运行环境：

| 平台 | 初始化内容 |
|------|-----------|
| macOS | 创建 Lima VM（`agentbox`）、安装 Docker |
| Windows | 检查 WSL 2、安装 Docker |
| Linux | 检查本地 Docker |

初始化过程在后台进行。在状态栏显示"环境就绪"之前，无法创建 Agent。

> 首次初始化约需 3-5 分钟（取决于网速），仅需一次。

---

## 部署 Agent

1. 点击左侧导航栏 **应用市场**
2. 浏览可用模板，点击 **部署** 按钮
3. AgentClawBox 会自动：
   - 在 VM 中安装 Agent 依赖
   - 配置运行环境
   - 启动 Agent 服务
4. 部署完成后跳转到 **我的 Agent** 页面，状态显示为 `运行中`

> **注意**：同一时刻只能部署一个实例，需等前一个完成后才能部署下一个。

### 部署时间参考

| 模板 | 预计时间 |
|------|---------|
| OpenClaw | 5-20 分钟（首次需下载 npm 包） |

---

## 配置 Agent

### OpenClaw 配置

部署完成后需要配置 LLM 服务商和 API Key：

1. 在 Agent 卡片上点击 **配置**（齿轮图标）
2. 填写以下字段：

| 字段 | 说明 | 示例 |
|------|------|------|
| LLM 服务商 | 选择 AI 模型提供商 | `anthropic`、`openai`、`deepseek` |
| API Key | 对应服务商的 API 密钥 | `sk-ant-...` |
| 模型名称 | 使用的模型（可选） | `anthropic/claude-sonnet-4-20250514` |
| 自定义 API 地址 | 仅 openrouter 或自建服务需要 | `https://openrouter.ai/api/v1` |

3. 点击 **保存并应用**

配置保存后 Agent 会自动重启以加载新配置。

### 支持的 LLM 服务商

| 服务商 | 环境变量 |
|--------|---------|
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |
| DeepSeek | `DEEPSEEK_API_KEY` |
| Ollama | （本地运行，无需 Key） |
| OpenRouter | `OPENROUTER_API_KEY` |
| Mistral | `MISTRAL_API_KEY` |
| Moonshot | `MOONSHOT_API_KEY` |
| 通义千问 | `DASHSCOPE_API_KEY` |

---

## 管理 Agent

### 状态说明

| 状态 | 含义 |
|------|------|
| 创建中 | 正在安装依赖和初始化，请耐心等待 |
| 运行中 | Agent 服务正在运行 |
| 已停止 | Agent 已停止 |
| 异常 | 运行出错，查看日志排查 |

### 操作

| 操作 | 说明 |
|------|------|
| 启动 | 启动已停止的 Agent |
| 停止 | 停止运行中的 Agent |
| 打开 | 在浏览器中打开 Agent Dashboard |
| 日志 | 查看 Agent 运行日志 |
| 终端 | 打开 Web Shell 或本地终端 |
| 配置 | 修改 LLM 配置 |
| 导出 | 导出 Agent 数据备份 |
| 升级 | 升级到最新版本（保留数据） |
| 开机自启 | 设置随应用启动自动运行 |
| 删除 | 删除 Agent 及其所有数据 |

---

## 终端操作

AgentClawBox 提供两种终端方式：

### Web Shell

点击 Agent 卡片上的 **Web Shell** 按钮，直接在应用内打开终端。
无需额外配置，适合快速查看和操作。

### 本地终端

点击 **本地终端** 按钮，在 macOS Terminal.app 中打开一个连接到 VM 的 SSH 会话。
该会话已自动设置好 Agent 的隔离环境。

---

## 数据管理

### 导出备份

1. 点击 Agent 卡片上的 **导出**（下载图标）
2. 选择保存位置
3. 生成 `.tar.gz` 备份文件，包含 Agent 配置和状态数据

### 导入恢复

1. 点击 Agent 卡片上的 **导入**
2. 选择之前导出的 `.tar.gz` 文件
3. 数据恢复到当前实例

### 升级

1. 点击 **升级** 按钮
2. AgentClawBox 会自动：
   - 导出当前数据
   - 创建新实例（最新版本）
   - 导入数据到新实例
   - 保留原实例（状态标记为已归档）

---

## 多实例管理

可以从同一模板创建多个独立实例，每个实例：

- 分配独立端口（如 18789、18790、18791…）
- 独立配置（API Key、模型等互不影响）
- 独立数据目录
- 可独立启停

> **限制**：同一时刻只能创建一个实例，需等前一个部署完成后才能创建下一个。

### 端口分配

| 实例 | 默认端口 |
|------|---------|
| openclaw-1 | 18789 |
| openclaw-2 | 18790 |
| openclaw-3 | 18791 |

如端口被占用，系统会自动跳过，寻找下一个可用端口。
