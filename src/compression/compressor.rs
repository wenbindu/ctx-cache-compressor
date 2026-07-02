use std::sync::Arc;

use tracing::instrument;

use crate::{
    compression::prompt::{build_compression_prompts, compression_input_char_count},
    config::CompressionPromptConfig,
    error::{AppError, AppResult},
    llm::client::CompressionLlm,
    session::{
        turn::{count_completed_turns, split_index_for_keep_recent_turns},
        types::{Message, Role},
    },
};

const MAX_SUMMARY_TO_SOURCE_RATIO_NUMERATOR: usize = 3;
const MAX_SUMMARY_TO_SOURCE_RATIO_DENOMINATOR: usize = 4;

#[derive(Debug, Clone)]
pub struct CompressionPlan {
    pub preserve_head: Vec<Message>,
    pub previous_summary: Option<Message>,
    pub compressible: Vec<Message>,
    pub preserve_tail: Vec<Message>,
    pub compressed_turns_delta: u32,
}

#[derive(Debug, Clone)]
pub struct CompressionOutcome {
    pub new_stable: Vec<Message>,
    pub compressed_turns_delta: u32,
    pub evaluation: CompressionEvaluation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionEvaluation {
    pub prompt_input_message_count: usize,
    pub prompt_input_char_count: usize,
    pub compressed_region_message_count: usize,
    pub compressed_region_char_count: usize,
    pub stable_output_message_count: usize,
    pub stable_output_char_count: usize,
    pub summary_char_count: usize,
    pub retained_recent_message_count: usize,
    pub previous_summary_included: bool,
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

        let (previous_summary, delta_body): (Option<Message>, &[Message]) = if body
            .first()
            .map(Message::is_context_summary)
            .unwrap_or(false)
        {
            (Some(body[0].clone()), &body[1..])
        } else {
            (None, body)
        };

        if delta_body.is_empty() {
            return None;
        }

        let split_idx = split_index_for_keep_recent_turns(delta_body, keep_recent_turns);
        let compressible = delta_body[..split_idx].to_vec();
        if compressible.is_empty() {
            return None;
        }

        let preserve_tail = delta_body[split_idx..].to_vec();
        let compressed_turns_delta = count_completed_turns(&compressible);

        Some(CompressionPlan {
            preserve_head,
            previous_summary,
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

        let mut prompt_messages = Vec::new();
        if let Some(previous_summary) = plan.previous_summary.as_ref() {
            prompt_messages.push(previous_summary.clone());
        }
        prompt_messages.extend(plan.compressible.clone());

        let compressible_turns = count_completed_turns(&plan.compressible);
        let (system_prompt, user_prompt) =
            build_compression_prompts(&prompt_messages, compressible_turns, &self.prompt_config)?;

        let prompt_input_message_count = prompt_messages.len();
        let prompt_input_char_count = user_prompt.chars().count();
        let compressed_region_message_count = plan.compressible.len();
        let compressed_region_char_count = compression_input_char_count(&plan.compressible)?;
        let previous_summary_included = plan.previous_summary.is_some();

        let summary = self.llm.compress(&system_prompt, &user_prompt).await?;
        let summary = summary.trim().to_string();
        validate_summary_quality(
            &summary,
            summary.chars().count(),
            compressed_region_char_count,
        )?;
        let summary_message = Message::system_summary(summary);
        let summary_char_count = summary_message.estimated_compression_visible_char_len();

        let mut new_stable = plan.preserve_head;
        new_stable.push(summary_message);
        let retained_recent_message_count = plan.preserve_tail.len();
        new_stable.extend(plan.preserve_tail);
        let stable_output_message_count = new_stable.len();
        let stable_output_char_count = new_stable
            .iter()
            .map(Message::estimated_compression_visible_char_len)
            .sum();

        Ok(CompressionOutcome {
            new_stable,
            compressed_turns_delta: plan.compressed_turns_delta,
            evaluation: CompressionEvaluation {
                prompt_input_message_count,
                prompt_input_char_count,
                compressed_region_message_count,
                compressed_region_char_count,
                stable_output_message_count,
                stable_output_char_count,
                summary_char_count,
                retained_recent_message_count,
                previous_summary_included,
            },
        })
    }
}

fn validate_summary_quality(
    summary: &str,
    summary_char_count: usize,
    compressed_region_char_count: usize,
) -> AppResult<()> {
    if summary.trim().is_empty() {
        return Err(AppError::Upstream(
            "compression summary was empty".to_string(),
        ));
    }

    if compressed_region_char_count == 0 {
        return Ok(());
    }

    if summary_char_count >= compressed_region_char_count {
        return Err(AppError::Upstream(format!(
            "compression summary did not reduce compressed region: summary {summary_char_count} chars >= source {compressed_region_char_count} chars"
        )));
    }

    if summary_char_count.saturating_mul(MAX_SUMMARY_TO_SOURCE_RATIO_DENOMINATOR)
        > compressed_region_char_count.saturating_mul(MAX_SUMMARY_TO_SOURCE_RATIO_NUMERATOR)
    {
        return Err(AppError::Upstream(format!(
            "compression summary exceeded maximum size ratio: summary {summary_char_count} chars, source {compressed_region_char_count} chars"
        )));
    }

    Ok(())
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
            reasoning_content: None,
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
            reasoning_content: None,
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
        assert_eq!(outcome.evaluation.prompt_input_message_count, 6);
        assert_eq!(outcome.evaluation.compressed_region_message_count, 6);
        assert_eq!(outcome.evaluation.stable_output_message_count, 4);
        assert_eq!(outcome.evaluation.retained_recent_message_count, 2);
        assert!(!outcome.evaluation.previous_summary_included);
    }

    #[tokio::test]
    async fn compression_rolls_existing_summary_forward_without_counting_it_as_delta() {
        let llm = Arc::new(MockLlm {
            mode: MockMode::Ok("updated summary".to_string()),
        });
        let compressor = Compressor::new(llm, CompressionPromptConfig::default());
        let previous_summary = Message::system_summary("old summary");

        let snapshot = vec![
            Message::text(Role::System, "you are helpful"),
            previous_summary.clone(),
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
            user("u3"),
            assistant("a3"),
        ];

        let plan = Compressor::plan(&snapshot, 1).expect("plan should exist");
        assert_eq!(plan.previous_summary, Some(previous_summary));
        assert_eq!(
            plan.compressible,
            vec![user("u1"), assistant("a1"), user("u2"), assistant("a2")]
        );
        assert_eq!(plan.preserve_tail, vec![user("u3"), assistant("a3")]);
        assert_eq!(plan.compressed_turns_delta, 2);

        let outcome = compressor
            .compress_snapshot(&snapshot, 1)
            .await
            .expect("compression should succeed");

        assert_eq!(outcome.new_stable.len(), 4);
        assert_eq!(outcome.new_stable[0].content_text(), "you are helpful");
        assert!(outcome.new_stable[1].is_context_summary());
        assert!(outcome.new_stable[1]
            .content_text()
            .contains("updated summary"));
        assert_eq!(outcome.new_stable[2], user("u3"));
        assert_eq!(outcome.new_stable[3], assistant("a3"));
        assert_eq!(outcome.compressed_turns_delta, 2);
        assert_eq!(outcome.evaluation.prompt_input_message_count, 5);
        assert_eq!(outcome.evaluation.compressed_region_message_count, 4);
        assert_eq!(outcome.evaluation.stable_output_message_count, 4);
        assert_eq!(outcome.evaluation.retained_recent_message_count, 2);
        assert!(outcome.evaluation.previous_summary_included);
    }

    #[tokio::test]
    async fn compression_evaluation_excludes_reasoning_content_from_prompt_counts() {
        let llm = Arc::new(MockLlm {
            mode: MockMode::Ok("summary".to_string()),
        });
        let compressor = Compressor::new(llm, CompressionPromptConfig::default());
        let mut assistant_with_reasoning = assistant("a1");
        assistant_with_reasoning.reasoning_content = Some("x".repeat(10_000));

        let snapshot = vec![
            user("u1"),
            assistant_with_reasoning,
            user("u2"),
            assistant("a2"),
        ];

        let outcome = compressor
            .compress_snapshot(&snapshot, 1)
            .await
            .expect("compression should succeed");

        assert!(outcome.evaluation.prompt_input_char_count < 1_000);
        assert!(outcome.evaluation.compressed_region_char_count < 1_000);
    }

    #[tokio::test]
    async fn compression_rejects_empty_summary() {
        let llm = Arc::new(MockLlm {
            mode: MockMode::Ok("   ".to_string()),
        });
        let compressor = Compressor::new(llm, CompressionPromptConfig::default());
        let snapshot = vec![user("u1"), assistant("a1"), user("u2"), assistant("a2")];

        let err = compressor
            .compress_snapshot(&snapshot, 1)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("compression summary was empty"));
    }

    #[tokio::test]
    async fn compression_rejects_oversized_summary() {
        let llm = Arc::new(MockLlm {
            mode: MockMode::Ok("x".repeat(10_000)),
        });
        let compressor = Compressor::new(llm, CompressionPromptConfig::default());
        let snapshot = vec![user("u1"), assistant("a1"), user("u2"), assistant("a2")];

        let err = compressor
            .compress_snapshot(&snapshot, 1)
            .await
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("compression summary did not reduce compressed region"));
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
