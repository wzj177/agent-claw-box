#!/bin/bash
set -e

# 应用网络隔离规则
if command -v iptables &> /dev/null; then
  iptables -A OUTPUT -d 172.17.0.1 -j DROP 2>/dev/null || true
  iptables -A OUTPUT -d 10.0.0.0/8 -j DROP 2>/dev/null || true
  iptables -A OUTPUT -d 172.16.0.0/12 -j DROP 2>/dev/null || true
  iptables -A OUTPUT -d 192.168.0.0/16 -j DROP 2>/dev/null || true
fi

# 初始化配置（如果首次启动）
if [ ! -f /root/.nanobot/config.json ]; then
  nanobot onboard 2>/dev/null || true
fi

exec nanobot gateway
