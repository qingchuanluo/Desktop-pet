//! IPC 协议定义
//!
//! 消息格式：JSON
//! {
//!     "id": "uuid-v4",
//!     "module": "chat|memory|diary|monitor|...",
//!     "action": "get|set|post|clear|...",
//!     "payload": { ... },
//!     "response": { ... }  // 仅响应消息有
//! }

use serde::{Deserialize, Serialize};

/// IPC 消息方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Request,
    Response,
    Event, // 单向事件，无需响应
}

/// IPC 消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcMessage {
    /// 消息唯一标识
    pub id: String,
    /// 模块名
    pub module: String,
    /// 操作类型
    pub action: String,
    /// 消息方向
    #[serde(default)]
    pub direction: Direction,
    /// 请求/响应数据
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl IpcMessage {
    pub fn new_request(
        module: impl Into<String>,
        action: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: uuid_v4(),
            module: module.into(),
            action: action.into(),
            direction: Direction::Request,
            payload,
        }
    }

    pub fn new_response(
        id: impl Into<String>,
        module: impl Into<String>,
        action: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            module: module.into(),
            action: action.into(),
            direction: Direction::Response,
            payload,
        }
    }

    pub fn new_event(
        module: impl Into<String>,
        action: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: uuid_v4(),
            module: module.into(),
            action: action.into(),
            direction: Direction::Event,
            payload,
        }
    }
}

/// 生成简单 UUID（不依赖外部 crate）
fn uuid_v4() -> String {
    use rand::RngCore;

    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);

    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

// ============ 通用请求/响应 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiOk {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub bind: String,
    pub base_url: String,
    pub model: String,
    pub system_prompt: String,
    #[serde(default)]
    pub api_key_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfigUpdate {
    pub bind: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendLog {
    pub ts_ms: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMonitorData {
    pub cpu_usage: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub memory_percent: f64,
    pub focused_window: Option<String>,
    pub process_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryData {
    #[serde(default)]
    pub events: Vec<serde_json::Value>,
    #[serde(default)]
    pub exchanges: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResp {
    pub path: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryEntry {
    pub ts_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryData {
    #[serde(default)]
    pub entries: Vec<DiaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryResp {
    pub path: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryAppendReq {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarySummarizeResp {
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatReq {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResp {
    pub ok: bool,
    pub reply: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReq {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResp {
    pub ok: bool,
    pub reply: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetLevel {
    pub level: u32,
    pub xp: u32,
    pub xp_to_next: u32,
    pub hunger: i32,
    #[serde(default)]
    pub coins: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetLevelUpdate {
    pub level: Option<u32>,
    pub xp: Option<u32>,
    pub hunger: Option<i32>,
    pub coins: Option<u32>,
}

// ============ Frontend -> Backend 事件 ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendEvent {
    pub event_type: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub timestamp: u64,
}

impl FrontendEvent {
    pub fn click(x: i32, y: i32) -> Self {
        Self {
            event_type: "click".to_string(),
            x: Some(x),
            y: Some(y),
            timestamp: now_ms(),
        }
    }

    pub fn double_click(x: i32, y: i32) -> Self {
        Self {
            event_type: "double_click".to_string(),
            x: Some(x),
            y: Some(y),
            timestamp: now_ms(),
        }
    }

    pub fn drag_start(x: i32, y: i32) -> Self {
        Self {
            event_type: "drag_start".to_string(),
            x: Some(x),
            y: Some(y),
            timestamp: now_ms(),
        }
    }

    pub fn drag_end(x: i32, y: i32) -> Self {
        Self {
            event_type: "drag_end".to_string(),
            x: Some(x),
            y: Some(y),
            timestamp: now_ms(),
        }
    }

    pub fn idle_tick() -> Self {
        Self {
            event_type: "idle_tick".to_string(),
            x: None,
            y: None,
            timestamp: now_ms(),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
