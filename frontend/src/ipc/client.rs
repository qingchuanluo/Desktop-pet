//! 统一 IPC 客户端接口
//!
//! 支持多种传输层：TCP、Named Pipe、HTTP
//! 支持消息路由：自动将消息发送到对应的处理模块

use super::protocol::IpcMessage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::RwLock;

static GLOBAL_CLIENT: OnceLock<IpcClient> = OnceLock::new();

thread_local! {
    static THREAD_RT: RefCell<Option<tokio::runtime::Runtime>> = const { RefCell::new(None) };
    static THREAD_CLIENT: RefCell<Option<IpcClient>> = const { RefCell::new(None) };
}

fn backend_bind() -> String {
    std::env::var("BACKEND_BIND").unwrap_or_else(|_| "127.0.0.1:4317".to_string())
}

fn backend_base_url() -> String {
    std::env::var("BACKEND_URL").unwrap_or_else(|_| format!("http://{}", backend_bind()))
}

fn default_http_config() -> IpcConfig {
    IpcConfig {
        transport: TransportType::Http,
        address: backend_base_url(),
        timeout: Duration::from_secs(30),
    }
}

fn with_thread_runtime<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&tokio::runtime::Runtime) -> R,
{
    THREAD_RT.with(|cell| {
        let mut guard = cell.borrow_mut();
        if guard.is_none() {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            *guard = Some(rt);
        }
        Ok(f(guard.as_ref().expect("runtime must exist")))
    })
}

pub fn global_client() -> &'static IpcClient {
    GLOBAL_CLIENT.get_or_init(|| IpcClient::new(default_http_config()))
}

pub fn with_thread_client<F, R>(f: F) -> R
where
    F: FnOnce(&IpcClient) -> R,
{
    THREAD_CLIENT.with(|cell| {
        let mut guard = cell.borrow_mut();
        if guard.is_none() {
            *guard = Some(IpcClient::new(default_http_config()));
        }
        f(guard.as_ref().expect("client must exist"))
    })
}

/// 传输层类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    Http,
    Tcp,
    NamedPipe,
}

/// IPC 客户端配置
#[derive(Clone)]
pub struct IpcConfig {
    pub transport: TransportType,
    pub address: String, // TCP/NamedPipe 地址 或 HTTP base URL
    pub timeout: Duration,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            transport: TransportType::Http,
            address: "http://127.0.0.1:4317".to_string(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// 统一 IPC 客户端
pub struct IpcClient {
    config: IpcConfig,
    http_client: super::http::HttpClient,
    tcp_client: super::tcp::TcpClient,
    pipe_client: super::named_pipe::PipeClient,
}

impl IpcClient {
    pub fn new(config: IpcConfig) -> Self {
        let http_address = if config.address.starts_with("http") {
            config.address.clone()
        } else {
            format!("http://{}", config.address)
        };

        Self {
            config: config.clone(),
            http_client: super::http::HttpClient::with_timeout(&http_address, config.timeout),
            tcp_client: super::tcp::TcpClient::with_timeout(&config.address, config.timeout),
            pipe_client: super::named_pipe::PipeClient::with_timeout(
                &config.address,
                config.timeout,
            ),
        }
    }

    /// 发送消息（同步）
    pub fn send(&self, msg: &IpcMessage) -> Result<IpcMessage, String> {
        match self.config.transport {
            TransportType::Http => {
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.block_on(self.http_client.send(msg))
                } else {
                    with_thread_runtime(|rt| rt.block_on(self.http_client.send(msg)))?
                }
            }
            TransportType::Tcp => self.tcp_client.send(msg).map_err(|e| e.to_string()),
            TransportType::NamedPipe => {
                #[cfg(windows)]
                {
                    self.pipe_client.send(msg).map_err(|e| e.to_string())
                }
                #[cfg(not(windows))]
                {
                    Err("Named Pipe 仅支持 Windows".to_string())
                }
            }
        }
    }

    /// 发送消息（异步）
    pub async fn async_send(&self, msg: &IpcMessage) -> Result<IpcMessage, String> {
        match self.config.transport {
            TransportType::Http => self.http_client.send(msg).await,
            TransportType::Tcp => {
                // TCP 客户端目前是同步的，包装成 async
                let client = self.tcp_client.clone();
                let msg = msg.clone();
                tokio::task::spawn_blocking(move || client.send(&msg))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())
            }
            TransportType::NamedPipe => {
                #[cfg(windows)]
                {
                    let client = self.pipe_client.clone();
                    let msg = msg.clone();
                    tokio::task::spawn_blocking(move || client.send(&msg))
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())
                }
                #[cfg(not(windows))]
                {
                    Err("Named Pipe 仅支持 Windows".to_string())
                }
            }
        }
    }

    /// 发送事件（单向，无需响应）
    pub fn send_event(&self, msg: &IpcMessage) -> Result<(), String> {
        match self.config.transport {
            TransportType::Http => {
                // HTTP 不支持单向事件
                Err("HTTP 不支持单向事件".to_string())
            }
            TransportType::Tcp => self.tcp_client.send_async(msg).map_err(|e| e.to_string()),
            TransportType::NamedPipe => {
                #[cfg(windows)]
                {
                    self.pipe_client.send_async(msg).map_err(|e| e.to_string())
                }
                #[cfg(not(windows))]
                {
                    Err("Named Pipe 仅支持 Windows".to_string())
                }
            }
        }
    }

    /// 便捷方法：发送前端事件
    pub fn send_frontend_event(
        &self,
        event_type: &str,
        x: Option<i32>,
        y: Option<i32>,
    ) -> Result<(), String> {
        let msg = IpcMessage::new_event(
            "frontend",
            event_type,
            serde_json::json!({
                "event_type": event_type,
                "x": x,
                "y": y,
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64
            }),
        );
        self.send_event(&msg)
    }
}

