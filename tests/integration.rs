use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use ctx_cache_compressor::{
    api::{routes::build_router, AppState},
    compression::{compressor::Compressor, scheduler::CompressionScheduler},
    config::{
        AppConfig, CompressionConfig, CompressionPromptConfig, LlmConfig, ServerConfig,
        TokenEstimationConfig,
    },
    error::{AppError, AppResult},
    llm::client::{ChatLlm, CompressionLlm},
    runtime::DemoRuntimeConfig,
    session::{
        store::SessionStore,
        types::{Message, MessageContent, Role, ToolCall, ToolFunction},
    },
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::{sync::RwLock, task::JoinSet, time::sleep};
use tower::ServiceExt;

#[derive(Clone)]
enum MockOutcome {
    Success(String),
    Error(String),
}

#[derive(Clone)]
struct MockLlm {
    delay: Duration,
    outcome: MockOutcome,
}

impl CompressionLlm for MockLlm {
    fn compress<'a>(
        &'a self,
        _system_prompt: &'a str,
        _user_prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>> {
        Box::pin(async move {
            if !self.delay.is_zero() {
                sleep(self.delay).await;
            }

            match &self.outcome {
                MockOutcome::Success(text) => Ok(text.clone()),
                MockOutcome::Error(err) => Err(AppError::Upstream(err.clone())),
            }
        })
    }
}

impl ChatLlm for MockLlm {
    fn complete<'a>(
        &'a self,
        _messages: &'a [ctx_cache_compressor::llm::types::ChatMessage],
    ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>> {
        Box::pin(async move {
            if !self.delay.is_zero() {
                sleep(self.delay).await;
            }

            match &self.outcome {
                MockOutcome::Success(text) => Ok(text.clone()),
                MockOutcome::Error(err) => Err(AppError::Upstream(err.clone())),
            }
        })
    }
}

fn make_config(
    every_n_turns: u32,
    keep_recent_turns: u32,
    llm_timeout_seconds: u64,
    max_retries: u32,
    session_ttl_seconds: u64,
) -> Arc<AppConfig> {
    Arc::new(AppConfig {
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            max_sessions: 20_000,
            session_ttl_seconds,
            session_cleanup_interval_seconds: 60,
        },
        compression: CompressionConfig {
            every_n_turns,
            keep_recent_turns,
            llm_timeout_seconds,
            max_retries,
            warn_on_failure: true,
            prompt: CompressionPromptConfig::default(),
        },
        llm: LlmConfig {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: "dummy".to_string(),
            model: "dummy".to_string(),
            max_tokens: 128,
            temperature: 0.0,
        },
        token_estimation: TokenEstimationConfig { chars_per_token: 3 },
    })
}

fn build_test_app(config: Arc<AppConfig>, llm: Arc<MockLlm>) -> (Router, Arc<SessionStore>) {
    let runtime = Arc::new(RwLock::new(DemoRuntimeConfig::from_app_config(&config)));
    let store = Arc::new(SessionStore::new(
        config.server.max_sessions,
        config.server.session_ttl_seconds,
    ));

    let compression_llm: Arc<dyn CompressionLlm> = llm.clone();
    let chat_llm: Arc<dyn ChatLlm> = llm;
    let compressor = Arc::new(Compressor::new(
        compression_llm,
        config.compression.prompt.clone(),
    ));
    let scheduler = Arc::new(CompressionScheduler::new(
        compressor,
        config.compression.every_n_turns,
        config.compression.keep_recent_turns,
        config.compression.llm_timeout_seconds,
        config.compression.max_retries,
        config.compression.warn_on_failure,
    ));

    let state = AppState {
        config,
        runtime,
        store: store.clone(),
        scheduler,
        chat_llm,
    };

    (build_router(state), store)
}

async fn call_json_owned(
    app: Router,
    method: Method,
    path: String,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = match body {
        Some(payload) => Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .expect("request should build"),
        None => Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("request should build"),
    };

    let response = app.oneshot(request).await.expect("request should execute");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should read")
        .to_bytes();

    if bytes.is_empty() {
        return (status, Value::Null);
    }

    let json: Value = serde_json::from_slice(&bytes).expect("json response expected");
    (status, json)
}

async fn call_json(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call_json_owned(app.clone(), method, path.to_string(), body).await
}

