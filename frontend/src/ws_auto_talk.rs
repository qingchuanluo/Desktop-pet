use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

pub struct AutoTalkWsMsg {
    pub ok: bool,
    pub source: String,
    pub level: Option<u32>,
    pub hunger: Option<i32>,
    pub text: Option<String>,
    pub error: Option<String>,
    pub recv_at: Instant,
}

#[derive(serde::Deserialize)]
struct WsPetStatsWire {
    hunger: i32,
}

#[derive(serde::Deserialize)]
struct WsAutoTalkEventWire {
    event: String,
    source: String,
    ok: bool,
    level: Option<u32>,
    text: String,
    stats: Option<WsPetStatsWire>,
    error: Option<String>,
    timestamp: i64,
}

pub enum WsClientCmd {
    PetClicked,
    LevelUp(u32),
    Feed { delta: i32, text: String },
    PersonaUpdated(Option<serde_json::Value>),
}

#[derive(serde::Serialize)]
struct WsClientEventWire {
    event: &'static str,
    level: Option<u32>,
    delta: Option<i32>,
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    personality: Option<serde_json::Value>,
}

fn ws_auto_talk_url(backend_base_url: &str) -> Option<String> {
    let base = backend_base_url.trim_end_matches('/');
    if let Some(rest) = base.strip_prefix("http://") {
        Some(format!("ws://{rest}/ws/auto_talk"))
    } else {
        base.strip_prefix("https://")
            .map(|rest| format!("wss://{rest}/ws/auto_talk"))
    }
}

pub fn spawn_auto_talk_ws_listener(
    backend_base_url: String,
    tx: Sender<AutoTalkWsMsg>,
    cmd_rx: Receiver<WsClientCmd>,
) {
    std::thread::spawn(move || {
        let ws_url = match ws_auto_talk_url(&backend_base_url) {
            Some(v) => v,
            None => return,
        };

        let mut connected_once = false;
        let mut last_personality: Option<serde_json::Value> = None;
        loop {
            match tungstenite::connect(ws_url.as_str()) {
                Ok((mut socket, _)) => {
                    if !connected_once {
                        println!("[ws_auto_talk] connected");
                        connected_once = true;
                    }
                    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_mut() {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                    }
                    if let Some(p) = last_personality.clone() {
                        let ev = WsClientEventWire {
                            event: "persona",
                            level: None,
                            delta: None,
                            text: None,
                            personality: Some(p),
                        };
                        if let Ok(s) = serde_json::to_string(&ev) {
                            let _ = socket.send(tungstenite::Message::Text(s.into()));
                        }
                    }
                    loop {
                        loop {
                            match cmd_rx.try_recv() {
                                Ok(WsClientCmd::PetClicked) => {
                                    println!("[ws_auto_talk] send cmd event=pet_clicked");
                                    let ev = WsClientEventWire {
                                        event: "pet_clicked",
                                        level: None,
                                        delta: None,
                                        text: None,
                                        personality: None,
                                    };
                                    if let Ok(s) = serde_json::to_string(&ev) {
                                        if socket
                                            .send(tungstenite::Message::Text(s.into()))
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                }
                                Ok(WsClientCmd::LevelUp(level)) => {
                                    println!(
                                        "[ws_auto_talk] send cmd event=level_up level={}",
                                        level
                                    );
                                    let ev = WsClientEventWire {
                                        event: "level_up",
                                        level: Some(level),
                                        delta: None,
                                        text: None,
                                        personality: None,
                                    };
                                    if let Ok(s) = serde_json::to_string(&ev) {
                                        if socket
                                            .send(tungstenite::Message::Text(s.into()))
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                }
                                Ok(WsClientCmd::Feed { delta, text }) => {
                                    println!("[ws_auto_talk] send cmd event=feed delta={}", delta);
                                    let ev = WsClientEventWire {
                                        event: "feed",
                                        level: None,
                                        delta: Some(delta),
                                        text: Some(text),
                                        personality: None,
                                    };
                                    if let Ok(s) = serde_json::to_string(&ev) {
                                        if socket
                                            .send(tungstenite::Message::Text(s.into()))
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                }
                                Ok(WsClientCmd::PersonaUpdated(p)) => {
                                    last_personality = p.clone();
                                    if let Some(p) = p {
                                        let ev = WsClientEventWire {
                                            event: "persona",
                                            level: None,
                                            delta: None,
                                            text: None,
                                            personality: Some(p),
                                        };
                                        if let Ok(s) = serde_json::to_string(&ev) {
                                            if socket
                                                .send(tungstenite::Message::Text(s.into()))
                                                .is_err()
                                            {
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                            }
                        }

                        match socket.read() {
                            Ok(tungstenite::Message::Binary(bin)) => {
                                if let Ok(ev) = bincode::deserialize::<WsAutoTalkEventWire>(&bin) {
                                    if ev.event != "auto_talk" {
                                        continue;
                                    }
                                    let hunger = ev.stats.as_ref().map(|s| s.hunger);
                                    let out = AutoTalkWsMsg {
                                        ok: ev.ok,
                                        source: ev.source,
                                        level: ev.level,
                                        hunger,
                                        text: if ev.text.is_empty() { None } else { Some(ev.text) },
                                        error: ev.error,
                                        recv_at: Instant::now(),
                                    };
                                    let _ = tx.send(out);
                                }
                            }
                            Ok(tungstenite::Message::Text(text)) => {
                                if let Ok(ev) = serde_json::from_str::<WsAutoTalkEventWire>(&text) {
                                    if ev.event != "auto_talk" {
                                        continue;
                                    }
                                    let hunger = ev.stats.as_ref().map(|s| s.hunger);
                                    let out = AutoTalkWsMsg {
                                        ok: ev.ok,
                                        source: ev.source,
                                        level: ev.level,
                                        hunger,
                                        text: if ev.text.is_empty() { None } else { Some(ev.text) },
                                        error: ev.error,
                                        recv_at: Instant::now(),
                                    };
                                    let _ = tx.send(out);
                                }
                            }
                            Ok(tungstenite::Message::Close(_)) => break,
                            Ok(_) => {}
                            Err(tungstenite::Error::Io(e))
                                if e.kind() == std::io::ErrorKind::WouldBlock =>
                            {
                                std::thread::sleep(Duration::from_millis(12));
                                continue;
                            }
                            Err(_) => break,
                        }
                    }
                }
                Err(_) => {}
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}