// ============ 消息路由器 ============

/// 消息处理器类型
type MessageHandler = Box<dyn Fn(IpcMessage) -> Result<IpcMessage, String> + Send + Sync>;

/// 消息路由器
pub struct IpcRouter {
    handlers: HashMap<String, HashMap<String, MessageHandler>>,
    client: Arc<RwLock<Option<IpcClient>>>,
}

impl IpcRouter {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            client: Arc::new(RwLock::new(None)),
        }
    }

    /// 设置 IPC 客户端
    pub async fn set_client(&self, client: IpcClient) {
        *self.client.write().await = Some(client);
    }

    /// 注册消息处理器
    pub fn register<F>(&mut self, module: &str, action: &str, handler: F)
    where
        F: Fn(IpcMessage) -> Result<IpcMessage, String> + Send + Sync + 'static,
    {
        self.handlers
            .entry(module.to_string())
            .or_default()
            .insert(action.to_string(), Box::new(handler));
    }

    /// 注册处理器（简化写法）
    pub fn on<F>(&mut self, module_action: &str, handler: F)
    where
        F: Fn(IpcMessage) -> Result<IpcMessage, String> + Send + Sync + 'static,
    {
        let parts: Vec<&str> = module_action.split('/').collect();
        if parts.len() == 2 {
            self.register(parts[0], parts[1], handler);
        }
    }

    /// 处理消息
    pub fn handle(&self, msg: IpcMessage) -> Result<IpcMessage, String> {
        let module = &msg.module;
        let action = &msg.action;

        if let Some(actions) = self.handlers.get(module) {
            if let Some(handler) = actions.get(action) {
                return handler(msg);
            }
        }

        Err(format!("No handler for {}/{}", module, action))
    }

    /// 处理消息（异步）
    pub async fn handle_async(&self, msg: IpcMessage) -> Result<IpcMessage, String> {
        if let Some(client_guard) = self.client.read().await.as_ref() {
            // 尝试发送到 Backend
            return client_guard.async_send(&msg).await;
        }

        // 本地处理
        self.handle(msg)
    }
}

impl Default for IpcRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ============ 便捷函数 ============

/// 创建默认客户端（HTTP）
pub fn create_default_client() -> IpcClient {
    IpcClient::new(IpcConfig::default())
}

/// 创建 TCP 客户端
pub fn create_tcp_client(address: &str) -> IpcClient {
    IpcClient::new(IpcConfig {
        transport: TransportType::Tcp,
        address: address.to_string(),
        timeout: Duration::from_secs(10),
    })
}

/// 创建 Named Pipe 客户端
pub fn create_pipe_client(pipe_name: &str) -> IpcClient {
    IpcClient::new(IpcConfig {
        transport: TransportType::NamedPipe,
        address: pipe_name.to_string(),
        timeout: Duration::from_secs(10),
    })
}
