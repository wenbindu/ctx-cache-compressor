# ctx-cache-compressor

[English README](./README.md)

> 面向长对话 OpenAI 兼容 LLM 场景的“Redis 风格上下文缓存 + 异步历史压缩”服务。

![ctx-cache-compressor 压缩演示页](./docs/sample.png)

`ctx-cache-compressor` 是一个 Rust 服务，用来按 session 保存会话上下文、返回实时合并视图，并通过 OpenAI 兼容接口在后台压缩较早历史消息。

最短理解方式：

- 你的应用负责助手行为和产品逻辑
- `ctx-cache-compressor` 负责上下文存储、缓存和历史压缩

## 它解决什么问题

- 按 session 存储 `system`、`user`、`assistant`、`tool` 消息
- 维护一份非阻塞的合并上下文视图：`stable + pending`
- 只在安全的 turn 边界上触发压缩
- 保留最近几轮原文，异步压缩更早历史
- 同时提供核心服务 API 和内置 `/compressor` 演示页

## 它不是什么

- 不是完整聊天产品
- 不是数据库型长期记忆平台
- 不是向量数据库或 RAG 系统
- 不是 agent runtime

## 核心模型

每个 session 有两个缓冲区：

- `stable`：当前已确认的上下文快照
- `pending`：压缩进行中时新追加的消息

核心不变式：

`full context = stable + pending`

因此这个服务具备几个关键特性：

- `append` 不等待压缩
- `fetch` 不等待压缩
- 压缩只处理快照
- 压缩失败时无损降级，不丢消息

压缩只会发生在完整 turn 边界上：

- 简单轮：`user -> assistant`
- tool 轮：`user -> assistant(tool_calls) -> tool... -> assistant`

## 快速开始

依赖：

- Rust stable
- `cargo`
- 一个 OpenAI 兼容上游接口
- 对应的 API key

先准备本地配置：

```bash
cp config.example.toml config.toml
cp .env.example .env.local
```

在 `.env.local` 里设置 `OPENAI_API_KEY`，然后加载：

```bash
source scripts/source_env.sh .env.local
```

本地启动：

```bash
cargo run
```

健康检查：

```bash
curl -sS http://127.0.0.1:8080/health | jq .
```

打开演示页：

```text
http://127.0.0.1:8080/compressor
```

## 核心路由

- `POST /sessions`
- `GET /sessions`
- `DELETE /sessions/{session_id}`
- `POST /sessions/{session_id}/messages`
- `GET /sessions/{session_id}/context`
- `GET /health`

Demo 辅助接口：

- `GET /demo/config`
- `PATCH /demo/config`
- `POST /demo/chat`

UI 页面：

- `/compressor`
- `/ex/dashboard`
- `/ex/playground`

## 推荐生产接入方式

1. 创建 session
2. 追加 user 消息
3. 获取合并后的 context
4. 用这份 context 调你自己的 LLM
5. 追加 assistant 消息
6. 循环往复

这个服务应该位于你的应用和上游模型之间。它负责上下文状态，你的应用仍然负责最终回答行为。

## 部署

构建并运行 release 二进制：

```bash
cargo build --release
CONFIG_FILE=deploy/config/prod.toml scripts/run_release.sh
```

为当前或指定目标平台生成发布包：

```bash
scripts/package_release.sh
TARGET=x86_64-unknown-linux-gnu scripts/package_release.sh
```

归档文件名会带上 Rust target triple，例如：

```text
ctx-cache-compressor-0.1.0-x86_64-unknown-linux-gnu.tar.gz
```

仓库里已包含的其他部署路径：

- Docker: [Dockerfile](./Dockerfile)
- systemd: [deploy/systemd/ctx-cache-compressor.service](./deploy/systemd/ctx-cache-compressor.service)
- 生产配置: [deploy/config/prod.toml](./deploy/config/prod.toml)

## Release 策略

这是一个后端服务，不是桌面应用。所以：

- 默认不需要 Windows 或 macOS 安装包
- 不需要做 GUI 安装器那套按平台分发
- 只有当你要直接分发服务二进制时，才需要区分平台构建

比较合适的 GitHub Releases 策略是：

- 始终发布带 tag 的源码 release
- 如果真实部署目标是 Linux，优先发布 Linux tar.gz
- 如果主要通过容器部署，再补一个 Docker image
- 只有在维护者或用户经常本地运行时，再补 macOS 二进制
- 没有真实需求前，不必做 Windows 安装包

对大多数团队来说，`Linux binary + Docker image + source tag` 就足够了。

仓库现在也包含一个 GitHub Actions workflow：当你推送 `v0.1.0` 这类 tag 时，会自动构建并发布 `x86_64-unknown-linux-gnu` 的 Linux release 产物。

## 测试

运行主测试集：

```bash
cargo test
```

常用辅助脚本：

```bash
scripts/test_suite.sh quick
scripts/smoke.sh
```

## 文档

- [项目总览](./docs/project-overview.zh-CN.md)
- [API 与可观测性地图](./docs/api-observability-map.md)
- [English README](./README.md)
