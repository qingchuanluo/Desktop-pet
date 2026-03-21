use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Biography {
    pub identity: Option<String>,
    pub experience: Option<String>,
    pub belief: Option<String>,
    pub goal: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct RelationshipEntry {
    pub affection: Option<f32>,
    pub trust: Option<f32>,
    pub remarks: Option<Vec<String>>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct SpeechStyle {
    pub prefix_by_mood: Option<HashMap<String, String>>,
    pub default_phrases: Option<Vec<String>>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct CharacterTexts {
    pub hello: Option<String>,
    pub snack_received: Option<String>,
    pub event_phrases: Option<HashMap<String, Vec<String>>>,
    pub pet_clicked_phrases: Option<Vec<String>>,
    pub feed_phrases: Option<Vec<String>>,
    pub level_up_template: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Personality {
    pub id: Option<String>,
    pub name: Option<String>,
    pub biography: Option<Biography>,
    pub traits: Option<HashMap<String, f32>>,
    pub preferences: Option<HashMap<String, Vec<String>>>,
    pub relationship: Option<HashMap<String, RelationshipEntry>>,
    pub speech_style: Option<SpeechStyle>,
    pub texts: Option<CharacterTexts>,
}

#[derive(Clone)]
pub struct CharacterMod {
    pub name: String,
    pub base_dir: PathBuf,
    pub animations_dir: PathBuf,
    pub personality: Personality,
}

impl CharacterMod {
    pub fn load_from_dir(dir: &Path) -> Self {
        let base_dir = dir.to_path_buf();
        let manifest_path = dir.join("character.json");
        let personality = if let Ok(s) = fs::read_to_string(&manifest_path) {
            serde_json::from_str::<Personality>(&s).unwrap_or_default()
        } else {
            Personality::default()
        };
        let name = personality
            .name
            .clone()
            .or_else(|| {
                dir.file_name()
                    .and_then(|x| x.to_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "Default".to_string());
        let skins_dir = dir.join("skins");
        let animations_dir = if skins_dir.is_dir() {
            let default_skin = skins_dir.join("default").join("animations");
            if default_skin.is_dir() {
                default_skin
            } else {
                dir.join("animations")
            }
        } else {
            dir.join("animations")
        };
        Self {
            name,
            base_dir,
            animations_dir,
            personality,
        }
    }

    pub fn list_skins(&self) -> Vec<String> {
        let skins_dir = self.base_dir.join("skins");
        let Ok(entries) = fs::read_dir(&skins_dir) else {
            return vec!["default".to_string()];
        };

        let mut skins: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if !p.is_dir() {
                    return None;
                }
                p.file_name()
                    .and_then(|x| x.to_str())
                    .map(|s| s.to_string())
            })
            .collect();

        skins.sort();
        if skins.is_empty() {
            return vec!["default".to_string()];
        }

        if let Some(pos) = skins.iter().position(|s| s == "default") {
            let v = skins.remove(pos);
            skins.insert(0, v);
        } else if let Some(pos) = skins.iter().position(|s| s == "默认") {
            let v = skins.remove(pos);
            skins.insert(0, v);
        }

        skins
    }

    pub fn animations_dir_for_skin(&self, skin: &str) -> PathBuf {
        let skins_dir = self.base_dir.join("skins");
        let skin_root = skins_dir.join(skin);
        let try_skin = skin_root.join("animations");
        if try_skin.is_dir() {
            return try_skin;
        }
        if skin_root.is_dir() {
            let has_state_dir = ["idle", "walk", "walk_left", "walk_right", "relax", "sleep", "drag", "drag_left", "drag_right"]
                .into_iter()
                .any(|d| skin_root.join(d).is_dir());
            if has_state_dir {
                return skin_root;
            }
        }

        let default_skin = skins_dir.join("default").join("animations");
        if default_skin.is_dir() {
            return default_skin;
        }
        let default_skin_root = skins_dir.join("default");
        if default_skin_root.is_dir() {
            let has_state_dir = ["idle", "walk", "walk_left", "walk_right", "relax", "sleep", "drag", "drag_left", "drag_right"]
                .into_iter()
                .any(|d| default_skin_root.join(d).is_dir());
            if has_state_dir {
                return default_skin_root;
            }
        }

        let cn_default = skins_dir.join("默认").join("animations");
        if cn_default.is_dir() {
            return cn_default;
        }
        let cn_default_root = skins_dir.join("默认");
        if cn_default_root.is_dir() {
            let has_state_dir = ["idle", "walk", "walk_left", "walk_right", "relax", "sleep", "drag", "drag_left", "drag_right"]
                .into_iter()
                .any(|d| cn_default_root.join(d).is_dir());
            if has_state_dir {
                return cn_default_root;
            }
        }

        let legacy = self.base_dir.join("animations");
        if legacy.is_dir() {
            return legacy;
        }

        self.base_dir.clone()
    }
}
