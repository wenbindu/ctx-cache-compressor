use crate::{config::CompressionPromptConfig, error::AppResult, session::types::Message};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryLanguage {
    Chinese,
    English,
}

pub fn build_compression_prompts(
    messages: &[Message],
    turn_count: u32,
    prompt_config: &CompressionPromptConfig,
) -> AppResult<(String, String)> {
    let serialized = serde_json::to_string_pretty(messages)?;
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
        assert!(user_prompt.contains("以下是需要压缩的对话历史"));
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
