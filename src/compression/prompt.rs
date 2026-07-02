use serde::Serialize;

use crate::{
    config::CompressionPromptConfig,
    error::AppResult,
    session::types::{Message, MessageContent, Role, ToolCall},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryLanguage {
    Chinese,
    English,
}

#[derive(Serialize)]
struct CompressionMessageView<'a> {
    role: &'a Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<&'a Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a String>,
}

pub fn build_compression_prompts(
    messages: &[Message],
    turn_count: u32,
    prompt_config: &CompressionPromptConfig,
) -> AppResult<(String, String)> {
    let serialized = format_weighted_compression_input(messages)?;
    let language = detect_dominant_language(messages);
    let language_label = match language {
        SummaryLanguage::Chinese => prompt_config.chinese_label.as_str(),
        SummaryLanguage::English => prompt_config.english_label.as_str(),
    };

    let mut system_prompt = prompt_config.system_instructions.trim().to_string();
    if prompt_config.enforce_dominant_language {
        let instruction = prompt_config
            .language_instruction_template
            .replace("{language_label}", language_label);
        if !instruction.trim().is_empty() {
            if !system_prompt.is_empty() {
                system_prompt.push('\n');
            }
            system_prompt.push_str(instruction.trim());
        }
    }

    let user_prompt = prompt_config
        .user_prompt_template
        .replace("{turn_count}", &turn_count.to_string())
        .replace("{serialized_messages}", &serialized);

    Ok((system_prompt, user_prompt))
}

fn format_weighted_compression_input(messages: &[Message]) -> AppResult<String> {
    let (previous_summary, dialogue_messages): (Option<&Message>, &[Message]) = if messages
        .first()
        .map(Message::is_context_summary)
        .unwrap_or(false)
    {
        (messages.first(), &messages[1..])
    } else {
        (None, messages)
    };

    let mut sections = Vec::new();
    if let Some(summary) = previous_summary {
        sections.push(format!(
            "【既有摘要｜权重：高｜用途：长期连续性基线】\n{}",
            summary.content_text()
        ));
    }

    sections.push(format!(
        "【新增对话｜权重：最高｜用途：更新当前状态、决策和最新用户意图】\n{}",
        serialize_messages_for_compression(dialogue_messages)?
    ));

    Ok(sections.join("\n\n"))
}

pub(crate) fn compression_input_char_count(messages: &[Message]) -> AppResult<usize> {
    Ok(format_weighted_compression_input(messages)?.chars().count())
}

fn serialize_messages_for_compression(messages: &[Message]) -> AppResult<String> {
    let views = messages
        .iter()
        .map(|message| CompressionMessageView {
            role: &message.role,
            content: message.content.as_ref(),
            tool_calls: message.tool_calls.as_ref(),
            tool_call_id: message.tool_call_id.as_ref(),
            name: message.name.as_ref(),
        })
        .collect::<Vec<_>>();

    Ok(serde_json::to_string_pretty(&views)?)
}

fn detect_dominant_language(messages: &[Message]) -> SummaryLanguage {
    let mut cjk_count = 0usize;
    let mut latin_count = 0usize;

    for message in messages {
        for ch in message.content_text().chars() {
            if is_cjk(ch) {
                cjk_count += 1;
            } else if ch.is_ascii_alphabetic() {
                latin_count += 1;
            }
        }
    }

    if latin_count > cjk_count {
        SummaryLanguage::English
    } else {
        // 默认中文，可覆盖中文主导与中英混合接近场景。
        SummaryLanguage::Chinese
    }
}

fn is_cjk(ch: char) -> bool {
    // CJK Unified Ideographs + CJK Compatibility Ideographs + Extension A
    matches!(
        ch as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
    )
}

#[cfg(test)]
mod tests {
    use super::{build_compression_prompts, detect_dominant_language, SummaryLanguage};
    use crate::{
        config::CompressionPromptConfig,
        session::types::{Message, Role},
    };

    #[test]
    fn detect_language_prefers_chinese() {
        let messages = vec![
            Message::text(Role::User, "今天进度不错，我们继续推进。"),
            Message::text(Role::Assistant, "好的，我来总结关键事项。"),
        ];
        assert_eq!(
            detect_dominant_language(&messages),
            SummaryLanguage::Chinese
        );
    }

    #[test]
    fn detect_language_prefers_english() {
        let messages = vec![
            Message::text(
                Role::User,
                "Please summarize the current status and next steps.",
            ),
            Message::text(
                Role::Assistant,
                "Sure, we completed integration and need stress tests.",
            ),
        ];
        assert_eq!(
            detect_dominant_language(&messages),
            SummaryLanguage::English
        );
    }

    #[test]
    fn prompt_contains_language_instruction() {
        let messages = vec![Message::text(Role::User, "请帮我总结一下。")];
        let cfg = CompressionPromptConfig::default();
        let (system_prompt, user_prompt) =
            build_compression_prompts(&messages, 1, &cfg).expect("prompt should build");

        assert!(system_prompt.contains("主要语言判定为：中文"));
        assert!(system_prompt.contains("[CONTEXT SUMMARY]"));
        assert!(system_prompt.contains("更新后的摘要"));
        assert!(system_prompt.contains("新增对话高于既有摘要"));
        assert!(user_prompt.contains("以下是需要压缩的对话历史"));
        assert!(user_prompt.contains("新增对话｜权重：最高"));
    }

    #[test]
    fn prompt_excludes_reasoning_content() {
        let mut assistant = Message::text(Role::Assistant, "最终回复");
        assistant.reasoning_content = Some("private chain of thought".to_string());
        let messages = vec![Message::text(Role::User, "请继续"), assistant];
        let cfg = CompressionPromptConfig::default();

        let (system_prompt, user_prompt) =
            build_compression_prompts(&messages, 1, &cfg).expect("prompt should build");

        assert!(!system_prompt.contains("private chain of thought"));
        assert!(!user_prompt.contains("private chain of thought"));
        assert!(!user_prompt.contains("reasoning_content"));
        assert!(user_prompt.contains("最终回复"));
    }

    #[test]
    fn prompt_separates_existing_summary_from_delta_dialogue() {
        let messages = vec![
            Message::system_summary("旧摘要：用户想调研压缩策略。"),
            Message::text(Role::User, "现在优先保持标准轮次压缩。"),
            Message::text(Role::Assistant, "明白，去掉 token window。"),
        ];
        let cfg = CompressionPromptConfig::default();

        let (_system_prompt, user_prompt) =
            build_compression_prompts(&messages, 1, &cfg).expect("prompt should build");

        assert!(user_prompt.contains("既有摘要｜权重：高"));
        assert!(user_prompt.contains("长期连续性基线"));
        assert!(user_prompt.contains("旧摘要：用户想调研压缩策略。"));
        assert!(user_prompt.contains("新增对话｜权重：最高"));
        assert!(user_prompt.contains("现在优先保持标准轮次压缩。"));
    }

    #[test]
    fn can_disable_language_instruction_by_config() {
        let messages = vec![Message::text(Role::User, "Please summarize this chat.")];
        let cfg = CompressionPromptConfig {
            enforce_dominant_language: false,
            ..CompressionPromptConfig::default()
        };
        let (system_prompt, _user_prompt) =
            build_compression_prompts(&messages, 1, &cfg).expect("prompt should build");
        assert!(!system_prompt.contains("主要语言判定为"));
    }
}
