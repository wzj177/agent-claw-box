FROM node:22-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    iptables curl git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 创建数据目录
RUN mkdir -p /root/.openclaw/workspace

COPY install.sh run.sh /app/
RUN chmod +x /app/install.sh /app/run.sh

# 安装 OpenClaw CLI
RUN /app/install.sh

EXPOSE 18789

HEALTHCHECK --interval=30s --timeout=10s --retries=3 \
  CMD curl -f http://localhost:18789/ || exit 1

CMD ["/app/run.sh"]
