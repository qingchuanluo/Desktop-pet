use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Default)]
pub struct RealWorldSnapshot {
    pub hour: u8,
    pub bad_weather: bool,
}

pub static REALWORLD: Mutex<RealWorldSnapshot> = Mutex::new(RealWorldSnapshot {
    hour: 12,
    bad_weather: false,
});

pub fn spawn_realworld_poller(backend_base_url: String) {
    std::thread::spawn(move || loop {
        let url = format!("{}/api/realworld/info", backend_base_url.trim_end_matches('/'));
        if let Ok(resp) = reqwest::blocking::get(url) {
            if let Ok(v) = resp.json::<serde_json::Value>() {
                let time_str = v.get("time").and_then(|x| x.as_str()).unwrap_or("");
                let hour = time_str
                    .split(':')
                    .next()
                    .and_then(|s| s.parse::<u8>().ok())
                    .unwrap_or(12);
                let weather = v.get("weather").and_then(|x| x.as_str()).unwrap_or("");
                let bad_weather = weather.contains("雨")
                    || weather.contains("雪")
                    || weather.contains("🌧")
                    || weather.contains("⛈")
                    || weather.contains("❄");
                if let Ok(mut g) = REALWORLD.lock() {
                    g.hour = hour;
                    g.bad_weather = bad_weather;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(60));
    });
}
