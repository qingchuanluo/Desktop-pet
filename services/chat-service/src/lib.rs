//! Chat Service
//!
//! 负责对话编排与大模型调用：
//! - Prompt 组装（系统提示词/角色设定/上下文裁剪）
//! - 工具调用/函数调用的调度
//! - 与 Memory Service 协作做检索增强
//! - 与 Voice Service 协作做语音输入/输出（可选）

