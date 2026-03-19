//! 状态管理模块
use crate::diary::DiaryStore;
use crate::memory::MemoryStore;
use crate::monitor::SystemMonitor;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub bind: String,
    pub base_url: String,
    pub model: String,
    pub system_prompt: String,
    pub api_key: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BackendConfigPublic {
    pub bind: String,
    pub base_url: String,
    pub model: String,
    pub system_prompt: String,
    pub api_key_set: bool,
}

#[derive(Deserialize)]
pub struct BackendConfigUpdate {
    pub bind: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct BackendLog {
    pub ts_ms: u64,
    pub level: String,
    pub message: String,
}

#[derive(Serialize, Deserialize)]
pub struct BackendChatReq {
    pub text: String,
    pub personality: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
pub struct BackendChatResp {
    pub ok: bool,
    pub reply: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct DiaryResp {
    pub path: String,
    pub data: serde_json::Value,
}

#[derive(Deserialize)]
pub struct DiaryAppendReq {
    pub text: String,
}

#[derive(Serialize)]
pub struct DiarySummarizeResp {
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct MemoryResp {
    pub path: String,
    pub data: serde_json::Value,
}

#[derive(Serialize)]
pub struct ApiOk {
    pub ok: bool,
}

#[derive(Serialize)]
pub struct BackendTestResp {
    pub ok: bool,
    pub reply: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PetLevel {
    pub level: u32,
    pub xp: u32,
    pub xp_to_next: u32,
    pub hunger: i32,
    #[serde(default)]
    pub coins: u32,
}

impl PetLevel {
    pub fn new() -> Self {
        let level = 1;
        Self {
            level,
            xp: 0,
            xp_to_next: Self::xp_to_next_for(level),
            hunger: 100,
            coins: 0,
        }
    }

    pub fn apply_update(
        &mut self,
        level: Option<u32>,
        xp: Option<u32>,
        hunger: Option<i32>,
        coins: Option<u32>,
    ) {
        if let Some(v) = level {
            self.level = v.max(1);
        }
        if let Some(v) = xp {
            self.xp = v;
        }
        if let Some(v) = hunger {
            self.hunger = v.clamp(0, 100);
        }
        if let Some(v) = coins {
            self.coins = v;
        }
        self.xp_to_next = Self::xp_to_next_for(self.level);
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, data)
    }

    pub fn load(path: &Path) -> Self {
        if path.exists() {
            let data = fs::read_to_string(path).unwrap_or_default();
            serde_json::from_str(&data).unwrap_or_else(|_| PetLevel::new())
        } else {
            PetLevel::new()
        }
    }

    pub fn default_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("pet_level.json")
    }

    fn xp_to_next_for(level: u32) -> u32 {
        100 + level.saturating_sub(1) * 20
    }
}

impl Default for PetLevel {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AppState {
    pub config: Mutex<BackendConfig>,
    pub logs: Mutex<Vec<BackendLog>>,
    pub memory: Mutex<MemoryStore>,
    pub diary: Mutex<DiaryStore>,
    pub monitor: Mutex<SystemMonitor>,
    pub pet_level: Mutex<PetLevel>,
}

impl AppState {
    pub fn new(config: BackendConfig) -> Self {
        let pet_path = PetLevel::default_path();
        let pet_level = PetLevel::load(&pet_path);
        Self {
            config: Mutex::new(config),
            logs: Mutex::new(Vec::new()),
            memory: Mutex::new(MemoryStore::new()),
            diary: Mutex::new(DiaryStore::new()),
            monitor: Mutex::new(SystemMonitor::new()),
            pet_level: Mutex::new(pet_level),
        }
    }
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            config: Mutex::new(self.config.lock().unwrap().clone()),
            logs: Mutex::new(Vec::new()),
            memory: Mutex::new(MemoryStore::new()),
            diary: Mutex::new(DiaryStore::new()),
            monitor: Mutex::new(SystemMonitor::new()),
            pet_level: Mutex::new(self.pet_level.lock().unwrap().clone()),
        }
    }
}
