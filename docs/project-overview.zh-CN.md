# 项目总览

[English Version](./project-overview.md)

这份文档用于从“项目层面”理解 `ctx-cache-compressor`，而不仅仅是从代码结构理解它。

## 1. 产品边界

这个项目的核心产品是：

`按 session 管理上下文缓存 + 异步压缩历史消息`

服务持续接收消息，检测安全的 turn 边界，在后台压缩较老的历史，并始终返回一份可继续推理的合并上下文视图。

也可以把它简单理解成：

`面向 LLM 的 Redis 风格上下文缓存 + 压缩器`

它的核心产品不是“聊天补全”。聊天补全只通过 demo 辅助接口提供。

## 2. 项目分层

### A 层：压缩核心服务

负责：

- 内存中存消息
- 校验 role 转移是否合法
- 检测完整 turn
- 判断何时允许压缩
- 异步调度压缩任务
- 返回当前合并后的上下文

关键模块：

- `src/session/`
- `src/compression/`
- `src/llm/`

### B 层：Demo Runtime 层

负责：

- 查看和修改运行时配置
- 为 demo 提供便利聊天流程
- 暴露更适合 playground 使用的接口

关键模块：

- `src/runtime.rs`
- `src/api/handlers/demo.rs`

### C 层：Demo UI 层

负责：

- 让服务行为可观测
- 展示上下文增长与压缩过程
- 提供轻量操作台

关键文件：

- `static/ctx-cache-compressor-playground.html`
- `static/dashboard.html`
- `static/playground-example.html`

## 3. 最关键的内部模型

每个 session 被拆成两块：

- `stable`：当前已确认上下文
- `pending`：压缩进行中新增的消息

核心不变式是：

`完整上下文 = stable + pending`

这个设计带来的好处是：

- `append` 不阻塞
- `fetch` 不阻塞
- 后台压缩不会丢失进行中的新消息

具体运行方式：

- 普通追加写入 `stable`
- 触发压缩时，服务对 `stable` 做快照，并把 session 标记为压缩中
- 压缩期间的新消息写入 `pending`
- fetch 始终返回 `stable + pending`
- 压缩成功后，用 `[summary] + 最近原文轮次` 替换旧的 `stable` 快照，再把 `pending` 合并回 `stable`
- 压缩失败时，保留旧 `stable`，并把 `pending` 合并回去
- 如果后台压缩运行期间 `stable` 被其他路径改动，过期压缩结果会被丢弃，避免覆盖新状态

fetch API 在任何时刻都返回完整的规范化上下文：

- 压缩前：原始详细轮次
- 压缩中：旧的原始 `stable` 快照 + 详细 `pending` 消息
- 压缩成功后：较早轮次的 `[CONTEXT SUMMARY]` + 最近详细轮次 + 已合并的详细 pending 消息
- 压缩失败或结果被丢弃后：旧的详细 `stable` + 已合并的详细 `pending`

## 4. 路由分组

### 核心接口

- `POST /sessions`
- `GET /sessions`
- `DELETE /sessions/{session_id}`
- `POST /sessions/{session_id}/messages`
- `GET /sessions/{session_id}/context`
- `GET /health`

### Demo 接口

- `GET /demo/config`
- `PATCH /demo/config`
- `POST /demo/chat`
- `POST /demo/tool-call`
- `POST /demo/complete`

### UI 页面

- `/compressor`
- `/ex/dashboard`
- `/ex/playground`

兼容二进制里的 Demo 与 UI 路由由 `server.enable_demo_routes` 控制。
生产部署通常应运行 `ctx-cache-compressor-api`，它只暴露核心 API。
`/demo/tool-call` 接收 OpenAI 兼容的 `tools` 数组，用于 playground tool 模拟。
`/demo/complete` 在手动追加 tool 结果后继续生成最终 assistant 回复。

可部署入口：

- `ctx-cache-compressor`：兼容服务，核心 API 加可选 demo/UI
- `ctx-cache-compressor-api`：生产 API-only 服务
- `ctx-cache-compressor-demo`：本地 demo/display 服务

## 5. 推荐理解方式

如果你把 `ctx-cache-compressor` 接入自己的应用，推荐用这种心智模型：

1. 你的应用负责产品行为和最终回答
2. `ctx-cache-compressor` 负责上下文状态和压缩

因此最干净的生产调用链应该是：

1. append user 消息
2. fetch 当前 context
3. 用这份 context 调你自己的 LLM
4. append assistant 消息

## 6. 当前开源准备度

这个仓库目前已经具备：

- 可运行实现
- 较强的集成测试覆盖
- 打包与部署辅助
- 多个 demo 页面
- 面向 operator 的文档

在正式面向更广泛开源使用前，最值得继续补强的是：

- 许可证与仓库元信息
- API 版本化说明
- benchmark 报告
- 外部应用集成示例

## 7. 文档阅读路径

建议先读：

- [README](../README.zh-CN.md)
- [API & Observability Map](./api-observability-map.md)

这份文档适合在你需要快速建立“整个项目怎么分层、怎么协作”认知时使用。
