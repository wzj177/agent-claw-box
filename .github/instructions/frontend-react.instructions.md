---
description: "Use when writing or modifying React frontend code: components, pages, API layer, styling. Covers Chinese UI strings, type contracts with Rust backend, Tailwind component classes, and routing."
applyTo: "apps/desktop/src/**/*.{ts,tsx}"
---
# Frontend React Conventions

## 中文 UI
所有用户可见的字符串使用**简体中文**（zh-CN）。不要使用英文 label、placeholder 或提示文案。

## API 类型契约
- 所有 Tauri invoke 调用封装在 `lib/api.ts` 中。
- TypeScript 接口**必须**与 Rust 序列化 struct 字段完全一致（字段名、类型）。
- 新增或修改 Rust struct 字段时，同步更新 `api.ts` 中对应的 interface。

## 样式
- 使用 Tailwind CSS utility classes。
- 按钮使用 `tailwind.css` 中 `@layer components` 定义的组件类：
  - `btn-primary` — 蓝色填充主按钮
  - `btn-default` — 白底描边按钮
  - `btn-text` — 无边框文字按钮
  - `btn-danger-text` — 红色危险操作按钮
- 主色 `#1677ff`，字体栈 PingFang SC → Microsoft YaHei → Helvetica。

## 图标
使用 `lucide-react` 图标库。引入方式：
```tsx
import { Play, Square, Trash2 } from "lucide-react";
```

## 路由
React Router v7，当前路由：
- `/` → AgentsPage（我的 Agent）
- `/marketplace` → MarketplacePage（应用市场）

## 数据刷新
AgentsPage 使用 `setInterval` 每 15 秒轮询 `api.listAgents()`，无 WebSocket。

## 组件模式
- 函数组件 + hooks（`useState`, `useEffect`）。
- Props 使用 inline `interface` 定义类型。
- 无复杂表单库，使用行内校验。
