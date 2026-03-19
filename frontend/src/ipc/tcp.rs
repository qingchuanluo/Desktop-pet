//! TCP 传输层实现

use super::protocol::IpcMessage;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// TCP 客户端
#[derive(Clone)]
pub struct TcpClient {
    address: String,
    timeout: Duration,
}

impl TcpClient {
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            timeout: Duration::from_secs(10),
        }
    }

    pub fn with_timeout(address: impl Into<String>, timeout: Duration) -> Self {
        Self {
            address: address.into(),
            timeout,
        }
    }

    /// 发送消息并等待响应
    pub fn send(&self, msg: &IpcMessage) -> std::io::Result<IpcMessage> {
        let mut stream = TcpStream::connect(&self.address)?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;

        // 序列化消息
        let json = serde_json::to_string(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let data = json.as_bytes();

        // 发送长度前缀（4 字节大端序）
        let len = (data.len() as u32).to_be_bytes();
        stream.write_all(&len)?;
        stream.write_all(data)?;

        // 读取响应
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut response_buf = vec![0u8; len];
        stream.read_exact(&mut response_buf)?;

        let response: IpcMessage = serde_json::from_slice(&response_buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        Ok(response)
    }

    /// 发送消息而不等待响应（单向）
    pub fn send_async(&self, msg: &IpcMessage) -> std::io::Result<()> {
        let mut stream = TcpStream::connect(&self.address)?;
        stream.set_write_timeout(Some(self.timeout))?;

        let json = serde_json::to_string(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let data = json.as_bytes();

        let len = (data.len() as u32).to_be_bytes();
        stream.write_all(&len)?;
        stream.write_all(data)?;

        Ok(())
    }
}

/// TCP 服务端（供 Backend 使用）
pub struct TcpServer {
    listener: std::net::TcpListener,
}

impl TcpServer {
    pub fn bind(address: impl Into<String>) -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind(address.into())?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener })
    }

    /// 接受连接并返回消息
    pub fn accept(&self) -> std::io::Result<Option<(IpcMessage, TcpStream)>> {
        match self.listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(30)))?;

                let mut len_buf = [0u8; 4];
                match stream.read_exact(&mut len_buf) {
                    Ok(_) => {
                        let len = u32::from_be_bytes(len_buf) as usize;
                        let mut buf = vec![0u8; len];
                        stream.read_exact(&mut buf)?;
                        match serde_json::from_slice::<IpcMessage>(&buf) {
                            Ok(msg) => Ok(Some((msg, stream))),
                            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 发送响应
    pub fn respond(&self, stream: &mut TcpStream, msg: &IpcMessage) -> std::io::Result<()> {
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let json = serde_json::to_string(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let data = json.as_bytes();
        let len = (data.len() as u32).to_be_bytes();
        stream.write_all(&len)?;
        stream.write_all(data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::protocol::Direction;

    #[test]
    fn test_message_serde() {
        let msg = IpcMessage::new_request("chat", "send", serde_json::json!({"text": "hello"}));
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: IpcMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.module, "chat");
        assert_eq!(parsed.action, "send");
        assert_eq!(parsed.direction, Direction::Request);
    }
}
