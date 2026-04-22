use std::sync::Arc;

use tracing::instrument;

use crate::{
    compression::prompt::build_compression_prompts,
    config::CompressionPromptConfig,
    error::{AppError, AppResult},
    llm::client::CompressionLlm,
    session::{
        turn::{count_completed_turns, split_index_for_keep_recent_turns},
        types::{Message, Role},
    },
};

#[derive(Debug, Clone)]
pub struct CompressionPlan {
    pub preserve_head: Vec<Message>,
    pub compressible: Vec<Message>,
    pub preserve_tail: Vec<Message>,
    pub compressed_turns_delta: u32,
}

#[derive(Debug, Clone)]
pub struct CompressionOutcome {
    pub new_stable: Vec<Message>,
    pub compressed_turns_delta: u32,
}

#[derive(Clone)]
pub struct Compressor {
    llm: Arc<dyn CompressionLlm>,
    prompt_config: CompressionPromptConfig,
}

impl Compressor {
    pub fn new(llm: Arc<dyn CompressionLlm>, prompt_config: CompressionPromptConfig) -> Self {
        Self { llm, prompt_config }
    }

    pub fn plan(snapshot: &[Message], keep_recent_turns: u32) -> Option<CompressionPlan> {
        if snapshot.is_empty() {
            return None;
        }

        let (preserve_head, body): (Vec<Message>, &[Message]) = if snapshot
            .first()
            .map(|msg| msg.role == Role::System && !msg.is_context_summary())
            .unwrap_or(false)
        {
            (vec![snapshot[0].clone()], &snapshot[1..])
        } else {
            (Vec::new(), snapshot)
        };

        if body.is_empty() {
            return None;
        }

        let split_idx = split_index_for_keep_recent_turns(body, keep_recent_turns);
        let compressible = body[..split_idx].to_vec();
        if compressible.is_empty() {
            return None;
        }

        let preserve_tail = body[split_idx..].to_vec();
        let compressed_turns_delta = count_completed_turns(&compressible);

        Some(CompressionPlan {
            preserve_head,
            compressible,
            preserve_tail,
            compressed_turns_delta,
        })
    }

    #[instrument(skip(self, snapshot))]
    pub async fn compress_snapshot(
        &self,
        snapshot: &[Message],
        keep_recent_turns: u32,
    ) -> AppResult<CompressionOutcome> {
        let Some(plan) = Self::plan(snapshot, keep_recent_turns) else {
            return Err(AppError::Conflict(
                "no compressible messages in current snapshot".to_string(),
            ));
        };

        let compressible_turns = count_completed_turns(&plan.compressible);
        let (system_prompt, user_prompt) =
            build_compression_prompts(&plan.compressible, compressible_turns, &self.prompt_config)?;

        let summary = self.llm.compress(&system_prompt, &user_prompt).await?;

        let mut new_stable = plan.preserve_head;
        new_stable.push(Message::system_summary(summary));
        new_stable.extend(plan.preserve_tail);

        Ok(CompressionOutcome {
            new_stable,
            compressed_turns_delta: plan.compressed_turns_delta,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{future::Future, pin::Pin};

    use super::*;
    use crate::{
        config::CompressionPromptConfig,
        llm::client::CompressionLlm,
        session::types::{MessageContent, ToolCall, ToolFunction},
    };

    enum MockMode {
        Ok(String),
        Err(String),
    }

    struct MockLlm {
        mode: MockMode,
    }

    impl CompressionLlm for MockLlm {
        fn compress<'a>(
            &'a self,
            _system_prompt: &'a str,
            _user_prompt: &'a str,
        ) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send + 'a>> {
            Box::pin(async move {
                match &self.mode {
                    MockMode::Ok(text) => Ok(text.clone()),
                    MockMode::Err(msg) => Err(AppError::Upstream(msg.clone())),
                }
            })
        }
    }

    fn user(text: &str) -> Message {
        Message::text(Role::User, text)
    }

    fn assistant(text: &str) -> Message {
        Message::text(Role::Assistant, text)
    }

    fn assistant_with_tool(id: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: id.to_string(),
                call_type: "function".to_string(),
                function: ToolFunction {
                    name: "search".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
        }
    }

    fn tool(id: &str) -> Message {
        Message {
            role: Role::Tool,
            content: Some(MessageContent::Text("result".to_string())),
            tool_calls: None,
            tool_call_id: Some(id.to_string()),
            name: Some("search".to_string()),
        }
    }

    #[tokio::test]
    async fn compression_replaces_old_history_and_keeps_recent_turns() {
        let llm = Arc::new(MockLlm {
            mode: MockMode::Ok("compressed summary".to_string()),
        });
        let compressor = Compressor::new(llm, CompressionPromptConfig::default());

        let snapshot = vec![
            Message::text(Role::System, "you are helpful"),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant_with_tool("c1"),
            tool("c1"),
            assistant("a2"),
            user("u3"),
            assistant("a3"),
        ];

        let outcome = compressor
            .compress_snapshot(&snapshot, 1)
            .await
            .expect("compression should succeed");

        assert_eq!(outcome.new_stable.len(), 4);
        assert_eq!(outcome.new_stable[0].role, Role::System);
        assert!(outcome.new_stable[1].is_context_summary());
        assert_eq!(outcome.new_stable[2], user("u3"));
        assert_eq!(outcome.new_stable[3], assistant("a3"));
        assert_eq!(outcome.compressed_turns_delta, 2);
    }

    #[tokio::test]
    async fn compression_returns_error_when_nothing_is_compressible() {
        let llm = Arc::new(MockLlm {
            mode: MockMode::Ok("summary".to_string()),
        });
        let compressor = Compressor::new(llm, CompressionPromptConfig::default());

        let snapshot = vec![user("u1"), assistant("a1")];
        let err = compressor
            .compress_snapshot(&snapshot, 1)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)));
    }

    #[tokio::test]
    async fn llm_failure_bubbles_up() {
        let llm = Arc::new(MockLlm {
            mode: MockMode::Err("llm failed".to_string()),
        });
        let compressor = Compressor::new(llm, CompressionPromptConfig::default());

        let snapshot = vec![user("u1"), assistant("a1"), user("u2"), assistant("a2")];

        let err = compressor
            .compress_snapshot(&snapshot, 1)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Upstream(_)));
    }
}