async fn call_text(app: &Router, method: Method, path: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request should build");

    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("request should execute");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should read")
        .to_bytes();

    (
        status,
        String::from_utf8(bytes.to_vec()).expect("utf8 body expected"),
    )
}

async fn create_session(app: &Router) -> String {
    let (status, body) = call_json(app, Method::POST, "/sessions", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    body["session_id"]
        .as_str()
        .expect("session id is required")
        .to_string()
}

async fn append_payload(app: &Router, session_id: &str, payload: Value) -> (StatusCode, Value) {
    call_json(
        app,
        Method::POST,
        &format!("/sessions/{session_id}/messages"),
        Some(payload),
    )
    .await
}

async fn append_text(
    app: &Router,
    session_id: &str,
    role: &str,
    content: &str,
) -> (StatusCode, Value) {
    append_payload(
        app,
        session_id,
        json!({
            "role": role,
            "content": content,
        }),
    )
    .await
}

async fn fetch_context(app: &Router, session_id: &str) -> (StatusCode, Value) {
    call_json(
        app,
        Method::GET,
        &format!("/sessions/{session_id}/context"),
        None,
    )
    .await
}

async fn wait_until_compression_finishes(
    store: &Arc<SessionStore>,
    session_id: &str,
    timeout: Duration,
) {
    let start = Instant::now();

    loop {
        let session = store
            .get(session_id)
            .expect("session should exist while waiting for compression");
        let is_compressing = {
            let guard = session.read().await;
            guard.is_compressing.load(Ordering::Relaxed)
        };

        if !is_compressing {
            return;
        }

        assert!(
            start.elapsed() <= timeout,
            "compression did not finish within {timeout:?}"
        );
        sleep(Duration::from_millis(10)).await;
    }
}

fn assistant_tool_call_payload(call_id: &str) -> Value {
    json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": [
            {
                "id": call_id,
                "type": "function",
                "function": {
                    "name": "search",
                    "arguments": "{\"q\":\"rust axum\"}"
                }
            }
        ]
    })
}

