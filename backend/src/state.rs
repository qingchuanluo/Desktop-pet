//! 状态管理模块
use crate::diary::DiaryStore;
use crate::memory::MemoryStore;
use crate::monitor::SystemMonitor;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
        let level = 0;
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
        let prev_level = self.level;
        if let Some(v) = level {
            self.level = v;
            if coins.is_none() && v > prev_level {
                let delta = v - prev_level;
                // 升级获得金币：每级 10 金币
                self.coins = self.coins.saturating_add(delta.saturating_mul(10));
            }
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
        let mut v = if path.exists() {
            let data = fs::read_to_string(path).unwrap_or_default();
            serde_json::from_str(&data).unwrap_or_else(|_| PetLevel::new())
        } else {
            PetLevel::new()
        };
        v.hunger = v.hunger.clamp(0, 100);
        v.xp_to_next = Self::xp_to_next_for(v.level);
        v
    }

    pub fn default_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("pet_level.json")
    }

    pub fn path_for_user(user_id: &str) -> PathBuf {
        if user_id.is_empty() || user_id == "guest" {
            return Self::default_path();
        }
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join(format!("pet_level_{}.json", user_id))
    }

    fn xp_to_next_for(level: u32) -> u32 {
        let _ = level;
        50
    }
}

impl Default for PetLevel {
    fn default() -> Self {
        Self::new()
    }
}

/// 桌宠端展示的「商城用户」信息（与养成金币无关；金币在 PetLevel）
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct StoreUser {
    #[serde(default)]
    pub user_id: String,
    /// 可选昵称；为空时用 user_id 作为展示
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

impl StoreUser {
    pub fn default_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data")
            .join("store_user.json")
    }

    pub fn load(path: &Path) -> Self {
        if path.exists() {
            let data = fs::read_to_string(path).unwrap_or_default();
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self {
                user_id: "guest".to_string(),
                display_name: None,
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, data)
    }

    /// 徽章/副标题用展示文案
    pub fn label(&self) -> &str {
        self.display_name
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(self.user_id.trim())
    }
}

#[derive(Deserialize)]
pub struct StoreUserUpdate {
    pub user_id: Option<String>,
    pub display_name: Option<String>,
}

pub struct AppState {
    pub config: Mutex<BackendConfig>,
    pub logs: Mutex<Vec<BackendLog>>,
    pub memory: Mutex<MemoryStore>,
    pub diary: Mutex<DiaryStore>,
    pub monitor: Mutex<SystemMonitor>,
    pub pet_levels: Mutex<HashMap<String, PetLevel>>,
    pub store_user: Mutex<StoreUser>,
    pub current_personality: Mutex<Option<serde_json::Value>>,
    pub current_personality_ts_ms: Mutex<u64>,
}

impl AppState {
    pub fn new(config: BackendConfig) -> Self {
        let store_path = StoreUser::default_path();
        let store_user = StoreUser::load(&store_path);

        // 初始化时预加载当前用户的 PetLevel
        let pet_path = PetLevel::path_for_user(&store_user.user_id);
        let pet_level = PetLevel::load(&pet_path);
        let mut pet_levels = HashMap::new();
        pet_levels.insert(store_user.user_id.clone(), pet_level);

        Self {
            config: Mutex::new(config),
            logs: Mutex::new(Vec::new()),
            memory: Mutex::new(MemoryStore::new()),
            diary: Mutex::new(DiaryStore::new()),
            monitor: Mutex::new(SystemMonitor::new()),
            pet_levels: Mutex::new(pet_levels),
            store_user: Mutex::new(store_user),
            current_personality: Mutex::new(None),
            current_personality_ts_ms: Mutex::new(0),
        }
    }

    /// 获取特定用户的 PetLevel
    pub fn get_pet_level(&self, user_id: &str) -> PetLevel {
        let mut levels = self.pet_levels.lock().unwrap();
        if let Some(level) = levels.get(user_id) {
            return level.clone();
        }

        // 如果没有缓存，则加载
        let path = PetLevel::path_for_user(user_id);
        let level = PetLevel::load(&path);
        levels.insert(user_id.to_string(), level.clone());
        level
    }

    /// 更新特定用户的 PetLevel
    pub fn update_pet_level(&self, user_id: &str, update: impl FnOnce(&mut PetLevel)) -> bool {
        let mut levels = self.pet_levels.lock().unwrap();
        let level = levels.entry(user_id.to_string()).or_insert_with(|| {
            let path = PetLevel::path_for_user(user_id);
            PetLevel::load(&path)
        });

        update(level);

        // 保存到磁盘
        let path = PetLevel::path_for_user(user_id);
        level.save(&path).is_ok()
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
            pet_levels: Mutex::new(self.pet_levels.lock().unwrap().clone()),
            store_user: Mutex::new(self.store_user.lock().unwrap().clone()),
            current_personality: Mutex::new(self.current_personality.lock().unwrap().clone()),
            current_personality_ts_ms: Mutex::new(*self.current_personality_ts_ms.lock().unwrap()),
        }
    }
}
