//! Named Pipe 传输层实现（Windows）
//!
//! 使用 Windows Named Pipe API 实现进程间通信

use super::protocol::IpcMessage;
use std::io::{Read, Write};
use std::time::Duration;

/// Named Pipe 客户端
#[derive(Clone)]
pub struct PipeClient {
    pipe_name: String,
    timeout: Duration,
}

impl PipeClient {
    pub fn new(pipe_name: impl Into<String>) -> Self {
        Self {
            pipe_name: format!("\\\\.\\pipe\\{}", pipe_name.into()),
            timeout: Duration::from_secs(10),
        }
    }

    pub fn with_timeout(pipe_name: impl Into<String>, timeout: Duration) -> Self {
        Self {
            pipe_name: format!("\\\\.\\pipe\\{}", pipe_name.into()),
            timeout,
        }
    }

    /// 发送消息并等待响应
    #[cfg(windows)]
    pub fn send(&self, msg: &IpcMessage) -> std::io::Result<IpcMessage> {
        let mut file = self.open_pipe(true)?;
        self.send_over_read_write(&mut file, msg)
    }

    #[cfg(not(windows))]
    pub fn send(&self, _msg: &IpcMessage) -> std::io::Result<IpcMessage> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Named Pipe 仅支持 Windows",
        ))
    }

    #[cfg(windows)]
    fn open_pipe(&self, read_write: bool) -> std::io::Result<std::fs::File> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::{FromRawHandle, RawHandle};
        use std::time::Instant;
        use windows::Win32::Foundation::{GetLastError, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_MODE, OPEN_EXISTING,
        };

        let desired_access = if read_write {
            FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0
        } else {
            FILE_GENERIC_WRITE.0
        };

        let wide: Vec<u16> = OsStr::new(&self.pipe_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let start = Instant::now();
        loop {
            let handle = unsafe {
                CreateFileW(
                    windows::core::PCWSTR::from_raw(wide.as_ptr()),
                    desired_access,
                    FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    Default::default(),
                    None,
                )
            };

            match handle {
                Ok(h) => {
                    let file = unsafe { std::fs::File::from_raw_handle(h.0 as RawHandle) };
                    return Ok(file);
                }
                Err(_) => {
                    let err = unsafe { GetLastError() };
                    if (err == ERROR_PIPE_BUSY || err == ERROR_FILE_NOT_FOUND)
                        && start.elapsed() < self.timeout
                    {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
    }

    fn send_over_read_write<RW: Read + Write>(
        &self,
        rw: &mut RW,
        msg: &IpcMessage,
    ) -> std::io::Result<IpcMessage> {
        let json = serde_json::to_string(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let data = json.as_bytes();

        let len = (data.len() as u32).to_be_bytes();
        rw.write_all(&len)?;
        rw.write_all(data)?;
        rw.flush()?;

        let mut len_buf = [0u8; 4];
        rw.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut response_buf = vec![0u8; len];
        rw.read_exact(&mut response_buf)?;

        let response: IpcMessage = serde_json::from_slice(&response_buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(response)
    }

    /// 发送消息而不等待响应（单向）
    #[cfg(windows)]
    pub fn send_async(&self, msg: &IpcMessage) -> std::io::Result<()> {
        let mut file = self.open_pipe(false)?;
        let json = serde_json::to_string(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let data = json.as_bytes();
        let len = (data.len() as u32).to_be_bytes();
        file.write_all(&len)?;
        file.write_all(data)?;
        file.flush()?;
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn send_async(&self, _msg: &IpcMessage) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Named Pipe 仅支持 Windows",
        ))
    }
}

/// Named Pipe 服务端（供 Backend 使用）
pub struct PipeServer {
    pipe_name: String,
    instance: Option<windows::Win32::Foundation::HANDLE>,
}

impl PipeServer {
    pub fn new(pipe_name: impl Into<String>) -> Self {
        Self {
            pipe_name: format!("\\\\.\\pipe\\{}", pipe_name.into()),
            instance: None,
        }
    }

    /// 创建管道实例（供循环调用）
    #[cfg(windows)]
    pub fn create_instance(&mut self) -> std::io::Result<bool> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows::Win32::System::Pipes::{
            CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_WAIT,
        };

        let pipe_name: Vec<u16> = OsStr::new(&self.pipe_name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateNamedPipeW(
                windows::core::PCWSTR::from_raw(pipe_name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                1,
                4096,
                4096,
                0,
                None,
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        self.instance = Some(handle);
        Ok(true)
    }

    #[cfg(not(windows))]
    pub fn create_instance(&mut self) -> std::io::Result<bool> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Named Pipe 仅支持 Windows",
        ))
    }

    /// 尝试接受连接
    #[cfg(windows)]
    pub fn accept(&self) -> std::io::Result<Option<IpcMessage>> {
        use std::os::windows::io::{FromRawHandle, IntoRawHandle, RawHandle};
        use windows::Win32::Foundation::{GetLastError, ERROR_PIPE_CONNECTED};
        use windows::Win32::System::Pipes::ConnectNamedPipe;

        let handle = match &self.instance {
            Some(h) => *h,
            None => return Ok(None),
        };

        let connected = unsafe { ConnectNamedPipe(handle, None).is_ok() };
        if !connected {
            let err = unsafe { GetLastError() };
            if err != ERROR_PIPE_CONNECTED {
                return Ok(None);
            }
        }

        let mut file = unsafe { std::fs::File::from_raw_handle(handle.0 as RawHandle) };
        let mut len_buf = [0u8; 4];
        let read_result = file.read_exact(&mut len_buf);
        let _ = file.into_raw_handle();
        read_result?;

        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];

        let mut file = unsafe { std::fs::File::from_raw_handle(handle.0 as RawHandle) };
        let read_result = file.read_exact(&mut buf);
        let _ = file.into_raw_handle();
        read_result?;

        let msg: IpcMessage = serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Some(msg))
    }

    #[cfg(not(windows))]
    pub fn accept(&self) -> std::io::Result<Option<IpcMessage>> {
        Ok(None)
    }

    /// 发送响应
    #[cfg(windows)]
    pub fn respond(&self, msg: &IpcMessage) -> std::io::Result<()> {
        use std::os::windows::io::{FromRawHandle, IntoRawHandle, RawHandle};
        use windows::Win32::System::Pipes::DisconnectNamedPipe;

        let handle = match &self.instance {
            Some(h) => *h,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "未连接",
                ))
            }
        };

        let json = serde_json::to_string(msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let data = json.as_bytes();
        let len = (data.len() as u32).to_be_bytes();

        let mut buffer = Vec::with_capacity(4 + data.len());
        buffer.extend_from_slice(&len);
        buffer.extend_from_slice(data);

        let mut file = unsafe { std::fs::File::from_raw_handle(handle.0 as RawHandle) };
        let write_result = file.write_all(&buffer);
        let _ = file.into_raw_handle();
        write_result?;
        unsafe {
            DisconnectNamedPipe(handle)?;
        }

        Ok(())
    }

    #[cfg(not(windows))]
    pub fn respond(&self, _msg: &IpcMessage) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Named Pipe 仅支持 Windows",
        ))
    }
}
