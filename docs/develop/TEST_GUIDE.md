# AgentBox 分步测试指南

本文档覆盖所有已实现功能的测试用例。按顺序执行，每一步标注了**预期结果**。

---

## 0. 前置环境

```bash
# 确认工具链
node -v          # 期望: v22.x
cargo --version  # 期望: 1.x stable
brew --version   # macOS 需要 (用于自动安装 Lima)

# 安装前端依赖
cd apps/desktop && pnpm install
```

---

## 阶段一：单元测试（自动化）

### TC-1.1 Rust 全量单元测试

```bash
cd /path/to/agent-box
cargo test
```

**预期结果：** 33 个测试全部通过，按模块分布：

| 模块 | 测试数 | 覆盖项 |
|------|--------|--------|
| `agentbox_desktop_lib` | 18 | metrics 解析(8)、network 策略(6)、template YAML 解析(4) |
| `agentbox_docker` | 8 | ContainerRuntime 前缀、命令构建、序列化 |
| `agentbox_vm` | 7 | VmManager 配置、平台检测、SetupStage 序列化 |

### TC-1.2 前端编译检查

```bash
cd apps/desktop
pnpm build
```

**预期结果：** TypeScript 编译 + Vite 构建成功，无报错。

---

## 阶段二：VM 运行环境自动化（首次启动）

> **重要：** 本阶段测试 VM 自动安装和初始化链路。首次运行需要下载镜像，可能需要 5-15 分钟。

### TC-2.1 Lima 自动安装

**前提：** 你的机器上没有 limactl（已确认）

```bash
cd apps/desktop/src-tauri
RUST_LOG=agentbox=debug cargo tauri dev
```

**观察日志输出（Terminal），预期按顺序出现：**

1. `检查运行环境...`
2. `正在安装运行环境，首次使用需要几分钟...`（因为没有 limactl）
3. `Installing Lima via Homebrew...`（会运行 `brew install lima`）
4. `Lima installed successfully`
5. `检查虚拟环境...`
6. `正在创建虚拟环境，首次使用需要几分钟...`（创建 Ubuntu VM，纯净版无 Docker）
7. `Docker 将在部署 Docker 类 Agent 时自动安装`
8. `环境就绪`
9. `VM environment ready`
10. `Docker commands will route through VM`，prefix 应为 `["limactl", "shell", "agentbox", "--"]`

**前端 UI 观察：**
- 打开后看到白色全屏 **SetupOverlay**：
  - AgentBox logo + 旋转 loading 图标
  - 步骤日志区域逐行显示上述中文消息
  - 每个完成的步骤前面有绿色 ✓
  - 当前步骤有蓝色旋转图标
  - 底部提示"首次启动需要初始化运行环境，请耐心等待"
- 初始化完成后 Overlay 自动消失，显示正常的"我的实例"页面

**验证 Lima VM 已就位：**

```bash
limactl list
```

预期：看到一行 `agentbox Running ...`

```bash
limactl shell agentbox -- docker info
```

预期：Docker Engine 信息输出，不报错。

### TC-2.2 再次启动（快速路径）

关掉应用后再次启动：

```bash
RUST_LOG=agentbox=debug cargo tauri dev
```

**预期日志：**
1. `检查运行环境...`（limactl 已存在，跳过安装）
2. `检查虚拟环境...`
3. `虚拟环境已就绪`（VM 已在运行）
4. `Docker 将在部署 Docker 类 Agent 时自动安装`（或已安装时显示 `Docker 环境已就绪`）
5. `环境就绪`

**前端：** SetupOverlay 在 ~2 秒内消失。

### TC-2.3 VM 关闭后重启恢复

```bash
limactl stop agentbox
RUST_LOG=agentbox=debug cargo tauri dev
```

**预期日志：** 出现 `正在启动虚拟环境...`，然后 `环境就绪`。

---

## 阶段三：应用市场 & 模板

### TC-3.1 模板列表加载

1. 打开应用，等待初始化完成
2. 点击左侧"应用市场"
3. **预期：** 当前默认只看到 1 个可用模板卡片：
   - **OpenClaw** — `native` 方式，2核/2GB

> **说明：** CoPaw 和 Nanobot 目前为临时隐藏状态，不应出现在应用市场。

4. 页面顶部显示系统信息（CPU 核心数、内存、最大实例数）

### TC-3.2 模板详情验证

每个模板卡片应显示：
- 名称、描述、版本号
- 安装方式标签（docker / compose / script / native）
- 资源需求（CPU / 内存 / 磁盘）
- "部署" 按钮

---

## 阶段四：Agent 生命周期

### TC-4.1 创建 Agent（Docker 方式）

> **当前状态：** CoPaw 模板已临时隐藏，本用例当前阶段暂不执行。

1. 保留该用例作为后续恢复 Docker 模板后的回归项

**预期：**
- 跳转到"我的实例"页面
- 卡片出现，状态显示 **创建中**（蓝色标签）
- 15 秒后轮询刷新，状态变为 **运行中**（绿色标签）
- 端口显示为 3000+

**日志验证：**
```
Creating agent: CoPaw-1
Pulling Docker image
Agent provisioned successfully
```

