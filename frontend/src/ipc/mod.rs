//! 进程通信（IPC）
//!
//! 负责与后台 Daemon 通信：
//! - 传输层：先 TCP 127.0.0.1（易调试），再可选 Named Pipe（更贴近 Windows）
//! - 协议层：JSON 消息（serde）
//! - 对外能力：发送事件（click/drag/idle_tick 等），接收后台指令（动作/台词/情绪）

