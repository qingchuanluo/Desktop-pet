//! AI Persona（AI 角色人格系统）
//!
//! 目标：每个角色可携带独立的人格设定与提示词资源，做到“换角色即换人格”。
//!
//! 规划职责：
//! - prompt_assets：prompt.txt / personality.json 等资源加载与版本管理
//! - context_policy：上下文裁剪策略与角色记忆策略（与 Memory Service 协作）
//! - runtime_hooks：把人格资源注入到 ai_client 的请求构造流程中

