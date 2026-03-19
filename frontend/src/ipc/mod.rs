//! 进程通信（IPC）
//!
//! 负责与后台 Daemon 通信：
//! - 传输层：TCP、Named Pipe、HTTP
//! - 协议层：JSON 消息（serde）
//! - 对外能力：发送事件（click/drag/idle_tick 等），接收后台指令（动作/台词/情绪）
//!
//! ## 模块结构
//!
//! ```text
//! ipc/
//! ├── mod.rs        # 主模块，导出子模块
//! ├── protocol.rs   # 协议定义（消息结构）
//! ├── tcp.rs        # TCP 传输层
//! ├── named_pipe.rs # Named Pipe 传输层（Windows）
//! ├── http.rs       # HTTP 传输层
//! └── client.rs     # 统一客户端接口 + 路由
//! ```
//!
//! ## 使用示例
//!
//! ```rust
//! use ipc::{create_default_client, IpcMessage};
//!
//! // 创建客户端
//! let client = create_default_client();
//!
//! // 发送聊天消息
//! let msg = IpcMessage::new_request("chat", "post", serde_json::json!({"text": "你好"}));
//! let response = client.async_send(&msg).await?;
//! ```

pub mod client;
pub mod http;
pub mod named_pipe;
pub mod protocol;
pub mod tcp;

pub use client::{
    create_default_client, create_pipe_client, create_tcp_client, global_client,
    with_thread_client, IpcClient, IpcConfig, IpcRouter, TransportType,
};
pub use protocol::{
    ApiOk, BackendConfig, BackendConfigUpdate, BackendLog, ChatReq, ChatResp, DiaryAppendReq,
    DiaryData, DiaryEntry, DiaryResp, DiarySummarizeResp, Direction, FrontendEvent, IpcMessage,
    MemoryData, MemoryResp, PetLevel, PetLevelUpdate, SystemMonitorData, TestReq, TestResp,
};
