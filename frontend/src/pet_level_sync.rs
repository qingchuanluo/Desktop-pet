use crate::ipc::{global_client, IpcMessage};
use crate::pet_stats::PET_STATS;
use std::env;
use std::sync::mpsc::Receiver;
use std::time::Duration;

#[derive(serde::Deserialize)]
pub struct BackendPetLevelResp {
    pub level: u32,
    pub xp: u32,
    pub hunger: i32,
    pub coins: Option<u32>,
}

#[derive(Clone, serde::Serialize)]
pub struct BackendPetLevelUpdate {
    pub user_id: String,
    pub level: u32,
    pub xp: u32,
    pub hunger: i32,
    pub coins: Option<u32>,
}

pub fn fetch_store_user_id() -> String {
    let msg = IpcMessage::new_request("store_user", "get", serde_json::json!({}));
    match global_client().send(&msg) {
        Ok(r) => r
            .payload
            .get("user_id")
            .and_then(|x| x.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("guest")
            .to_string(),
        Err(_) => "guest".to_string(),
    }
}

pub fn backend_bind() -> String {
    env::var("BACKEND_BIND").unwrap_or_else(|_| "127.0.0.1:4317".to_string())
}

pub fn backend_base_url() -> String {
    env::var("BACKEND_URL").unwrap_or_else(|_| format!("http://{}", backend_bind()))
}

pub fn init_from_backend() {
    let ipc_client = global_client();
    let msg = IpcMessage::new_request("pet_level", "get", serde_json::json!({}));
    if let Ok(r) = ipc_client.send(&msg) {
        if let Ok(v) = serde_json::from_value::<BackendPetLevelResp>(r.payload) {
            if let Ok(mut s) = PET_STATS.lock() {
                s.level = v.level;
                s.xp = v.xp;
                s.hunger = v.hunger.clamp(0, 100);
                s.coins = v.coins.unwrap_or(0);
                s.hunger_acc_ms = 0;
                s.xp_acc_ms = 0;
                s.sleep_roll_acc_ms = 0;
                s.dirty = false;
            }
        }
    }
}

pub fn spawn_saver(rx: Receiver<BackendPetLevelUpdate>) {
    std::thread::spawn(move || {
        let ipc_client = global_client();
        while let Ok(payload) = rx.recv() {
            let msg = IpcMessage::new_request(
                "pet_level",
                "post",
                serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
            );
            if let Ok(resp) = ipc_client.send(&msg) {
                if let Some(level_val) = resp.payload.get("level") {
                    if let Ok(v) = serde_json::from_value::<BackendPetLevelResp>(level_val.clone()) {
                        if let Ok(mut s) = PET_STATS.lock() {
                            s.level = v.level;
                            s.xp = v.xp;
                            s.hunger = v.hunger.clamp(0, 100);
                            if let Some(c) = v.coins {
                                s.coins = c;
                            }
                            s.dirty = false;
                        }
                    }
                }
            }
        }
    });
}

pub fn spawn_poller() {
    std::thread::spawn(move || {
        let ipc_client = global_client();
        loop {
            std::thread::sleep(Duration::from_secs(8));
            let msg = IpcMessage::new_request("pet_level", "get", serde_json::json!({}));
            if let Ok(r) = ipc_client.send(&msg) {
                if let Ok(v) = serde_json::from_value::<BackendPetLevelResp>(r.payload) {
                    if let Ok(mut s) = PET_STATS.lock() {
                        if !s.dirty {
                            s.level = v.level;
                            s.xp = v.xp;
                            s.hunger = v.hunger.clamp(0, 100);
                            s.coins = v.coins.unwrap_or(s.coins);
                        }
                    }
                }
            }
        }
    });
}

pub fn fetch_once_apply() -> bool {
    let ipc_client = global_client();
    let msg = IpcMessage::new_request("pet_level", "get", serde_json::json!({}));
    if let Ok(r) = ipc_client.send(&msg) {
        if let Ok(v) = serde_json::from_value::<BackendPetLevelResp>(r.payload) {
            if let Ok(mut s) = PET_STATS.lock() {
                s.level = v.level;
                s.xp = v.xp;
                s.hunger = v.hunger.clamp(0, 100);
                s.coins = v.coins.unwrap_or(s.coins);
                s.dirty = false;
            }
            return true;
        }
    }
    false
}
