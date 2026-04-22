use std::collections::HashSet;

use crate::session::types::Message;

pub fn is_at_turn_boundary(messages: &[Message]) -> bool {
    if messages.is_empty() {
        return false;
    }

    let Some(last) = messages.last() else {
        return false;
    };

    last.is_assistant_final() && all_tool_calls_resolved(messages)
}

pub fn all_tool_calls_resolved(messages: &[Message]) -> bool {
    let issued_calls: HashSet<&str> = messages
        .iter()
        .filter_map(|m| m.tool_calls.as_ref())
        .flatten()
        .map(|tc| tc.id.as_str())
        .collect();

    let resolved_calls: HashSet<&str> = messages
        .iter()
        .filter_map(|m| m.tool_call_id.as_deref())
        .collect();

    issued_calls == resolved_calls
}

pub fn count_completed_turns(messages: &[Message]) -> u32 {
    messages
        .iter()
        .filter(|message| message.is_assistant_final())
        .count() as u32
}

pub fn split_index_for_keep_recent_turns(messages: &[Message], keep_recent_turns: u32) -> usize {
    if messages.is_empty() {
        return 0;
    }

    if keep_recent_turns == 0 {
        return messages.len();
    }

    let final_assistant_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, message)| message.is_assistant_final().then_some(idx))
        .collect();

    if final_assistant_indices.len() <= keep_recent_turns as usize {
        return 0;
    }

    let cutoff_turn_end_idx =
        final_assistant_indices[final_assistant_indices.len() - keep_recent_turns as usize - 1];
    cutoff_turn_end_idx + 1
}

#[cfg(test)]
mod tests {
    use crate::session::types::{Message, MessageContent, Role, ToolCall, ToolFunction};

    use super::{
        all_tool_calls_resolved, count_completed_turns, is_at_turn_boundary,
        split_index_for_keep_recent_turns,
    };

    fn user(text: &str) -> Message {
        Message::text(Role::User, text)
    }

    fn assistant(text: &str) -> Message {
        Message::text(Role::Assistant, text)
    }

    fn assistant_tool_call(id: &str) -> Message {
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

    #[test]
    fn boundary_requires_last_final_assistant() {
        let messages = vec![user("hi")];
        assert!(!is_at_turn_boundary(&messages));

        let messages = vec![user("hi"), assistant("done")];
        assert!(is_at_turn_boundary(&messages));
    }

    #[test]
    fn unresolved_tool_call_is_not_boundary() {
        let messages = vec![user("hi"), assistant_tool_call("call_1")];
        assert!(!all_tool_calls_resolved(&messages));
        assert!(!is_at_turn_boundary(&messages));
    }

    #[test]
    fn resolved_tool_call_with_final_assistant_is_boundary() {
        let messages = vec![
            user("hi"),
            assistant_tool_call("call_1"),
            tool("call_1"),
            assistant("done"),
        ];
        assert!(all_tool_calls_resolved(&messages));
        assert!(is_at_turn_boundary(&messages));
    }

    #[test]
    fn completed_turn_count_matches_final_assistant_count() {
        let messages = vec![
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant_tool_call("call_1"),
            tool("call_1"),
            assistant("a2"),
        ];

        assert_eq!(count_completed_turns(&messages), 2);
    }

    #[test]
    fn split_index_keeps_recent_turns() {
        let messages = vec![
            user("u1"),
            assistant("a1"),
            user("u2"),
            assistant("a2"),
            user("u3"),
            assistant("a3"),
        ];

        assert_eq!(split_index_for_keep_recent_turns(&messages, 2), 2);
        assert_eq!(split_index_for_keep_recent_turns(&messages, 0), 6);
        assert_eq!(split_index_for_keep_recent_turns(&messages, 3), 0);
    }
}
