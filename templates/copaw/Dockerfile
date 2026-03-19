# CoPaw 使用官方 Docker 镜像，此 Dockerfile 仅作自定义构建备用
# 正常部署通过 docker pull agentscope/copaw:latest
FROM agentscope/copaw:latest

# 安装 iptables 用于网络隔离
RUN apt-get update && apt-get install -y --no-install-recommends iptables \
    && rm -rf /var/lib/apt/lists/*

COPY run.sh /app/run.sh
RUN chmod +x /app/run.sh

EXPOSE 8088

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD curl -f http://localhost:8088/ || exit 1

CMD ["/app/run.sh"]
