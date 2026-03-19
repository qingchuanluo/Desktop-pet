//! AI 桌宠后台服务库
pub mod diary;
pub mod memory;
pub mod monitor;
pub mod state;

// 引用独立服务
pub use chat_service::{
    build_chat_messages, build_test_message, call_ai_openai_compat, AiConfig, AiError,
    OpenAiMessage, OpenAiReq, OpenAiResp,
};
