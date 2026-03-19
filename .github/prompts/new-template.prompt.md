---
description: "创建新的 Agent 模板：生成 agent.yaml、Dockerfile、install.sh、run.sh 四件套"
agent: "agent"
argument-hint: "模板名称（如 my-bot）"
---
在 `templates/` 目录下创建一个新的 Agent 模板。模板目录结构必须包含以下 4 个文件：

## 需要创建的文件

### 1. `templates/{{name}}/agent.yaml`
参考 [templates/copaw/agent.yaml](../../templates/copaw/agent.yaml) 的格式：
- `name`: 模板显示名称
- `description`: 一句话描述
- `version`: "1.0"
- `runtime.docker`: true, `runtime.image`: `agentbox/{{name}}:latest`
- `ports`: 服务端口列表
- `env`: 需要的环境变量名（如 API key）
- `resources`: cpus, memory_mb, disk_gb
- `health.url`: 健康检查 URL, `health.interval_secs`: 30

### 2. `templates/{{name}}/Dockerfile`
基于 Python 3.11，安装 iptables 和 curl，复制 install.sh 和 run.sh 并执行：
```dockerfile
FROM python:3.11-slim
RUN apt-get update && apt-get install -y iptables curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY install.sh run.sh ./
RUN chmod +x install.sh run.sh && ./install.sh
CMD ["./run.sh"]
```

### 3. `templates/{{name}}/install.sh`
依赖安装脚本（pip install 等），以 `#!/bin/bash` 开头，`set -e`。

### 4. `templates/{{name}}/run.sh`
入口脚本，先应用 iptables 网络隔离规则（如果 AGENTBOX_IPTABLES_RULES 环境变量存在），然后 exec 启动主服务：
```bash
#!/bin/bash
set -e
if [ -n "$AGENTBOX_IPTABLES_RULES" ]; then
    eval "$AGENTBOX_IPTABLES_RULES"
fi
exec <启动命令>
```

询问用户模板的用途、端口、环境变量等信息后再生成。
