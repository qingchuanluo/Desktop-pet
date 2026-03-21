//! HTTP 传输层实现
//!
//! 封装现有的 HTTP 调用，提供统一的 IPC 接口

use super::protocol::IpcMessage;
use reqwest::Client;
use reqwest::Method;
use std::time::Duration;

/// HTTP 客户端
pub struct HttpClient {
    base_url: String,
    client: Client,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, Duration::from_secs(30))
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
        }
    }

    /// 发送消息并等待响应
    /// 自动将 module/action 映射到对应的 HTTP 端点
    pub async fn send(&self, msg: &IpcMessage) -> Result<IpcMessage, String> {
        let (method, endpoint, with_body) = self.route(&msg.module, &msg.action)?;
        let url = format!("{}{}", self.base_url, endpoint);

        let request = self.client.request(method, &url);
        let request = if with_body {
            request.json(&msg.payload)
        } else {
            request
        };
        let response = request.send().await;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let payload: serde_json::Value = resp
                        .json()
                        .await
                        .map_err(|e| format!("Invalid JSON response: {}", e))?;
                    Ok(IpcMessage::new_response(
                        &msg.id,
                        &msg.module,
                        &msg.action,
                        payload,
                    ))
                } else {
                    Err(format!("HTTP error: {}", resp.status()))
                }
            }
            Err(e) => Err(format!("Request error: {}", e)),
        }
    }

    fn route(&self, module: &str, action: &str) -> Result<(Method, &'static str, bool), String> {
        match (module, action) {
            ("config", "get") => Ok((Method::GET, "/api/config", false)),
            ("config", "post") => Ok((Method::POST, "/api/config", true)),
            ("logs", "get") => Ok((Method::GET, "/api/logs", false)),
            ("monitor", "get") => Ok((Method::GET, "/api/monitor", false)),

            ("memory", "get") => Ok((Method::GET, "/api/memory", false)),
            ("memory", "clear") => Ok((Method::POST, "/api/memory/clear", true)),

            ("pet_level", "get") => Ok((Method::GET, "/api/pet_level", false)),
            ("pet_level", "post") => Ok((Method::POST, "/api/pet_level", true)),

            ("store_user", "get") => Ok((Method::GET, "/api/store/user", false)),
            ("store_user", "post") => Ok((Method::POST, "/api/store/user", true)),

            ("diary", "get") => Ok((Method::GET, "/api/diary", false)),
            ("diary", "append") => Ok((Method::POST, "/api/diary/append", true)),
            ("diary", "clear") => Ok((Method::POST, "/api/diary/clear", true)),
            ("diary", "summarize") => Ok((Method::POST, "/api/diary/summarize", true)),

            ("file", "summarize") => Ok((Method::POST, "/api/file/summarize", true)),
            ("auto_talk", "post") => Ok((Method::POST, "/api/auto_talk", true)),

            ("chat", "post") => Ok((Method::POST, "/api/chat", true)),
            ("test", "post") => Ok((Method::POST, "/api/test", true)),
            _ => Err(format!("Unknown HTTP route: {}/{}", module, action)),
        }
    }

    /// 便捷方法：直接调用特定 API
    pub async fn get_config(&self) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("config", "get", serde_json::json!({}));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn update_config(
        &self,
        update: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("config", "post", update);
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn get_logs(&self) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("logs", "get", serde_json::json!({}));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn get_monitor(&self) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("monitor", "get", serde_json::json!({}));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn get_memory(&self) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("memory", "get", serde_json::json!({}));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn clear_memory(&self) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("memory", "clear", serde_json::json!({}));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn get_diary(&self) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("diary", "get", serde_json::json!({}));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn append_diary(&self, text: String) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("diary", "append", serde_json::json!({ "text": text }));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn chat(&self, text: String) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("chat", "post", serde_json::json!({ "text": text }));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }

    pub async fn test(&self, text: String) -> Result<serde_json::Value, String> {
        let msg = IpcMessage::new_request("test", "post", serde_json::json!({ "text": text }));
        let resp = self.send(&msg).await?;
        Ok(resp.payload)
    }
}
