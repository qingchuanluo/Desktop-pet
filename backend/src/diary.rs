//! 日记系统模块
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, path::Path};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct DiaryData {
    pub entries: Vec<DiaryEntry>,
    pub last_log_ts_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiaryEntry {
    pub ts_ms: u64,
    pub text: String,
}

#[derive(Default)]
pub struct DiaryStore {
    data: DiaryData,
}

impl DiaryStore {
    pub fn new() -> Self {
        let mut s = Self::default();
        s.load_from_file();
        s
    }

    pub fn snapshot(&self) -> DiaryData {
        self.data.clone()
    }

    pub fn append(&mut self, text: String) -> DiaryEntry {
        let entry = DiaryEntry {
            ts_ms: now_ms(),
            text: text.trim().to_string(),
        };
        self.data.entries.push(entry.clone());
        if self.data.entries.len() > 500 {
            self.data.entries.drain(0..self.data.entries.len() - 500);
        }
        self.save_to_file();
        entry
    }

    pub fn clear(&mut self) {
        self.data = DiaryData::default();
        self.save_to_file();
    }

    fn load_from_file(&mut self) {
        let p = Path::new("memory").join("diary.json");
        if let Ok(s) = fs::read_to_string(&p) {
            if let Ok(d) = serde_json::from_str::<DiaryData>(&s) {
                self.data = d;
            }
        }
    }

    fn save_to_file(&self) {
        let dir = Path::new("memory");
        let _ = fs::create_dir_all(dir);
        let p = dir.join("diary.json");
        let _ = fs::write(
            p,
            serde_json::to_string_pretty(&self.data).unwrap_or_default(),
        );
    }
}