**Docker 验证（通过 VM）：**
```bash
limactl shell agentbox -- docker ps
```
预期：看到一个名为 `copaw-1` 的容器在运行。

### TC-4.2 停止 Agent

> **当前状态：** CoPaw 模板已临时隐藏，本用例当前阶段暂不执行。

**预期：**
- 状态变为 **已停止**（灰色标签）
- `limactl shell agentbox -- docker ps -a` 显示该容器为 Exited

### TC-4.3 启动 Agent

> **当前状态：** CoPaw 模板已临时隐藏，本用例当前阶段暂不执行。

**预期：** 状态恢复为 **运行中**

### TC-4.4 删除 Agent

> **当前状态：** CoPaw 模板已临时隐藏，本用例当前阶段暂不执行。

**预期：**
- 卡片从列表消失
- `limactl shell agentbox -- docker ps -a` 中不再有该容器

### TC-4.5 创建 Agent（Native 方式）

1. 部署 **OpenClaw**
2. 观察后台日志中是否出现：
   - `Running native install for agent`
   - `curl -fsSL https://openclaw.ai/install.sh | bash`
   - `Agent provisioned successfully`

> **注意：** Native 方式直接在 VM 内通过 curl 安装，无需 Docker，首次安装视网速需 1-3 分钟。

**预期：** 创建成功，状态最终变为运行中。首个 OpenClaw 实例默认使用 18789 端口。

**VM 内验证：**
```bash
limactl shell agentbox -- ps aux | grep openclaw
```
预期：看到 `openclaw gateway` 进程在运行。

### TC-4.6 创建多个 OpenClaw 实例

1. 连续部署 3 个 OpenClaw 实例
2. 等待全部进入运行中状态

**预期：**
- 第 1 个实例端口为 `18789`
- 第 2 个实例端口为 `18790`
- 第 3 个实例端口为 `18791`
- 实例之间不会复用同一个端口

### TC-4.7 多实例配置隔离

1. 分别打开 2 个 OpenClaw 实例的配置页
2. 给它们填写不同的 provider、model 或 API Key
3. 对其中一个实例点击"应用配置"

**预期：**
- 当前实例会按新配置重启
- 另一个 OpenClaw 实例保持运行，不会被一起重启
- 两个实例刷新后仍显示各自独立的配置值

### TC-4.8 多实例 Dashboard 隔离

1. 分别从 2 个 OpenClaw 实例卡片点击打开控制面板
2. 观察浏览器地址和页面状态

**预期：**
- 两个实例都能成功打开 Dashboard
- 地址中的端口分别对应各自实例端口
- 不会出现 token 串用、跳到同一实例、或打开后直接未授权的情况

### TC-4.9 多实例停止与控制台隔离

1. 启动 2 个以上 OpenClaw 实例
2. 停止其中一个实例
3. 再分别打开剩余实例的控制台或 WebShell

**预期：**
- 被停止的实例状态变为已停止
- 其它 OpenClaw 实例继续保持运行中
- 打开的控制台进入的是对应实例环境，不会串到其它实例

---

## 阶段五：Agent 配置

### TC-5.1 查看配置

1. 在 Agent 卡片上点击齿轮 ⚙️ 图标

**预期：** 跳转到 `/config/:id` 页面，显示该模板的配置表单：
- CoPaw：LLM 服务商（下拉）、API 密钥（密码框）等
- OpenClaw：LLM 服务商（下拉）等

### TC-5.2 保存配置

1. 填入一个配置值（例如 API 密钥随便填 `test-key-123`）
2. 点击保存

**预期：** 保存成功提示。刷新页面后值仍在。

### TC-5.3 应用配置（重建容器）

1. 点击"应用配置"按钮

**预期：**
- 后台: 停止旧容器 → 删除 → 用新 env 重新创建
- 日志出现环境变量注入信息
- Agent 重新进入运行中状态

---

## 阶段六：日志与监控

### TC-6.1 查看日志

1. 在 Agent 卡片上点击日志 📜 图标（或点击 Agent 名称）

**预期：** 跳转到 `/agent/:id`，默认显示"日志"标签页：
- 黑底白字日志输出区域
- 顶部有行数选择（100/500/1000/5000）
- 可手动刷新、复制日志
- 如果 Agent 在运行中，5 秒自动刷新

### TC-6.2 查看监控

1. 切换到"监控"标签页

**预期：** 显示（需要等待至少 30 秒让 metrics collector 收集一次数据）：
- 统计卡片：CPU、内存、网络收发
- 折线图（SVG）
- 数据表格

### TC-6.3 监控数据累积

等待 2-3 分钟，再次查看监控页面。

**预期：** 图表和表格中有多个数据点，能看到趋势。

---

## 阶段七：导出 / 导入 / 升级

### TC-7.1 导出备份

1. 在 Agent 卡片上点击下载 ↓ 图标

**预期：**
- 弹窗显示备份路径（`~/.agentbox/backups/xxx.tar.gz`）
- 验证文件存在：`ls ~/.agentbox/backups/`

### TC-7.2 升级 Agent

