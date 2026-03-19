下面是一份**可直接用于内部立项 / 技术评审 / 投资人介绍的完整立项文档（PRD + 技术方案）**。
我按你的核心诉求设计：**跨平台、小白操作、安全、网络畅通、VM 管理**。

---

# AgentBox 项目立项文档

## 1. 项目概述

**项目名称**

AgentBox

**项目定位**

AgentBox 是一个跨平台桌面应用，用于**一键创建和运行 AI Agent 虚拟环境（Agent VM）**。
用户无需复杂环境配置，即可快速部署和运行主流 AI Agent 项目。

AgentBox 为 AI Agent 提供：

* 自动环境安装
* VM 沙盒隔离
* 一键部署 Agent
* 网络与资源管理
* 可视化操作界面

目标是成为：

> **AI Agent 的本地运行平台（类似 Docker Desktop for AI Agents）**

---

# 2. 产品目标

AgentBox 的核心目标：

### 1 跨平台运行

支持系统：

* macOS
* Windows 10
* Windows 11
* Windows Server
* Ubuntu Desktop

统一运行体验。

---

### 2 小白用户可用

用户无需：

* 安装 Python
* 配置 Docker
* 配置虚拟机
* 解决依赖冲突

只需要：

```
下载 AgentBox
选择 Agent 模板
点击 Deploy
```

即可运行 Agent。

---

### 3 安全隔离

Agent 在 **Sandbox VM** 中运行。

防止：

* Agent 执行危险代码
* 访问用户文件
* 扫描局域网
* 修改系统

---

### 4 网络畅通

Agent 可以：

* 正常访问互联网
* 调用 API
* 访问 LLM 服务

同时限制：

* 内网扫描
* 主机访问

---

### 5 VM 管理能力

AgentBox 提供：

* 创建 VM
* 启动 VM
* 停止 VM
* 删除 VM
* 资源限制

实现 **Agent 运行环境管理**。

---

# 3. 目标用户

### AI 开发者

使用 AgentBox 快速运行 Agent 项目。

例如：

* AI automation
* Browser agents
* Autonomous coding agents

---

### AI 产品团队

用于：

* 演示 Agent
* 部署本地 Agent
* 快速测试

---

### 普通用户

无需编程能力即可运行 Agent。

---

# 4. 核心功能

AgentBox MVP 功能：

### 1 Agent Marketplace

用户可以选择 Agent 模板：

示例：

```
OpenClaw
CoPaw
CrewAI
AutoGPT
```

用户点击：

```
Deploy
```

即可创建 Agent。

---

### 2 Agent VM 创建

AgentBox 自动：

```
创建 VM
安装 Docker
启动 Agent
```

无需手动操作。

---

### 3 Agent 管理

用户可以：

```
Start Agent
Stop Agent
Restart Agent
Delete Agent
```

查看：

```
Agent Logs
```

---

### 4 网络管理

Agent 默认：

```
允许访问互联网
禁止访问主机
禁止访问局域网
```

确保安全。

---

### 5 资源管理

用户可限制：

```
CPU
Memory
Disk
```

避免 Agent 占满电脑资源。

---

# 5. 技术架构

AgentBox 总体架构：

```
AgentBox Desktop
        │
        ▼
Sandbox VM
        │
        ▼
Docker Runtime
        │
        ▼
Agent Container
```

即：

```
Agent
 inside
Container
 inside
VM
```

双层隔离。

---

# 6. 技术栈

桌面端：

使用
Tauri

原因：

* 跨平台
* 性能高
* 安装包小

UI 技术：

```
React
Tailwind
```

---

VM 运行时：

macOS

使用
Lima

---

Windows

使用
Windows Subsystem for Linux

---

Linux

直接使用：

```
Docker
```

---

容器运行时：

使用
Docker

运行 Agent。

---

# 7. VM 沙盒设计

每个 Agent 在 VM 中运行。

VM 资源：

```
CPU: 2-4
Memory: 4GB
Disk: 20GB
```

VM 内运行：

```
Docker
Agent Container
```

文件系统隔离：

```
/agentbox
   /agents
```

Agent 无法访问：

```
host /Users
host /home
host /Windows
```

---

# 8. 网络策略

默认网络策略：

Agent 可以：

```
访问 Internet
```

Agent 不可以：

```
访问 Host
访问 LAN
```

网络结构：

```
Agent Container
       │
       ▼
Docker Network
       │
       ▼
Internet
```

防止：

```
内网扫描
端口攻击
```

---

# 9. Agent 模板系统

AgentBox 使用模板系统部署 Agent。

模板结构：

```
templates/

   openclaw
   copaw
   crewai
```

模板包含：

```
agent.yaml
dockerfile
install.sh
run.sh
```

示例：

agent.yaml

```
name: OpenClaw

runtime:
  docker: true

ports:
  - 3000

env:
  - OPENAI_API_KEY
```

---

# 10. Agent 生命周期

Agent 状态：

```
CREATING
RUNNING
STOPPED
ERROR
```

生命周期：

```
Create
Start
Stop
Delete
```

---

# 11. Agent 数据管理

AgentBox 使用：

```
SQLite
```

存储：

```
Agent
VM
Ports
Status
```

示例：

```
agents table
```

字段：

```
id
name
template
port
status
created_at
```

---

# 12. 用户界面

首页：

```
My Agents
```

示例：

```
OpenClaw-1
Status: Running
URL: http://localhost:3000
```

操作：

```
Start
Stop
Logs
Delete
```

---

Marketplace：

```
Agent Templates
```

示例：

```
OpenClaw
CoPaw
CrewAI
```

按钮：

```
Deploy
```

---

# 13. 自动环境检测

AgentBox 启动时检测：

```
Docker
WSL
Lima
```

如果缺失：

提示用户安装。

---

# 14. 安全设计

AgentBox 安全机制：

### VM 隔离

Agent 运行在独立 VM。

---

### Container 隔离

Agent 在 Docker container 中运行。

---

### 文件系统隔离

Agent 只能访问：

```
/workspace
```

---

### 网络隔离

限制 Agent：

```
无法访问 Host
无法扫描 LAN
```

---

# 15. MVP 开发计划

预计开发周期：

4 周。

第一周：

```
Tauri UI
模板系统
```

第二周：

```
Docker runtime
Agent 启动
```

第三周：

```
WSL
Lima
VM 管理
```

第四周：

```
Agent Marketplace
日志系统
```

---

# 16. 项目结构

```
agentbox

apps
  desktop

runtime
  vm
  docker

templates

registry

scripts
```

---

# 17. 未来扩展

未来 AgentBox 可以扩展：

### Agent Marketplace

用户发布 Agent 模板。

---

### 云 Agent

支持：

```
Local VM
Remote VM
Cloud Agent
```

---

### 企业版本

企业功能：

```
多用户
权限控制
私有 Agent 市场
```

---

# 18. 项目价值

AgentBox 解决当前 AI Agent 生态的核心问题：

```
部署复杂
环境混乱
安全风险
```

AgentBox 提供：

```
标准化 Agent 运行环境
```

类似：

* Docker Desktop 对容器生态的作用
* LM Studio 对本地 LLM 的作用
