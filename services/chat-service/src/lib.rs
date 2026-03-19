//! Chat Service - AI 对话服务
//!
//! 负责与大模型交互：
//! - OpenAI 兼容 API 调用
//! - Prompt 组装与消息处理

use serde::{Deserialize, Serialize};

/// OpenAI 格式消息
#[derive(Clone, Serialize, Deserialize)]
pub struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

/// OpenAI 请求格式
#[derive(Serialize)]
pub struct OpenAiReq {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
}

/// OpenAI 响应格式
#[derive(Deserialize)]
pub struct OpenAiResp {
    pub choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
pub struct OpenAiChoice {
    pub message: OpenAiMessage,
}

/// AI 服务配置
#[derive(Clone, Debug)]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl AiConfig {
    pub fn new(base_url: String, model: String, api_key: Option<String>) -> Self {
        Self {
            base_url,
            model,
            api_key,
        }
    }
}

/// AI 调用错误
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("缺少 API Key")]
    MissingKey,
    #[error("请求失败: {0}")]
    RequestFailed(String),
    #[error("解析响应失败: {0}")]
    ParseError(String),
}

/// 调用 OpenAI 兼容 API
pub async fn call_ai_openai_compat(
    cfg: &AiConfig,
    messages: &[OpenAiMessage],
) -> Result<String, AiError> {
    let key = cfg.api_key.clone().unwrap_or_default();

    if key.is_empty() {
        return Err(AiError::MissingKey);
    }

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", cfg.base_url);

    let req = OpenAiReq {
        model: cfg.model.clone(),
        messages: messages.to_vec(),
    };

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(&req)
        .send()
        .await
        .map_err(|e| AiError::RequestFailed(e.to_string()))?;

    let resp: OpenAiResp = response
        .json()
        .await
        .map_err(|e| AiError::ParseError(e.to_string()))?;

    Ok(resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default())
}

/// 构建聊天请求
pub fn build_chat_messages(system_prompt: &str, user_text: &str) -> Vec<OpenAiMessage> {
    vec![
        OpenAiMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        OpenAiMessage {
            role: "user".to_string(),
            content: user_text.to_string(),
        },
    ]
}

/// 构建仅用户消息（用于测试）
pub fn build_test_message(user_text: &str) -> Vec<OpenAiMessage> {
    vec![OpenAiMessage {
        role: "user".to_string(),
        content: user_text.to_string(),
    }]
}
