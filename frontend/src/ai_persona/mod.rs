//! AI Persona（AI 角色人格系统）
//!
//! 目标：每个角色可携带独立的人格设定与提示词资源，做到“换角色即换人格”。
//!
//! 规划职责：
//! - prompt_assets：prompt.txt / personality.json 等资源加载与版本管理
//! - context_policy：上下文裁剪策略与角色记忆策略（与 Memory Service 协作）
//! - runtime_hooks：把人格资源注入到 ai_client 的请求构造流程中

use crate::character::CharacterMod;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

static BASE_PERSONALITY: Mutex<Option<serde_json::Value>> = Mutex::new(None);
static PROMPT_TEXT: Mutex<Option<String>> = Mutex::new(None);

pub fn set_base_personality_from_character(char_mod: &CharacterMod) {
    let v = serde_json::to_value(&char_mod.personality).ok();
    set_base_personality_value(v);

    let prompt = load_prompt_text(char_mod.base_dir.as_path());
    if let Ok(mut g) = PROMPT_TEXT.lock() {
        *g = prompt;
    }
}

pub fn set_base_personality_value(value: Option<serde_json::Value>) {
    if let Ok(mut g) = BASE_PERSONALITY.lock() {
        *g = value;
    }
}

pub fn base_personality_value() -> Option<serde_json::Value> {
    BASE_PERSONALITY.lock().ok().and_then(|g| g.clone())
}

#[allow(dead_code)]
pub fn prompt_text() -> Option<String> {
    PROMPT_TEXT.lock().ok().and_then(|g| g.clone())
}

pub fn compose_personality(pet_status: Option<serde_json::Value>) -> Option<serde_json::Value> {
    let mut p = base_personality_value();
    if let Some(sv) = pet_status {
        if let Some(mut pv) = p.take() {
            match &mut pv {
                serde_json::Value::Object(obj) => {
                    obj.insert("pet_status".to_string(), sv);
                    p = Some(pv);
                }
                _ => {
                    p = Some(serde_json::json!({
                        "pet_status": sv,
                        "base": pv
                    }));
                }
            }
        } else {
            p = Some(serde_json::json!({ "pet_status": sv }));
        }
    }
    p
}

fn load_prompt_text(character_dir: &Path) -> Option<String> {
    let path = character_dir.join("prompt.txt");
    let Ok(s) = fs::read_to_string(path) else {
        return None;
    };
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