1. 在 Agent 卡片上点击升级 ↑ 图标
2. 确认对话框

**预期：**
- 旧 Agent 被标记为 STOPPED（或已归档）
- 新 Agent 出现，名称为 `xxx-2`，instance_no 增加
- 新 Agent 最终状态为 RUNNING

### TC-7.3 查看备份列表

在后续版本中可通过 API 验证：

```bash
# 在开发者工具 Console 中
window.__TAURI__.core.invoke('list_agent_backups', { id: '<agent-id>' })
```

---

## 阶段八：自启动

### TC-8.1 设置自启动

1. 点击 Agent 卡片上的电源图标切换为蓝色（开启自启动）

**预期：** 卡片信息行显示"开机自启：已开启"

### TC-8.2 验证自启动

1. 先停止该 Agent
2. 关闭整个 AgentBox 应用
3. 重新启动：`cargo tauri dev`

**预期：** 环境初始化完成后，该 Agent 自动变为 RUNNING。日志中出现 `Auto-starting agent`。

---

## 阶段九：网络隔离（手动验证）

### TC-9.1 确认 agentbox-net 网络

```bash
limactl shell agentbox -- docker network inspect agentbox-net
```

**预期：** 网络存在，Driver 为 bridge。

### TC-9.2 确认容器在隔离网络

```bash
limactl shell agentbox -- docker inspect <container-name> | grep -A5 Networks
```

**预期：** 容器连接到 `agentbox-net`，具有 `NET_ADMIN` capability。

---

## 阶段十：错误恢复

### TC-10.1 VM 被意外删除

```bash
limactl delete agentbox --force
cargo tauri dev
```

**预期：**
- 日志显示 `正在创建虚拟环境...`（重新创建）
- 初始化完成后正常可用
- 注意：之前的容器数据会丢失

### TC-10.2 创建失败的 Agent

尝试部署一个不存在的模板（可临时修改前端测试）或直接调用：

```js
// 开发者工具 Console
window.__TAURI__.core.invoke('create_agent', { name: 'test', template: 'nonexistent' })
```

**预期：** 返回错误信息，不会崩溃。数据库中不留脏数据。

---

## 测试结果记录模板

| 编号 | 测试项 | 通过/失败 | 备注 |
|------|--------|-----------|------|
| TC-1.1 | Rust 单元测试 | | |
| TC-1.2 | 前端编译检查 | | |
| TC-2.1 | Lima 自动安装 | | |
| TC-2.2 | 再次启动快速路径 | | |
| TC-2.3 | VM 关闭后恢复 | | |
| TC-3.1 | 模板列表加载 | | |
| TC-3.2 | 模板详情验证 | | |
| TC-4.1 | 创建 Agent (docker) | | |
| TC-4.2 | 停止 Agent | | |
| TC-4.3 | 启动 Agent | | |
| TC-4.4 | 删除 Agent | | |
| TC-4.5 | 创建 Agent (native) | | |
| TC-4.6 | 创建多个 OpenClaw 实例 | | |
| TC-4.7 | 多实例配置隔离 | | |
| TC-4.8 | 多实例 Dashboard 隔离 | | |
| TC-4.9 | 多实例停止与控制台隔离 | | |
| TC-5.1 | 查看配置 | | |
| TC-5.2 | 保存配置 | | |
| TC-5.3 | 应用配置 | | |
| TC-6.1 | 查看日志 | | |
| TC-6.2 | 查看监控 | | |
| TC-6.3 | 监控数据累积 | | |
| TC-7.1 | 导出备份 | | |
| TC-7.2 | 升级 Agent | | |
| TC-7.3 | 查看备份列表 | | |
| TC-8.1 | 设置自启动 | | |
| TC-8.2 | 验证自启动 | | |
| TC-9.1 | agentbox-net 网络 | | |
| TC-9.2 | 容器隔离网络 | | |
| TC-10.1 | VM 被删除恢复 | | |
| TC-10.2 | 创建失败容错 | | |
| TC-11.1 | macOS 产物检查 | | |
| TC-11.2 | Windows 安装包检查 | | |
| TC-11.3 | 打包忽略规则检查 | | |

---

## 阶段十一：安装包验证

### TC-11.1 macOS 产物检查

```bash
cd apps/desktop
pnpm package:mac
```

**预期：**
- `apps/desktop/src-tauri/target/release/bundle/app/` 下生成 `AgentBox.app`
- `apps/desktop/src-tauri/target/release/bundle/dmg/` 下生成 `.dmg`
- 双击 `.app` 后可以正常进入主界面

### TC-11.2 Windows 安装包检查

在 Windows 环境执行：

```bash
cd apps/desktop
pnpm install
pnpm package:windows
```

**预期：**
- `bundle/nsis/` 下生成 `.exe`
- `bundle/msi/` 下生成 `.msi`
- 安装后应用可以正常启动、创建实例、打开监控页

### TC-11.3 打包忽略规则检查

确认以下内容不会进入 Tauri 打包监听范围：
- `docs/`
- 任意 `agent.md`
- 任意以 `.` 开头的目录

可检查根目录 `.taurignore`。
