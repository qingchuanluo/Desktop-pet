//! 记忆系统模块
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct MemoryData {
    pub facts: HashMap<String, MemoryFact>,
    pub episodes: Vec<MemoryEpisode>,
    pub tasks: Vec<MemoryTask>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub value: String,
    pub confidence: f32,
    pub updated_at_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryEpisode {
    pub id: u64,
    pub summary: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MemoryTask {
    pub id: u64,
    pub title: String,
    pub status: String,
}

#[derive(Default)]
pub struct MemoryStore {
    data: MemoryData,
}

impl MemoryStore {
    pub fn new() -> Self {
        let mut s = Self::default();
        s.load_from_file();
        s
    }

    pub fn snapshot(&self) -> MemoryData {
        self.data.clone()
    }

    pub fn apply_user_message(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let id = (self.data.episodes.len() as u64) + 1;
        self.data.episodes.push(MemoryEpisode {
            id,
            summary: text.trim().to_string(),
            tags: vec!["user".to_string()],
        });
        self.save_to_file();
    }

    pub fn apply_exchange(&mut self, user: &str, assistant: &str) {
        let id_u = (self.data.episodes.len() as u64) + 1;
        if !user.trim().is_empty() {
            self.data.episodes.push(MemoryEpisode {
                id: id_u,
                summary: user.trim().to_string(),
                tags: vec!["user".to_string()],
            });
        }
        if !assistant.trim().is_empty() {
            let id_a = id_u + 1;
            self.data.episodes.push(MemoryEpisode {
                id: id_a,
                summary: assistant.trim().to_string(),
                tags: vec!["assistant".to_string()],
            });
        }
        self.save_to_file();
    }

    pub fn build_memory_block(&self, query: &str) -> String {
        let mut lines = Vec::new();
        if !query.trim().is_empty() {
            lines.push(format!("Q={}", query.trim()));
        }
        for (k, v) in &self.data.facts {
            lines.push(format!("FACT {}: {} ({:.2})", k, v.value, v.confidence));
        }
        let recent = self.data.episodes.iter().rev().take(10);
        for e in recent {
            lines.push(format!("EP#{},{}: {}", e.id, e.tags.join(","), e.summary));
        }
        lines.join("\n")
    }

    pub fn clear(&mut self) {
        self.data = MemoryData::default();
        self.save_to_file();
    }

    fn load_from_file(&mut self) {
        let p = Path::new("memory").join("memory.json");
        if let Ok(s) = fs::read_to_string(&p) {
            if let Ok(d) = serde_json::from_str::<MemoryData>(&s) {
                self.data = d;
            }
        }
    }

    fn save_to_file(&self) {
        let dir = Path::new("memory");
        let _ = fs::create_dir_all(dir);
        let p = dir.join("memory.json");
        let _ = fs::write(
            p,
            serde_json::to_string_pretty(&self.data).unwrap_or_default(),
        );
    }
}
