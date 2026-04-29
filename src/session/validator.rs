use std::collections::HashSet;

use crate::{
    error::AppError,
    session::types::{Message, Role},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeState {
    Start,
    System,
    User,
    AssistantToolCalls,
    AssistantFinal,
    Tool,
}

pub fn validate_append(existing_messages: &[Message], incoming: &Message) -> Result<(), AppError> {
    let prev_state = existing_messages
        .last()
        .map(classify)
        .unwrap_or(NodeState::Start);
    let next_state = classify(incoming);

    if !is_transition_allowed(prev_state, next_state) {
        return Err(AppError::BadRequest(format!(
            "invalid role sequence: {prev_state:?} -> {next_state:?}"
        )));
    }

    validate_tool_fields(existing_messages, incoming)
}

fn classify(message: &Message) -> NodeState {
    match message.role {
        Role::System => NodeState::System,
        Role::User => NodeState::User,
        Role::Assistant => {
            if message.has_tool_calls() {
                NodeState::AssistantToolCalls
            } else {
                NodeState::AssistantFinal
            }
        }
        Role::Tool => NodeState::Tool,
    }
}

fn is_transition_allowed(prev: NodeState, next: NodeState) -> bool {
    matches!(
        (prev, next),
        (NodeState::Start, NodeState::User)
            | (NodeState::Start, NodeState::System)
            | (NodeState::System, NodeState::User)
            | (NodeState::User, NodeState::AssistantToolCalls)
            | (NodeState::User, NodeState::AssistantFinal)
            | (NodeState::AssistantToolCalls, NodeState::Tool)
            | (NodeState::Tool, NodeState::Tool)
            | (NodeState::Tool, NodeState::AssistantFinal)
            | (NodeState::AssistantFinal, NodeState::User)
    )
}

fn validate_tool_fields(existing_messages: &[Message], incoming: &Message) -> Result<(), AppError> {
    match incoming.role {
        Role::Tool => {
            if incoming.tool_call_id.is_none() {
                return Err(AppError::BadRequest(
                    "tool message requires tool_call_id".to_string(),
                ));
            }

            if incoming.name.is_none() {
                return Err(AppError::BadRequest(
                    "tool message requires tool name".to_string(),
                ));
            }

            let issued_calls = collect_issued_tool_call_ids(existing_messages);
            let resolved_calls = collect_resolved_tool_call_ids(existing_messages);
            let call_id = incoming.tool_call_id.as_deref().unwrap_or_default();
            if !issued_calls.contains(call_id) {
                return Err(AppError::BadRequest(format!(
                    "tool_call_id '{call_id}' was never issued"
                )));
            }
            if resolved_calls.contains(call_id) {
                return Err(AppError::BadRequest(format!(
                    "tool_call_id '{call_id}' was already resolved"
                )));
            }
        }
        Role::Assistant => {
            if let Some(tool_calls) = incoming.tool_calls.as_ref() {
                if tool_calls.is_empty() {
                    return Err(AppError::BadRequest(
                        "assistant tool_calls cannot be empty".to_string(),
                    ));
                }

                let existing_issued_calls = collect_issued_tool_call_ids(existing_messages);
                let mut incoming_ids = HashSet::new();
                for call in tool_calls {
                    if !incoming_ids.insert(call.id.as_str()) {
                        return Err(AppError::BadRequest(format!(
                            "duplicate tool_call id '{}'",
                            call.id
                        )));
                    }

                    if existing_issued_calls.contains(call.id.as_str()) {
                        return Err(AppError::BadRequest(format!(
                            "tool_call id '{}' was already issued",
                            call.id
                        )));
                    }
                }
            }
        }
        Role::User | Role::System => {}
    }

    Ok(())
}

fn collect_issued_tool_call_ids(messages: &[Message]) -> HashSet<&str> {
    messages
        .iter()
        .filter_map(|m| m.tool_calls.as_ref())
        .flatten()
        .map(|tc| tc.id.as_str())
        .collect()
}

fn collect_resolved_tool_call_ids(messages: &[Message]) -> HashSet<&str> {
    messages
        .iter()
        .filter_map(|m| m.tool_call_id.as_deref())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::types::{Message, Role, ToolCall, ToolFunction};

    fn user_msg() -> Message {
        Message::text(Role::User, "hello")
    }

    fn system_msg() -> Message {
        Message::text(Role::System, "you are helpful")
    }

    fn assistant_final() -> Message {
        Message::text(Role::Assistant, "done")
    }

    fn assistant_tool_call() -> Message {
        assistant_tool_call_with_id("call_1")
    }

    fn assistant_tool_call_with_id(call_id: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: call_id.to_string(),
                call_type: "function".to_string(),
                function: ToolFunction {
                    name: "search".to_string(),
                    arguments: "{\"q\":\"rust\"}".to_string(),
                },
            }]),
            tool_call_id: None,
            name: None,
        }
    }

    fn tool_msg() -> Message {
        Message {
            role: Role::Tool,
            content: Some(crate::session::types::MessageContent::Text(
                "result".to_string(),
            )),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some("call_1".to_string()),
            name: Some("search".to_string()),
        }
    }

    #[test]
    fn start_accepts_user_or_system() {
        assert!(validate_append(&[], &user_msg()).is_ok());
        assert!(validate_append(&[], &system_msg()).is_ok());
    }

    #[test]
    fn start_rejects_assistant() {
        let err = validate_append(&[], &assistant_final()).unwrap_err();
        assert!(err.to_string().contains("invalid role sequence"));
    }

    #[test]
    fn system_only_allows_user_after_it() {
        let existing = vec![system_msg()];
        assert!(validate_append(&existing, &user_msg()).is_ok());
        assert!(validate_append(&existing, &assistant_final()).is_err());
    }

    #[test]
    fn user_requires_assistant_next() {
        let existing = vec![user_msg()];
        assert!(validate_append(&existing, &assistant_final()).is_ok());
        assert!(validate_append(&existing, &tool_msg()).is_err());
    }

    #[test]
    fn tool_chain_is_valid() {
        let existing = vec![user_msg()];
        assert!(validate_append(&existing, &assistant_tool_call()).is_ok());

        let existing = vec![user_msg(), assistant_tool_call()];
        assert!(validate_append(&existing, &tool_msg()).is_ok());

        let existing = vec![user_msg(), assistant_tool_call(), tool_msg()];
        assert!(validate_append(&existing, &assistant_final()).is_ok());
    }

    #[test]
    fn tool_message_requires_tool_fields() {
        let existing = vec![user_msg(), assistant_tool_call()];
        let mut msg = tool_msg();
        msg.tool_call_id = None;
        assert!(validate_append(&existing, &msg).is_err());

        let mut msg = tool_msg();
        msg.name = None;
        assert!(validate_append(&existing, &msg).is_err());
    }

    #[test]
    fn tool_message_requires_previously_issued_call_id() {
        let existing = vec![user_msg(), assistant_tool_call()];
        let mut msg = tool_msg();
        msg.tool_call_id = Some("call_other".to_string());
        assert!(validate_append(&existing, &msg).is_err());
    }

    #[test]
    fn tool_message_rejects_duplicate_result_for_same_call_id() {
        let existing = vec![user_msg(), assistant_tool_call(), tool_msg()];
        let err = validate_append(&existing, &tool_msg()).unwrap_err();
        assert!(err.to_string().contains("already resolved"));
    }

    #[test]
    fn assistant_tool_calls_reject_empty_or_duplicate_ids() {
        let existing = vec![user_msg()];
        let mut empty = assistant_tool_call();
        empty.tool_calls = Some(Vec::new());
        assert!(validate_append(&existing, &empty)
            .unwrap_err()
            .to_string()
            .contains("cannot be empty"));

        let mut duplicate = assistant_tool_call();
        duplicate.tool_calls = Some(vec![
            ToolCall {
                id: "call_1".to_string(),
                call_type: "function".to_string(),
                function: ToolFunction {
                    name: "search".to_string(),
                    arguments: "{}".to_string(),
                },
            },
            ToolCall {
                id: "call_1".to_string(),
                call_type: "function".to_string(),
                function: ToolFunction {
                    name: "lookup".to_string(),
                    arguments: "{}".to_string(),
                },
            },
        ]);
        assert!(validate_append(&existing, &duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate tool_call id"));
    }

    #[test]
    fn assistant_tool_calls_reject_reused_ids_from_existing_history() {
        let existing = vec![
            user_msg(),
            assistant_tool_call(),
            tool_msg(),
            assistant_final(),
            user_msg(),
        ];
        let err = validate_append(&existing, &assistant_tool_call_with_id("call_1")).unwrap_err();
        assert!(err.to_string().contains("already issued"));
    }

    #[test]
    fn assistant_final_must_be_followed_by_user() {
        let existing = vec![user_msg(), assistant_final()];
        assert!(validate_append(&existing, &user_msg()).is_ok());
        assert!(validate_append(&existing, &assistant_final()).is_err());
    }
}
