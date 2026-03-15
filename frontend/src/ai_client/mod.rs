//! AI Client（对话/网关通信）
//!
//! 负责与 Gateway API 交互，把桌宠侧的消息、状态、语音结果等发送到后端，并接收回复：
//! - chat_api：HTTP/REST 调用封装（请求/响应模型）
//! - websocket：长连接（流式回复、实时事件）
//! - message_queue：本地队列与重试（离线/抖动场景下的消息可靠性）

