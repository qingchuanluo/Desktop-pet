//! 系统监控模块 - CPU/内存/焦点窗口
use serde::Serialize;
use sysinfo::{get_current_pid, System};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

#[derive(Clone, Serialize)]
pub struct SystemMonitorData {
    pub cpu_usage: f64,
    pub memory_used: u64,
    pub memory_total: u64,
    pub memory_percent: f64,
    pub self_memory_used: u64,
    pub focused_window: Option<String>,
    pub process_count: u32,
}

pub struct SystemMonitor {
    sys: System,
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SystemMonitor {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self { sys }
    }

    pub fn get_data(&mut self) -> SystemMonitorData {
        self.sys.refresh_all();
        let cpu_usage = self.sys.global_cpu_usage() as f64;
        let memory_used = self.sys.used_memory();
        let memory_total = self.sys.total_memory();
        let memory_percent = if memory_total > 0 {
            (memory_used as f64 / memory_total as f64) * 100.0
        } else {
            0.0
        };
        let self_memory_used = get_current_pid()
            .ok()
            .and_then(|pid| self.sys.process(pid))
            .map(|p| p.memory())
            .unwrap_or(0);
        let process_count = self.sys.processes().len() as u32;
        let focused_window = Self::get_foreground_window_title();

        SystemMonitorData {
            cpu_usage,
            memory_used,
            memory_total,
            memory_percent,
            self_memory_used,
            focused_window,
            process_count,
        }
    }

    fn get_foreground_window_title() -> Option<String> {
        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.0 == 0 {
                return None;
            }
            let mut buf = vec![0u16; 256];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len <= 0 {
                return None;
            }
            let s = String::from_utf16_lossy(&buf[..len as usize]);
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
    }
}
