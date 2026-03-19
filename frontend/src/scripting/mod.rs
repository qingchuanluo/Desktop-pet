//! Scripting（角色行为脚本系统）
//!
//! 目标：把角色行为逻辑从代码中抽离，使用脚本定义“何时播放什么动作/如何响应事件”，便于 MOD 作者定制。
//!
//! 规划职责：
//! - script_runtime：脚本运行时（Lua 等）与调用边界
//! - api_surface：暴露给脚本的安全 API（play_animation、emit_event、get_state 等）
//! - hot_reload：脚本热重载与错误隔离（脚本崩溃不影响主程序）

use crate::ipc::{global_client, IpcMessage};
use std::sync::mpsc::{Receiver, Sender};

#[derive(Clone)]
pub struct AiRequest {
    pub user_text: String,
    pub personality: Option<serde_json::Value>,
}

#[derive(Clone)]
pub struct AiResponse {
    pub assistant_text: String,
}

#[derive(serde::Serialize)]
pub struct BackendChatReq {
    pub text: String,
    pub personality: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
pub struct BackendChatResp {
    pub ok: bool,
    pub reply: Option<String>,
    pub error: Option<String>,
}

pub fn spawn_ai_worker(rx: Receiver<AiRequest>, tx: Sender<AiResponse>) {
    std::thread::spawn(move || {
        let ipc_client = global_client();

        loop {
            let Ok(req) = rx.recv() else {
                break;
            };

            let msg = IpcMessage::new_request(
                "chat",
                "post",
                serde_json::to_value(BackendChatReq {
                    text: req.user_text,
                    personality: req.personality,
                })
                .unwrap_or(serde_json::Value::Null),
            );

            let assistant_text = match ipc_client.send(&msg) {
                Ok(resp) => match serde_json::from_value::<BackendChatResp>(resp.payload) {
                    Ok(j) => {
                        if j.ok {
                            j.reply.unwrap_or_default()
                        } else {
                            format!(
                                "（AI 错误）{}",
                                j.error.unwrap_or_else(|| "unknown".to_string())
                            )
                        }
                    }
                    Err(e) => format!("（后端错误）解析失败：{e}"),
                },
                Err(e) => format!("（后端错误）{e}"),
            };

            let _ = tx.send(AiResponse { assistant_text });
        }
    });
}
