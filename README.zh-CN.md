# ctx-cache-compressor

[English README](./README.md)

> 面向长对话 LLM 场景的“Redis 风格上下文缓存 + 异步历史压缩”服务。

![ctx-cache-compressor 压缩演示页](./docs/sample.png)

`ctx-cache-compressor` 是一个 Rust 服务，用来按 session 保存会话上下文、持续返回可用的合并视图，并通过 OpenAI 兼容接口在后台压缩较早历史消息。

最短理解方式是：

- 你的应用负责最终助手行为
- `ctx-cache-compressor` 负责上下文状态、缓存与压缩

所以它很适合放在你的聊天应用和上游模型之间，作为一个轻量的上下文基础设施层。

## 为什么做这个项目

长对话通常会反复遇到这些问题：

- prompt 随轮次线性增长
- 成本和延迟持续上升
- 较早但重要的上下文越来越难被模型稳定利用

`ctx-cache-compressor` 的做法是把“上下文状态”本身抽成一个服务：

- 按 session 存储消息
- 支持 `system`、`user`、`assistant`、`tool` 等角色安全追加
- 只在完整 turn 边界上触发压缩
- 保留最近几轮原文
- 后台异步压缩较早历史
- 对外始终返回一份可继续推理的合并上下文

## 这个项目是什么

`ctx-cache-compressor` 是：

- 一个面向 LLM 对话上下文的内存型 session 存储层
- 一个理解 turn 边界的压缩调度器
- 一个通过 OpenAI 兼容接口做摘要压缩的服务
- 一个可放在主模型前面的上下文缓存层

`ctx-cache-compressor` 不是：

- 完整聊天产品
- 数据库型长期记忆平台
- 向量数据库或 RAG 系统
- agent runtime

仓库里虽然包含 demo 路由和页面，但核心产品始终是“压缩 + 缓存”服务本身。

## 核心心智模型

每个 session 都拆成两个缓冲区：

- `stable`：当前已确认的上下文快照
- `pending`：压缩进行中时新追加的消息

核心不变式：

`full context = stable + pending`

这也是它最重要的行为基础：

- `append` 不等待压缩完成
- `fetch` 不等待压缩完成
- 压缩只处理快照
- 压缩失败也不会丢消息，而是无损降级

## Turn 感知压缩

只有在安全边界上才允许压缩：

- 简单轮：`user -> assistant`
- tool 轮：`user -> assistant(tool_calls) -> tool... -> assistant`

达到阈值后，服务会把较早窗口压成一条以 `[CONTEXT SUMMARY]` 为前缀的摘要消息，同时保留最新几轮原文不压缩。

## 路由分层

### 核心服务接口

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

`/demo/chat` 是给演示页用的便利接口，不是最纯粹的压缩契约。它会追加 user 消息、调用上游聊天模型、追加 assistant 回复，并返回更新后的 context。

### UI 页面

- `/compressor`
- `/ex/dashboard`
- `/ex/playground`

`/compressor` 页面默认英文，并内置 `EN / 中文` 切换。

## 推荐的生产接入方式

在真实业务里，推荐这样接：

1. 创建 session
2. append user 消息
3. fetch 当前 context
4. 用这份 context 调你自己的 LLM
5. append assistant 消息
6. 循环往复

也就是说：

- 你的业务系统负责产品行为和最终回答
- `ctx-cache-compressor` 负责上下文状态、缓存和压缩

## 快速开始

### 依赖

- Rust stable
- `cargo`
- 一个 OpenAI 兼容上游接口
- 对应 provider 的 API key

### 本地环境流程

先准备本地配置和本地环境文件：

```bash
cp config.example.toml config.toml
cp .env.example .env.local
```

编辑 `.env.local`，统一填入 `OPENAI_API_KEY`，用于任意兼容 OpenAI 协议的上游模型服务。

然后把它加载到当前 shell：

```bash
source scripts/source_env.sh .env.local
```

它会导出这些变量：

- `OPENAI_API_KEY`
- `CTX_CACHE_COMPRESSOR_CONFIG_FILE`

### 本地运行

配置文件和环境文件准备好后：

```bash
cargo run
```

如果你更喜欢临时 `export`，也可以直接这样跑：