async fn assert_concurrent_session_appends(session_count: usize) {
    let config = make_config(100, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("unused".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let mut tasks = JoinSet::new();
    for idx in 0..session_count {
        let app_clone = app.clone();
        tasks.spawn(async move {
            let session_id = format!("session-{idx}");

            let (status, _) = call_json_owned(
                app_clone.clone(),
                Method::POST,
                format!("/sessions/{session_id}/messages"),
                Some(json!({ "role": "user", "content": "hello" })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);

            let (status, _) = call_json_owned(
                app_clone,
                Method::POST,
                format!("/sessions/{session_id}/messages"),
                Some(json!({ "role": "assistant", "content": "world" })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.expect("task should finish cleanly");
    }

    assert_eq!(store.len(), session_count);

    for idx in 0..session_count {
        let session_id = format!("session-{idx}");
        let session = store.get(&session_id).expect("session should exist");
        let guard = session.read().await;
        assert_eq!(guard.turn_count, 1);
    }
}

fn tool_result_payload(call_id: &str) -> Value {
    json!({
        "role": "tool",
        "content": "tool result",
        "tool_call_id": call_id,
        "name": "search"
    })
}

#[tokio::test]
async fn scenario_1_normal_five_turns_trigger_compression_and_reduce_length() {
    let config = make_config(5, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(10),
        outcome: MockOutcome::Success("compressed memory".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;

    let mut triggered = false;
    for idx in 1..=5 {
        let (status, _) = append_text(&app, &session_id, "user", &format!("u{idx}")).await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = append_text(&app, &session_id, "assistant", &format!("a{idx}")).await;
        assert_eq!(status, StatusCode::OK);
        if idx == 5 {
            triggered = body["compression_triggered"]
                .as_bool()
                .expect("compression_triggered should be bool");
        }
    }

    assert!(triggered, "5th turn should trigger compression");
    wait_until_compression_finishes(&store, &session_id, Duration::from_secs(3)).await;

    let (status, body) = fetch_context(&app, &session_id).await;
    assert_eq!(status, StatusCode::OK);

    let messages = body["messages"]
        .as_array()
        .expect("messages should be array");
    assert!(messages.len() < 10, "compressed context should be shorter");
    assert_eq!(
        body["turn_count"]
            .as_u64()
            .expect("turn_count should be number"),
        5
    );

    let has_summary = messages.iter().any(|message| {
        message["role"] == "system"
            && message["content"]
                .as_str()
                .map(|text| text.starts_with("[CONTEXT SUMMARY]"))
                .unwrap_or(false)
    });
    assert!(has_summary, "summary system message should exist");
}

#[tokio::test]
async fn scenario_2_tool_chain_only_triggers_after_final_assistant() {
    let config = make_config(1, 0, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(10),
        outcome: MockOutcome::Success("summary".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;

    let (status, body) = append_text(&app, &session_id, "user", "need search").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], false);

    let (status, body) =
        append_payload(&app, &session_id, assistant_tool_call_payload("call_1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], false);

    let (status, body) = append_payload(&app, &session_id, tool_result_payload("call_1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], false);

    let (status, body) = append_text(&app, &session_id, "assistant", "final answer").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], true);
}

#[tokio::test]
async fn scenario_3_compression_timeout_degrades_and_merges_pending_without_loss() {
    let config = make_config(1, 0, 1, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(1500),
        outcome: MockOutcome::Success("too late summary".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;

    let (status, _) = append_text(&app, &session_id, "user", "u1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = append_text(&app, &session_id, "assistant", "a1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], true);

    let (status, _) = append_text(&app, &session_id, "user", "u2").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = append_text(&app, &session_id, "assistant", "a2").await;
    assert_eq!(status, StatusCode::OK);

    wait_until_compression_finishes(&store, &session_id, Duration::from_secs(4)).await;

    let (status, body) = fetch_context(&app, &session_id).await;
    assert_eq!(status, StatusCode::OK);
    let messages = body["messages"]
        .as_array()
        .expect("messages should be array");
    assert_eq!(messages.len(), 4);

    let has_summary = messages.iter().any(|message| {
        message["role"] == "system"
            && message["content"]
                .as_str()
                .map(|text| text.starts_with("[CONTEXT SUMMARY]"))
                .unwrap_or(false)
    });
    assert!(!has_summary, "failed compression should not inject summary");
}

#[tokio::test]
async fn scenario_4_append_during_compression_goes_to_pending_and_fetch_sees_full_view() {
    let config = make_config(1, 0, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(600),
        outcome: MockOutcome::Success("summary".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;

    let (status, _) = append_text(&app, &session_id, "user", "u1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = append_text(&app, &session_id, "assistant", "a1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], true);

    let (status, _) = append_text(&app, &session_id, "user", "u2").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = append_text(&app, &session_id, "assistant", "a2").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = fetch_context(&app, &session_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["is_compressing"], true);

    let messages = body["messages"]
        .as_array()
        .expect("messages should be array");
    let contents: Vec<&str> = messages
        .iter()
        .filter_map(|message| message["content"].as_str())
        .collect();
    assert!(contents.contains(&"u2"));
    assert!(contents.contains(&"a2"));
}

#[tokio::test]
async fn scenario_5_successful_compression_results_in_summary_plus_pending_and_clears_pending() {
    let config = make_config(1, 0, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(500),
        outcome: MockOutcome::Success("summary body".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;

    let (status, _) = append_text(&app, &session_id, "user", "u1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = append_text(&app, &session_id, "assistant", "a1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], true);

    let (status, _) = append_text(&app, &session_id, "user", "u2").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = append_text(&app, &session_id, "assistant", "a2").await;
    assert_eq!(status, StatusCode::OK);

    wait_until_compression_finishes(&store, &session_id, Duration::from_secs(4)).await;

    let session = store.get(&session_id).expect("session should exist");
    let guard = session.read().await;

    assert!(
        guard.pending.is_empty(),
        "pending must be drained after merge"
    );
    assert_eq!(guard.stable.len(), 3);
    assert!(guard.stable[0].is_context_summary());
    assert_eq!(guard.stable[1].content_text(), "u2");
    assert_eq!(guard.stable[2].content_text(), "a2");
}

#[tokio::test]
async fn scenario_6_concurrent_100_sessions_append_without_deadlock() {
    assert_concurrent_session_appends(100).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "load"]
async fn scenario_6_concurrent_1000_sessions_append_without_deadlock() {
    assert_concurrent_session_appends(1_000).await;
}

#[tokio::test]
async fn scenario_7_ttl_expiration_removes_session_and_later_access_recreates_it() {
    let config = make_config(100, 2, 2, 0, 1);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("unused".to_string()),
    });
    let (app, store) = build_test_app(config, llm);
    store.clone().spawn_ttl_cleanup_with_interval(1);

    let session_id = create_session(&app).await;
    assert!(store.get(&session_id).is_some());

    sleep(Duration::from_secs(3)).await;
    assert!(
        store.get(&session_id).is_none(),
        "session should expire via ttl"
    );

    let (status, _) = append_text(&app, &session_id, "user", "recreated").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        store.get(&session_id).is_some(),
        "session should be recreated on append"
    );
}

#[tokio::test]
async fn scenario_8_tool_call_chain_crossing_stable_and_pending_boundary_is_valid() {
    let config = make_config(1, 0, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("summary".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let session_id = "boundary-session".to_string();
    let session = store
        .get_or_create_with_id(&session_id, 1)
        .expect("session should be created");

    {
        let mut guard = session.write().await;
        guard.stable = vec![
            Message::text(Role::User, "u1"),
            Message {
                role: Role::Assistant,
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: ToolFunction {
                        name: "search".to_string(),
                        arguments: "{\"q\":\"ctx\"}".to_string(),
                    },
                }]),
                tool_call_id: None,
                name: None,
            },
        ];
        guard.pending.clear();
        guard.turn_count = 0;
        guard.next_compress_at = 1;
        guard.is_compressing.store(true, Ordering::SeqCst);
    }

    let (status, body) = append_payload(&app, &session_id, tool_result_payload("call_1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], false);

    let (status, body) = append_text(&app, &session_id, "assistant", "final").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["turn_count"], 1);
    assert_eq!(body["compression_triggered"], false);
}

#[tokio::test]
async fn scenario_9_fetch_during_compression_returns_quickly_without_waiting_llm() {
    let config = make_config(1, 0, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(900),
        outcome: MockOutcome::Success("summary".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;

    let (status, _) = append_text(&app, &session_id, "user", "u1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = append_text(&app, &session_id, "assistant", "a1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], true);

    let start = Instant::now();
    let (status, body) = fetch_context(&app, &session_id).await;
    let elapsed = start.elapsed();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["is_compressing"], true);
    assert!(
        elapsed < Duration::from_millis(50),
        "fetch should return quickly during compression, elapsed={elapsed:?}"
    );
}

#[tokio::test]
async fn scenario_10_threshold_keeps_triggering_every_n_completed_turns_after_compression() {
    let config = make_config(3, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(20),
        outcome: MockOutcome::Success("summary".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;

    let mut trigger_turns = Vec::new();
    for idx in 1..=6 {
        let (status, _) = append_text(&app, &session_id, "user", &format!("u{idx}")).await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = append_text(&app, &session_id, "assistant", &format!("a{idx}")).await;
        assert_eq!(status, StatusCode::OK);

        if body["compression_triggered"] == Value::Bool(true) {
            trigger_turns.push(
                body["turn_count"]
                    .as_u64()
                    .expect("turn_count should be number"),
            );
            wait_until_compression_finishes(&store, &session_id, Duration::from_secs(2)).await;
        }
    }

    assert_eq!(
        trigger_turns,
        vec![3, 6],
        "compression should trigger at turn 3 and turn 6"
    );

    let (status, body) = fetch_context(&app, &session_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["turn_count"], 6);
    assert_eq!(body["is_compressing"], false);
}

#[tokio::test]
async fn scenario_11_keep_recent_zero_recompresses_full_window_except_persona() {
    let config = make_config(3, 0, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(20),
        outcome: MockOutcome::Success("summary".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let (status, body) = call_json(
        &app,
        Method::POST,
        "/sessions",
        Some(json!({"system_prompt": "你是测试助手"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = body["session_id"]
        .as_str()
        .expect("session id is required")
        .to_string();

    let mut trigger_turns = Vec::new();
    for idx in 1..=6 {
        let (status, _) = append_text(&app, &session_id, "user", &format!("u{idx}")).await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = append_text(&app, &session_id, "assistant", &format!("a{idx}")).await;
        assert_eq!(status, StatusCode::OK);

        if body["compression_triggered"] == Value::Bool(true) {
            trigger_turns.push(
                body["turn_count"]
                    .as_u64()
                    .expect("turn_count should be number"),
            );
            wait_until_compression_finishes(&store, &session_id, Duration::from_secs(2)).await;
        }
    }

    assert_eq!(trigger_turns, vec![3, 6]);

    let (status, body) = fetch_context(&app, &session_id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["turn_count"], 6);
    assert_eq!(body["compressed_turns"], 6);
    assert_eq!(body["is_compressing"], false);

    let messages = body["messages"]
        .as_array()
        .expect("messages should be array");
    assert_eq!(
        messages.len(),
        2,
        "only persona + latest summary should remain"
    );
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "你是测试助手");
    assert_eq!(messages[1]["role"], "system");
    assert!(
        messages[1]["content"]
            .as_str()
            .map(|text| text.starts_with("[CONTEXT SUMMARY]"))
            .unwrap_or(false),
        "latest summary should be present"
    );
}

#[tokio::test]
async fn scenario_compression_error_path_with_retry_count_zero() {
    let config = make_config(1, 0, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(5),
        outcome: MockOutcome::Error("forced failure".to_string()),
    });
    let (app, store) = build_test_app(config, llm);

    let session_id = create_session(&app).await;
    let (status, _) = append_text(&app, &session_id, "user", "u1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = append_text(&app, &session_id, "assistant", "a1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["compression_triggered"], true);

    wait_until_compression_finishes(&store, &session_id, Duration::from_secs(2)).await;
    let (status, body) = fetch_context(&app, &session_id).await;
    assert_eq!(status, StatusCode::OK);

    let messages = body["messages"]
        .as_array()
        .expect("messages array expected");
    assert_eq!(messages.len(), 2);
}

#[tokio::test]
async fn scenario_ex_dashboard_route_serves_three_column_shell() {
    let config = make_config(5, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("hello from demo".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let (status, body) = call_text(&app, Method::GET, "/ex/dashboard").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Compression Preview Studio"));
    assert!(body.contains("builder-layout"));
    assert!(body.contains("trace-feed"));
}

#[tokio::test]
async fn scenario_ex_playground_route_serves_livekit_inspired_shell() {
    let config = make_config(5, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("hello from example".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let (status, body) = call_text(&app, Method::GET, "/ex/playground").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("LiveKit Playground Example"));
    assert!(body.contains("ctx-cache-compressor Playground"));
    assert!(body.contains("Agent Video"));
    assert!(body.contains("Show video"));
}

#[tokio::test]
async fn scenario_compressor_route_serves_chat_and_metrics_shell() {
    let config = make_config(5, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("hello from playground".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let (status, body) = call_text(&app, Method::GET, "/compressor").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ctx-cache-compressor Demo Console"));
    assert!(body.contains("Conversation"));
    assert!(body.contains("Observe"));
    assert!(body.contains("Settings"));
    assert!(body.contains("Compression Strategy"));
    assert!(body.contains("Conversation Model"));
    assert!(body.contains("Current Session"));
    assert!(body.contains("Session Cache"));
    assert!(body.contains("Create Session"));
    assert!(body.contains("EN"));
    assert!(body.contains("中文"));
    assert!(!body.contains("最近事件"));
    assert!(!body.contains("最新摘要"));
    assert!(!body.contains("系统提示词"));
    assert!(!body.contains("发送消息前会同步到当前会话"));
    assert!(!body.contains(
        "这个页面专门用来演示 /demo/chat 如何驱动上下文增长，以及 ctx-cache-compressor 何时开始压缩。"
    ));
}

#[tokio::test]
async fn scenario_removed_legacy_page_routes_return_not_found() {
    let config = make_config(5, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("hello from legacy aliases".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let (status, body) = call_text(&app, Method::GET, "/dashboard").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_empty());

    let (status, body) = call_text(&app, Method::GET, "/playground-example").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_empty());

    let (status, body) = call_text(&app, Method::GET, "/ctx-compressor-playground").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.is_empty());
}

#[tokio::test]
async fn scenario_demo_chat_returns_reply_and_trace_rich_context() {
    let config = make_config(2, 1, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(5),
        outcome: MockOutcome::Success("assistant demo reply".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let (status, config_body) = call_json(&app, Method::GET, "/demo/config", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(config_body["compression_every_n_turns"], 2);

    let (status, body) = call_json(
        &app,
        Method::POST,
        "/demo/chat",
        Some(json!({
            "system_prompt": "You are a concise assistant.",
            "user_message": "hello dashboard"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["assistant_message"], "assistant demo reply");

    let session_id = body["session_id"]
        .as_str()
        .expect("session id should exist")
        .to_string();
    let context = &body["context"];
    assert_eq!(context["session_id"], session_id);
    assert_eq!(context["turn_count"], 1);

    let messages = context["messages"]
        .as_array()
        .expect("messages should be array");
    assert!(
        messages
            .iter()
            .any(|message| message["content"] == "hello dashboard"),
        "user message should exist in context"
    );
    assert!(
        messages
            .iter()
            .any(|message| message["content"] == "assistant demo reply"),
        "assistant message should exist in context"
    );

    let traces = context["traces"]
        .as_array()
        .expect("traces should be an array");
    assert!(
        traces
            .iter()
            .any(|trace| trace["kind"] == "demo_chat_completed"),
        "demo chat completion trace should be present"
    );
}

#[tokio::test]
async fn scenario_demo_chat_updates_existing_session_system_prompt() {
    let config = make_config(2, 1, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::from_millis(5),
        outcome: MockOutcome::Success("assistant prompt sync reply".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let (status, first_body) = call_json(
        &app,
        Method::POST,
        "/demo/chat",
        Some(json!({
            "system_prompt": "You are Alice.",
            "user_message": "hello"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let session_id = first_body["session_id"]
        .as_str()
        .expect("session id should exist")
        .to_string();

    let (status, second_body) = call_json(
        &app,
        Method::POST,
        "/demo/chat",
        Some(json!({
            "session_id": session_id,
            "system_prompt": "你是小明",
            "user_message": "你是谁？"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let messages = second_body["context"]["messages"]
        .as_array()
        .expect("messages should be array");
    assert!(
        messages
            .iter()
            .any(|message| message["role"] == "system" && message["content"] == "你是小明"),
        "updated system prompt should be stored in current session"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message["role"] == "system" && message["content"] == "You are Alice."),
        "stale system prompt should be replaced"
    );
}

#[tokio::test]
async fn scenario_demo_config_can_be_updated_at_runtime() {
    let config = make_config(5, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("runtime config reply".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let (status, body) = call_json(
        &app,
        Method::PATCH,
        "/demo/config",
        Some(json!({
            "conversation_llm_base_url": "https://api.deepseek.com",
            "conversation_llm_api_key": "sk-test-1234",
            "conversation_llm_model": "deepseek-v4-flash"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["conversation_llm_base_url"],
        "https://api.deepseek.com"
    );
    assert_eq!(body["conversation_llm_model"], "deepseek-v4-flash");
    assert_eq!(body["compression_every_n_turns"], 5);
    assert_eq!(body["conversation_llm_api_key_configured"], true);

    let (status, config_body) = call_json(&app, Method::GET, "/demo/config", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        config_body["conversation_llm_base_url"],
        "https://api.deepseek.com"
    );
    assert_eq!(config_body["conversation_llm_model"], "deepseek-v4-flash");
    assert_eq!(config_body["compression_every_n_turns"], 5);
    assert_eq!(config_body["conversation_llm_api_key_configured"], true);
}

#[tokio::test]
async fn scenario_sessions_endpoint_lists_active_sessions() {
    let config = make_config(5, 2, 2, 0, 3600);
    let llm = Arc::new(MockLlm {
        delay: Duration::ZERO,
        outcome: MockOutcome::Success("list sessions".to_string()),
    });
    let (app, _store) = build_test_app(config, llm);

    let session_a = create_session(&app).await;
    let session_b = create_session(&app).await;

    let (status, body) = call_json(&app, Method::GET, "/sessions", None).await;
    assert_eq!(status, StatusCode::OK);

    let sessions = body["sessions"]
        .as_array()
        .expect("sessions should be an array");
    assert!(
        sessions
            .iter()
            .any(|session| session["session_id"] == session_a),
        "session_a should be present"
    );
    assert!(
        sessions
            .iter()
            .any(|session| session["session_id"] == session_b),
        "session_b should be present"
    );
}

fn _tool_message(call_id: &str) -> Message {
    Message {
        role: Role::Tool,
        content: Some(MessageContent::Text("result".to_string())),
        tool_calls: None,
        tool_call_id: Some(call_id.to_string()),
        name: Some("search".to_string()),
    }
}
