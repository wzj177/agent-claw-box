# nanobot 使用 Docker Compose 部署，此 Dockerfile 仅作自定义构建备用
FROM ghcr.io/astral-sh/uv:python3.12-bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    iptables curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /root/.nanobot

RUN uv pip install --system nanobot-ai

COPY run.sh /app/run.sh
RUN chmod +x /app/run.sh

EXPOSE 18790

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD curl -f http://localhost:18790/ || exit 1

CMD ["/app/run.sh"]
