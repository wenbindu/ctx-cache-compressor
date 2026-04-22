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

### UI 页面

- `/compressor`
- `/ex/dashboard`
- `/ex/playground`

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
