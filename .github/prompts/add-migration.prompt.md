---
description: "创建 SQLite 数据库迁移并同步更新 Rust struct 和 TypeScript 类型定义"
agent: "agent"
argument-hint: "描述要做的 schema 变更（如：给 agents 表加 tags 字段）"
---
执行数据库 schema 变更，需要同步修改以下三处：

## 步骤

### 1. 创建迁移文件
在 `apps/desktop/src-tauri/migrations/` 下创建新的 SQL 迁移文件。

命名格式：`YYYYMMDDHHMMSS_描述.sql`（如 `20260309120000_add_tags.sql`）。

参考已有迁移 [20260305000001_initial.sql](../../apps/desktop/src-tauri/migrations/20260305000001_initial.sql) 的风格。

SQLite 注意事项：
- 不支持 `ALTER TABLE ... DROP COLUMN`（3.35.0 之前）
- 不支持 `ALTER TABLE ... ADD CONSTRAINT`
- 新增列用 `ALTER TABLE ... ADD COLUMN`，必须带默认值或允许 NULL

### 2. 更新 Rust struct
在 `apps/desktop/src-tauri/src/commands.rs` 中找到对应的 struct（如 `AgentInfo`），添加新字段。确保：
- 使用 `#[serde(Serialize, Deserialize)]`
- 可选字段用 `Option<T>`
- 更新相关的 `sqlx::query_as!` 或 `sqlx::query!` 查询

### 3. 更新 TypeScript 接口
在 `apps/desktop/src/lib/api.ts` 中更新对应的 TypeScript interface，字段名和类型必须与 Rust struct **完全匹配**。

### 4. 验证
运行 `cargo check` 确保 Rust 编译通过，运行 `cd apps/desktop && pnpm test` 确保前端测试通过。