```bash
export OPENAI_API_KEY="your-api-key"
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

### 指定配置文件运行

```bash
export OPENAI_API_KEY="your-api-key"
CTX_CACHE_COMPRESSOR_CONFIG_FILE=deploy/config/prod.toml cargo run --release
```

## 配置说明

`config.toml` 被刻意加入 `.gitignore`，目的是避免把本地密钥提交到 Git。推荐从 `config.example.toml` 开始，并通过环境变量传入 key。

配置加载顺序：

1. 代码内默认值
2. 配置文件
   - 若设置了 `CTX_CACHE_COMPRESSOR_CONFIG_FILE`，优先加载它
   - 否则在存在时加载仓库根目录的 `config.toml`
3. `CTX_CACHE_COMPRESSOR__...` 环境变量覆盖
4. 当 `llm.api_key` 为空时，自动走统一 API key 环境变量兜底

API key 兜底顺序：

- `OPENAI_API_KEY`

可直接参考的配置文件：

- `.env.example`
- `config.example.toml`
- `deploy/config/prod.toml`
- `deploy/config/prod-1000.toml`
- `deploy/systemd/ctx-cache-compressor.env.example`

## 仓库结构

```text
ctx-cache-compressor/
├── src/
│   ├── api/               # HTTP DTO、handler、route
│   ├── compression/       # 压缩计划、prompt、调度器
│   ├── llm/               # OpenAI 兼容客户端
│   ├── session/           # session 类型、校验、turn 逻辑、store
│   ├── config.rs          # 配置加载
│   ├── error.rs           # 错误模型
│   ├── lib.rs
│   └── main.rs
├── static/                # demo 与 playground 页面
├── deploy/                # 部署配置与 systemd 模板
├── scripts/               # smoke、打包、本地辅助脚本
├── tests/                 # 集成测试
└── docs/                  # 架构与运维文档
```

## API 使用示例

### 创建 Session

```bash
curl -sS -X POST http://127.0.0.1:8080/sessions \
  -H 'content-type: application/json' \
  -d '{"system_prompt":"You are a concise assistant."}'
```

### 追加用户消息

```bash
curl -sS -X POST http://127.0.0.1:8080/sessions/<session-id>/messages \
  -H 'content-type: application/json' \
  -d '{"role":"user","content":"Summarize what we decided so far."}'
```

### 追加助手消息

```bash
curl -sS -X POST http://127.0.0.1:8080/sessions/<session-id>/messages \
  -H 'content-type: application/json' \
  -d '{"role":"assistant","content":"Here is the current summary..."}'
```

### 获取当前上下文

```bash
curl -sS http://127.0.0.1:8080/sessions/<session-id>/context | jq .
```

### 列出当前会话

```bash
curl -sS http://127.0.0.1:8080/sessions | jq .
```

### 删除 Session

```bash
curl -i -X DELETE http://127.0.0.1:8080/sessions/<session-id>
```

### Demo Chat 便利接口

这个接口适合 playground 和手工快速验证：

```bash
curl -sS -X POST http://127.0.0.1:8080/demo/chat \
  -H 'content-type: application/json' \
  -d '{"user_message":"Explain what ctx-cache-compressor does."}' | jq .
```

## 部署

构建发布包：

```bash
scripts/package_release.sh
```

使用显式配置启动 release 二进制：

```bash
cargo build --release
CONFIG_FILE=deploy/config/prod.toml scripts/run_release.sh
```

如果仓库根目录存在 `.env.local`，可直接用后台脚本启动开发实例：

```bash
scripts/start_bg.sh
scripts/stop_bg.sh
```

使用 systemd 部署：

1. 将发布文件或 release 构建结果复制到 `/opt/ctx-cache-compressor`
2. 将 `deploy/config/prod.toml` 复制到 `/etc/ctx-cache-compressor/prod.toml`
3. 将 `deploy/systemd/ctx-cache-compressor.env.example` 复制到 `/etc/ctx-cache-compressor/ctx-cache-compressor.env`
4. 在该 env 文件中填入 provider API key
5. 安装 `deploy/systemd/ctx-cache-compressor.service`
6. 执行 `systemctl enable --now ctx-cache-compressor`

构建 Docker 镜像：

```bash
docker build -t ctx-cache-compressor:local .
```

通过 env 文件运行：

```bash
docker run --rm -p 8080:8080 \
  --env-file .env.local \
  -v "$(pwd)/config.example.toml:/app/config.toml:ro" \
  ctx-cache-compressor:local
```

## 测试

运行主测试集：

```bash
cargo test
```

项目辅助测试：

```bash
scripts/test_suite.sh quick
scripts/test_suite.sh load-1000
scripts/test_suite.sh all
```

其他常用检查：

- `scripts/smoke.sh`
- `scripts/load_test.sh`

当前自动化覆盖包括：

- role 序列合法性
- tool call 链路完整性
- turn 边界判断
- 压缩成功路径
- 压缩失败时的无损降级
- 压缩中的 append / fetch
- TTL 清理
- 并发 session append 场景

## 文档

- [项目总览](./docs/project-overview.zh-CN.md)
- [API 与可观测性地图](./docs/api-observability-map.md)
- [English README](./README.md)
- [英文项目总览](./docs/project-overview.md)

## 开源说明

当前仓库的公开发布假设是：

- 本地密钥放在忽略文件或环境变量里
- replay / eval 等生成物不进入 Git 历史
- `AGENTS.md` 是公开可读的贡献说明，而不是私有工作区导出

正式发布到 GitHub 前，仍有一项需要维护者明确决定：

- 选择并加入最终的 `LICENSE`
