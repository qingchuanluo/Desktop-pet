//! Mod Loader（MOD 系统）
//!
//! 目标：支持加载“角色 MOD”和“功能 MOD”，形成可扩展生态。
//!
//! 规划职责：
//! - discovery：扫描 mods/ 与 characters/（本地目录或安装目录）
//! - manifest：读取 mod.json/character.json（版本、依赖、权限声明）
//! - install_uninstall：安装、卸载、启用/禁用、冲突检测与回滚

use crate::character::{CharacterMod, Personality};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static SKIN_REQUESTED: AtomicBool = AtomicBool::new(false);
static SKIN_NAME: Mutex<Option<String>> = Mutex::new(None);
static CHARACTER_REQUESTED: AtomicBool = AtomicBool::new(false);
static CHARACTER_ID: Mutex<Option<String>> = Mutex::new(None);

pub struct LoadedCharacter {
    pub char_mod: CharacterMod,
    pub skins: Vec<String>,
    pub current_skin: String,
    pub animations_dir: PathBuf,
}

#[derive(Clone, serde::Serialize)]
pub struct CharacterEntry {
    pub id: String,
    pub name: String,
}

fn assets_dir() -> PathBuf {
    if let Ok(v) = std::env::var("PET_ASSETS_DIR") {
        let p = PathBuf::from(v);
        if p.is_dir() {
            return p;
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cands = [
                dir.join("assets"),
                dir.join("..").join("assets"),
                dir.join("..").join("..").join("assets"),
            ];
            for p in cands {
                if p.is_dir() {
                    return p;
                }
            }
        }
    }

    [env!("CARGO_MANIFEST_DIR"), "assets"].into_iter().collect()
}

pub fn list_characters() -> Vec<CharacterEntry> {
    let root = assets_dir();
    let Ok(entries) = fs::read_dir(&root) else {
        return vec![];
    };

    let mut out: Vec<CharacterEntry> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.is_dir() {
                return None;
            }
            let id = p.file_name()?.to_str()?.to_string();
            let character_json = p.join("character.json");
            if !character_json.is_file() {
                return None;
            }
            let name = fs::read_to_string(&character_json)
                .ok()
                .and_then(|s| serde_json::from_str::<Personality>(&s).ok())
                .and_then(|p| p.name)
                .unwrap_or_else(|| id.clone());
            Some(CharacterEntry { id, name })
        })
        .collect();

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn load_character_from_env() -> LoadedCharacter {
    let default_character_dir = assets_dir().join("debug_pet");

    let character_dir = std::env::var("PET_CHARACTER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_character_dir);

    let char_mod = CharacterMod::load_from_dir(&character_dir);
    let skins = char_mod.list_skins();
    let current_skin = skins
        .first()
        .cloned()
        .unwrap_or_else(|| "default".to_string());
    let animations_dir = char_mod.animations_dir_for_skin(&current_skin);

    LoadedCharacter {
        char_mod,
        skins,
        current_skin,
        animations_dir,
    }
}

pub fn load_character_by_id(id: &str) -> LoadedCharacter {
    let character_dir = resolve_character_dir(id);
    let char_mod = CharacterMod::load_from_dir(&character_dir);
    let skins = char_mod.list_skins();
    let current_skin = skins
        .first()
        .cloned()
        .unwrap_or_else(|| "default".to_string());
    let animations_dir = char_mod.animations_dir_for_skin(&current_skin);
    LoadedCharacter {
        char_mod,
        skins,
        current_skin,
        animations_dir,
    }
}

fn resolve_character_dir(id: &str) -> PathBuf {
    let root = assets_dir();
    let dir = root.join(id);
    if dir.is_dir() {
        return dir;
    }
    root.join("debug_pet")
}

pub fn request_skin(skin: String) {
    if let Ok(mut g) = SKIN_NAME.lock() {
        *g = Some(skin);
    }
    SKIN_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn take_requested_skin() -> Option<String> {
    if !SKIN_REQUESTED.swap(false, Ordering::Relaxed) {
        return None;
    }
    let skin = SKIN_NAME
        .lock()
        .ok()
        .and_then(|mut g| g.take())
        .unwrap_or_else(|| "default".to_string());
    Some(skin)
}

pub fn request_character(id: String) {
    if let Ok(mut g) = CHARACTER_ID.lock() {
        *g = Some(id);
    }
    CHARACTER_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn take_requested_character() -> Option<String> {
    if !CHARACTER_REQUESTED.swap(false, Ordering::Relaxed) {
        return None;
    }
    CHARACTER_ID.lock().ok().and_then(|mut g| g.take())
}
