//! AI 桌宠后台服务 - 主入口
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Query, State},
    http::header::{CACHE_CONTROL, CONTENT_TYPE},
    http::HeaderMap,
    http::StatusCode,
    response::Html,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chat_service::{build_chat_messages, build_test_message, call_ai_openai_compat, AiConfig};
use gateway_api::monitor::ProcessInfo;
use gateway_api::state::{
    ApiOk, AppState, BackendChatReq, BackendChatResp, BackendConfig, BackendConfigPublic,
    BackendConfigUpdate, BackendLog, BackendTestResp, DiaryAppendReq, DiaryResp, StoreUser,
    StoreUserUpdate,
    DiarySummarizeResp, MemoryResp, PetLevel,
};
use reqwest::Url;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::io::AsyncWriteExt;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use chrono::Datelike;

static INSTALL_JOB_COUNTER: AtomicU64 = AtomicU64::new(1);
static INSTALL_JOBS: OnceLock<Mutex<HashMap<String, InstallJobStatus>>> = OnceLock::new();

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn push_log(state: &Arc<AppState>, level: &str, message: String) {
    if let Ok(mut logs) = state.logs.lock() {
        logs.push(BackendLog {
            ts_ms: now_ms(),
            level: level.to_string(),
            message,
        });
        if logs.len() > 300 {
            let drain_count = logs.len() - 300;
            logs.drain(0..drain_count);
        }
    }
}

#[derive(Clone, serde::Serialize)]
struct InstallJobStatus {
    job_id: String,
    stage: String,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    done: bool,
    error: Option<String>,
    installed_character_id: Option<String>,
}

#[derive(serde::Deserialize)]
struct InstallStartReq {
    store_base_url: String,
    item_id: String,
    skin_name: Option<String>,
}

#[derive(serde::Deserialize)]
struct StoreAuthLoginReq {
    store_base_url: String,
    #[serde(alias = "userId")]
    user_id: String,
}

#[derive(serde::Serialize)]
struct InstallStartResp {
    job_id: String,
}

#[derive(serde::Serialize)]
struct InstallStatusResp {
    job_id: String,
    stage: String,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    percent: Option<f64>,
    done: bool,
    error: Option<String>,
    installed_character_id: Option<String>,
}

fn jobs() -> &'static Mutex<HashMap<String, InstallJobStatus>> {
    INSTALL_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn store_url(base: &str, segments: &[&str]) -> Result<Url, String> {
    let mut url = Url::parse(base).map_err(|e| format!("store_base_url 无效: {}", e))?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err("store_base_url 仅支持 http/https".to_string()),
    }
    if url.host_str().is_none() {
        return Err("store_base_url 缺少 host".to_string());
    }
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|_| "store_base_url 不支持作为 base".to_string())?;
        segs.pop_if_empty();
        segs.extend(segments.iter().copied());
    }
    Ok(url)
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
                dir.join("..").join("..").join("frontend").join("assets"),
            ];
            for p in cands {
                if p.is_dir() {
                    return p;
                }
            }
        }
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("frontend")
        .join("assets")
}

fn unzip_to_dir(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开 zip 失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {}", e))?;
    let max_files: usize = 10_000;
    let max_single_uncompressed: u64 = 200 * 1024 * 1024;
    let max_total_uncompressed: u64 = 800 * 1024 * 1024;
    let mut total_uncompressed: u64 = 0;
    for i in 0..archive.len() {
        if i >= max_files {
            return Err(format!("zip 文件过大：entry 数超过 {max_files}"));
        }
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip entry 失败: {}", e))?;
        let Some(name) = entry.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        let size = entry.size();
        if size > max_single_uncompressed {
            return Err(format!("zip entry 过大：{} bytes", size));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        if total_uncompressed > max_total_uncompressed {
            return Err("zip 解压总量过大，疑似压缩炸弹".to_string());
        }

        let out_path = dest_dir.join(&name);
        if entry.name().ends_with('/') {
            std::fs::create_dir_all(&out_path).map_err(|e| format!("创建目录失败: {}", e))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
        let mut out = std::fs::File::create(&out_path).map_err(|e| format!("创建文件失败: {}", e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("写文件失败: {}", e))?;
    }
    Ok(())
}

fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp")
}

fn find_cover_image_in_dir(dir: &Path) -> Option<PathBuf> {
    let preferred = ["cover.png", "cover.jpg", "cover.jpeg", "cover.webp"];
    for f in preferred {
        let p = dir.join(f);
        if p.is_file() {
            return Some(p);
        }
    }

    let mut candidates: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .map(|s| is_image_ext(&s.to_ascii_lowercase()))
                .unwrap_or(false)
        })
        .map(|p| {
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            (size, p)
        })
        .collect();

    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().next().map(|x| x.1)
}

fn find_cover_image_for_character(character_dir: &Path) -> Option<PathBuf> {
    if let Some(p) = find_cover_image_in_dir(character_dir) {
        return Some(p);
    }
    let skins_dir = character_dir.join("skins");
    if !skins_dir.is_dir() {
        return None;
    }
    for skin in ["default", "默认"] {
        let p = skins_dir.join(skin);
        if p.is_dir() {
            if let Some(img) = find_cover_image_in_dir(&p) {
                return Some(img);
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(&skins_dir) {
        for ent in entries.flatten() {
            let p = ent.path();
            if !p.is_dir() {
                continue;
            }
            if let Some(img) = find_cover_image_in_dir(&p) {
                return Some(img);
            }
        }
    }
    None
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn safe_child_dir(base: &Path, child: &str) -> Option<PathBuf> {
    if child.is_empty() {
        return None;
    }
    if child.contains('/') || child.contains('\\') || child.contains("..") {
        return None;
    }
    let p = base.join(child);
    if p.is_dir() { Some(p) } else { None }
}

#[derive(Clone, serde::Serialize)]
struct OwnedItemInfo {
    id: String,
    name: String,
    description: Option<String>,
    cover: String,
    skins_count: usize,
}

#[derive(Clone, serde::Serialize)]
struct OwnedSkinInfo {
    name: String,
    cover: String,
}

#[derive(serde::Serialize)]
struct OwnedItemsResp {
    items: Vec<OwnedItemInfo>,
}

#[derive(serde::Serialize)]
struct OwnedSkinsResp {
    item: OwnedItemInfo,
    skins: Vec<OwnedSkinInfo>,
}

fn is_builtin_character_id(id: &str) -> bool {
    matches!(id, "debug_pet" | "夕")
}

fn read_character_meta(character_dir: &Path, fallback_id: &str) -> (String, Option<String>) {
    let p = character_dir.join("character.json");
    let data = std::fs::read_to_string(p).ok();
    let v: serde_json::Value = data
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let name = v
        .get("name")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_id.to_string());
    let bio = v.get("biography");
    let desc = bio
        .and_then(|b| b.get("identity"))
        .and_then(|x| x.as_str())
        .or_else(|| bio.and_then(|b| b.get("experience")).and_then(|x| x.as_str()))
        .or_else(|| bio.and_then(|b| b.get("belief")).and_then(|x| x.as_str()))
        .or_else(|| bio.and_then(|b| b.get("goal")).and_then(|x| x.as_str()))
        .map(|s| s.to_string());
    (name, desc)
}

fn load_character_personality(character_id: &str) -> Option<serde_json::Value> {
    let base = assets_dir();
    let character_dir = safe_child_dir(&base, character_id)?;
    let p = character_dir.join("character.json");
    let raw = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&raw).ok()
}

fn list_owned_items() -> Vec<OwnedItemInfo> {
    let base = assets_dir();
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for ent in entries.flatten() {
            let dir = ent.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(id_os) = dir.file_name() else { continue };
            let Some(id) = id_os.to_str() else { continue };
            if id.starts_with('.') {
                continue;
            }
            let skins_dir = dir.join("skins");
            let has_skins = skins_dir.is_dir();
            let has_character_json = dir.join("character.json").is_file();
            let has_legacy_animations = dir.join("animations").is_dir();
            if !(has_skins || has_character_json || has_legacy_animations) {
                continue;
            }

            let (name, description) = read_character_meta(&dir, id);
            let skins_count = if has_skins {
                std::fs::read_dir(&skins_dir)
                    .ok()
                    .map(|it| it.flatten().filter(|e| e.path().is_dir()).count())
                    .unwrap_or(0)
            } else {
                0
            };
            let cover = format!("/api/owned/item-covers/{}", id);
            items.push(OwnedItemInfo {
                id: id.to_string(),
                name,
                description,
                cover,
                skins_count,
            });
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

async fn get_owned_items() -> impl IntoResponse {
    let items = list_owned_items();
    (StatusCode::OK, Json(OwnedItemsResp { items }))
}

async fn post_delete_owned_item(
    axum::extract::Path(item_id): axum::extract::Path<String>,
) -> axum::response::Response {
    if is_builtin_character_id(&item_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"ok": false, "error": "默认角色不可删除"})),
        )
            .into_response();
    }
    let base = assets_dir();
    let Some(character_dir) = safe_child_dir(&base, &item_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"ok": false, "error": "角色不存在"})),
        )
            .into_response();
    };
    match tokio::fs::remove_dir_all(&character_dir).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_owned_item_skins(
    axum::extract::Path(item_id): axum::extract::Path<String>,
) -> axum::response::Response {
    let base = assets_dir();
    let Some(character_dir) = safe_child_dir(&base, &item_id) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "角色不存在"}))).into_response();
    };

    let skins_dir = character_dir.join("skins");
    if !skins_dir.is_dir() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "角色不存在"}))).into_response();
    }

    let (name, description) = read_character_meta(&character_dir, &item_id);
    let cover = format!("/api/owned/item-covers/{}", item_id);
    let mut skins: Vec<OwnedSkinInfo> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&skins_dir) {
        for ent in entries.flatten() {
            let dir = ent.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(skin_os) = dir.file_name() else { continue };
            let Some(skin) = skin_os.to_str() else { continue };
            if skin.starts_with('.') {
                continue;
            }
            let cover = format!("/api/owned/covers/{}/{}", item_id, skin);
            skins.push(OwnedSkinInfo {
                name: skin.to_string(),
                cover,
            });
        }
    }
    skins.sort_by(|a, b| a.name.cmp(&b.name));

    (
        StatusCode::OK,
        Json(OwnedSkinsResp {
            item: OwnedItemInfo {
                id: item_id,
                name,
                description,
                cover,
                skins_count: skins.len(),
            },
            skins,
        }),
    )
        .into_response()
}

async fn get_owned_item_cover(
    axum::extract::Path(item_id): axum::extract::Path<String>,
) -> axum::response::Response {
    let base = assets_dir();
    let Some(character_dir) = safe_child_dir(&base, &item_id) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not_found"}))).into_response();
    };
    let Some(p) = find_cover_image_for_character(&character_dir) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "no_cover"}))).into_response();
    };
    match tokio::fs::read(&p).await {
        Ok(bytes) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(CONTENT_TYPE, axum::http::HeaderValue::from_static(content_type_for_path(&p)));
            headers.insert(CACHE_CONTROL, axum::http::HeaderValue::from_static("no-cache"));
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not_found"}))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct OwnedSkinCoverPath {
    item_id: String,
    skin_name: String,
}

async fn get_owned_skin_cover(
    axum::extract::Path(p): axum::extract::Path<OwnedSkinCoverPath>,
) -> axum::response::Response {
    let base = assets_dir();
    let Some(character_dir) = safe_child_dir(&base, &p.item_id) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not_found"}))).into_response();
    };
    let skins_dir = character_dir.join("skins");
    let Some(skin_dir) = safe_child_dir(&skins_dir, &p.skin_name) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not_found"}))).into_response();
    };
    let Some(img) = find_cover_image_in_dir(&skin_dir) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "no_cover"}))).into_response();
    };
    match tokio::fs::read(&img).await {
        Ok(bytes) => {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(CONTENT_TYPE, axum::http::HeaderValue::from_static(content_type_for_path(&img)));
            headers.insert(CACHE_CONTROL, axum::http::HeaderValue::from_static("no-cache"));
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not_found"}))).into_response(),
    }
}

async fn get_store_items(
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let base = q.get("base").cloned().unwrap_or_default();
    if base.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"缺少 base"})));
    }
    let url = match store_url(&base, &["api", "store", "items"]) {
        Ok(u) => u,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))),
    };

    let client = reqwest::Client::new();
    let mut req = client.get(url);
    if let Some(v) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(s) = v.to_str() {
            req = req.header(reqwest::header::AUTHORIZATION, s);
        }
    }
    match req.send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => (StatusCode::OK, Json(v)),
            Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()}))),
        },
        Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()}))),
    }
}

async fn post_store_auth_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StoreAuthLoginReq>,
) -> impl IntoResponse {
    let store_base_url = req.store_base_url.trim();
    if store_base_url.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"store_base_url 为空"})));
    }
    let user_id = req.user_id.trim();
    if user_id.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"user_id 为空"})));
    }

    let url = match store_url(store_base_url, &["api", "auth", "login"]) {
        Ok(u) => u,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))),
    };

    let client = reqwest::Client::new();
    match client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&serde_json::json!({"userId": user_id}))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                let path = StoreUser::default_path();
                if let Ok(mut g) = state.store_user.lock() {
                    g.user_id = user_id.to_string();
                    let _ = g.save(&path);
                }
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => (status, Json(v)),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({"error": e.to_string()})),
                ),
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

async fn get_store_user(State(state): State<Arc<AppState>>) -> Json<StoreUser> {
    let u = state.store_user.lock().unwrap().clone();
    Json(u)
}

async fn post_store_user(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StoreUserUpdate>,
) -> Json<StoreUser> {
    let path = StoreUser::default_path();
    let mut g = state.store_user.lock().unwrap();
    if let Some(v) = req.user_id {
        g.user_id = v;
    }
    if let Some(v) = req.display_name {
        let t = v.trim().to_string();
        g.display_name = if t.is_empty() { None } else { Some(t) };
    }
    if let Err(e) = g.save(&path) {
        tracing::error!("保存 store_user 失败: {}", e);
    }
    Json(g.clone())
}

async fn post_store_install_start(
    State(_state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<InstallStartReq>,
) -> (StatusCode, Json<InstallStartResp>) {
    let job_id = format!(
        "inst_{}_{}",
        now_ms(),
        INSTALL_JOB_COUNTER.fetch_add(1, Ordering::Relaxed)
    );

    {
        if let Ok(mut g) = jobs().lock() {
            g.insert(
                job_id.clone(),
                InstallJobStatus {
                    job_id: job_id.clone(),
                    stage: "downloading".to_string(),
                    downloaded_bytes: 0,
                    total_bytes: None,
                    done: false,
                    error: None,
                    installed_character_id: None,
                },
            );
        }
    }

    let store_base_url = req.store_base_url.trim().to_string();
    let item_id = req.item_id.trim().to_string();
    let skin_name = req.skin_name.clone();
    let job_id2 = job_id.clone();
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    tokio::spawn(async move {
        let url = match skin_name.as_deref() {
            Some(skin) if !skin.trim().is_empty() => {
                match store_url(&store_base_url, &["api", "store", "packages", &item_id, skin]) {
                    Ok(u) => u,
                    Err(e) => {
                        if let Ok(mut g) = jobs().lock() {
                            if let Some(s) = g.get_mut(&job_id2) {
                                s.stage = "error".to_string();
                                s.done = true;
                                s.error = Some(e);
                            }
                        }
                        return;
                    }
                }
            }
            _ => match store_url(&store_base_url, &["api", "store", "packages", &item_id]) {
                Ok(u) => u,
                Err(e) => {
                    if let Ok(mut g) = jobs().lock() {
                        if let Some(s) = g.get_mut(&job_id2) {
                            s.stage = "error".to_string();
                            s.done = true;
                            s.error = Some(e);
                        }
                    }
                    return;
                }
            },
        };

        let dl_dir = if let Ok(exe) = std::env::current_exe() {
            exe.parent().unwrap_or(Path::new(".")).join("data").join("store_downloads")
        } else {
            PathBuf::from("data").join("store_downloads")
        };
        let _ = tokio::fs::create_dir_all(&dl_dir).await;
        let zip_path = dl_dir.join(format!("{job_id2}.zip"));

        let client = reqwest::Client::new();
        let mut req = client.get(url);
        if let Some(v) = auth_header.as_deref() {
            req = req.header(reqwest::header::AUTHORIZATION, v);
        }
        let mut resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                if let Ok(mut g) = jobs().lock() {
                    if let Some(s) = g.get_mut(&job_id2) {
                        s.stage = "error".to_string();
                        s.done = true;
                        s.error = Some(format!("下载失败: {}", e));
                    }
                }
                return;
            }
        };

        if !resp.status().is_success() {
            if let Ok(mut g) = jobs().lock() {
                if let Some(s) = g.get_mut(&job_id2) {
                    s.stage = "error".to_string();
                    s.done = true;
                    s.error = Some(format!("下载失败: HTTP {}", resp.status()));
                }
            }
            return;
        }

        let total: Option<u64> = resp.content_length();
        if let Ok(mut g) = jobs().lock() {
            if let Some(s) = g.get_mut(&job_id2) {
                s.total_bytes = total;
            }
        }

        let mut file = match tokio::fs::File::create(&zip_path).await {
            Ok(f) => f,
            Err(e) => {
                if let Ok(mut g) = jobs().lock() {
                    if let Some(s) = g.get_mut(&job_id2) {
                        s.stage = "error".to_string();
                        s.done = true;
                        s.error = Some(format!("创建文件失败: {}", e));
                    }
                }
                return;
            }
        };

        let mut downloaded: u64 = 0;
        loop {
            let chunk = match resp.chunk().await {
                Ok(c) => c,
                Err(e) => {
                    if let Ok(mut g) = jobs().lock() {
                        if let Some(s) = g.get_mut(&job_id2) {
                            s.stage = "error".to_string();
                            s.done = true;
                            s.error = Some(format!("读取数据失败: {}", e));
                        }
                    }
                    return;
                }
            };
            let Some(bytes) = chunk else { break };

            if let Err(e) = file.write_all(&bytes).await {
                if let Ok(mut g) = jobs().lock() {
                    if let Some(s) = g.get_mut(&job_id2) {
                        s.stage = "error".to_string();
                        s.done = true;
                        s.error = Some(format!("写入文件失败: {}", e));
                    }
                }
                return;
            }

            downloaded = downloaded.saturating_add(bytes.len() as u64);
            if let Ok(mut g) = jobs().lock() {
                if let Some(s) = g.get_mut(&job_id2) {
                    s.downloaded_bytes = downloaded;
                }
            }
        }

        if let Ok(mut g) = jobs().lock() {
            if let Some(s) = g.get_mut(&job_id2) {
                s.stage = "extracting".to_string();
            }
        }

        let target_assets = assets_dir();
        let _ = tokio::fs::create_dir_all(&target_assets).await;
        let zip_path2 = zip_path.clone();
        let extract_result = tokio::task::spawn_blocking(move || unzip_to_dir(&zip_path2, &target_assets))
            .await
            .unwrap_or_else(|e| Err(format!("解压任务失败: {}", e)));

        match extract_result {
            Ok(_) => {
                if let Ok(mut g) = jobs().lock() {
                    if let Some(s) = g.get_mut(&job_id2) {
                        s.stage = "done".to_string();
                        s.done = true;
                        s.installed_character_id = Some(item_id);
                    }
                }
            }
            Err(e) => {
                if let Ok(mut g) = jobs().lock() {
                    if let Some(s) = g.get_mut(&job_id2) {
                        s.stage = "error".to_string();
                        s.done = true;
                        s.error = Some(e);
                    }
                }
            }
        }
    });

    (StatusCode::OK, Json(InstallStartResp { job_id }))
}

#[derive(serde::Deserialize)]
struct InstallStatusQuery {
    job_id: String,
}

async fn get_store_install_status(Query(q): Query<InstallStatusQuery>) -> (StatusCode, Json<InstallStatusResp>) {
    let status = jobs()
        .lock()
        .ok()
        .and_then(|g| g.get(&q.job_id).cloned())
        .unwrap_or(InstallJobStatus {
            job_id: q.job_id.clone(),
            stage: "error".to_string(),
            downloaded_bytes: 0,
            total_bytes: None,
            done: true,
            error: Some("job_id 不存在".to_string()),
            installed_character_id: None,
        });

    let percent = status.total_bytes.and_then(|t| {
        if t == 0 {
            None
        } else {
            Some((status.downloaded_bytes as f64 / t as f64) * 100.0)
        }
    });

    (
        StatusCode::OK,
        Json(InstallStatusResp {
            job_id: status.job_id,
            stage: status.stage,
            downloaded_bytes: status.downloaded_bytes,
            total_bytes: status.total_bytes,
            percent,
            done: status.done,
            error: status.error,
            installed_character_id: status.installed_character_id,
        }),
    )
}

async fn index(Query(q): Query<HashMap<String, String>>) -> Html<String> {
    let page = q
        .get("page")
        .map(|s| s.as_str())
        .filter(|p| matches!(*p, "api" | "persona" | "characters" | "diary" | "monitor"))
        .unwrap_or("api");

    let html = r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
  <title>AI 桌宠 - 管理中心</title>
    <style>
      :root {
      --bg: #0b0b10;
      --surface: #14141c;
      --surface-hover: #1c1c27;
      --surface-2: #101018;
      --border: rgba(255, 255, 255, 0.08);
      --border-strong: rgba(255, 255, 255, 0.14);
      --primary: #845ef7;
      --primary-hover: #9771f8;
      --accent: #4cc5ff;
      --text: #e8e8ed;
      --text-dim: rgba(232, 232, 237, 0.62);
      --success: #40c057;
      --danger: #ff6b6b;
      --warning: #ffd43b;
      --shadow: 0 18px 55px rgba(0, 0, 0, 0.55);
      --shadow-soft: 0 12px 30px rgba(0, 0, 0, 0.36);
      --radius: 14px;
      --radius-sm: 10px;
    }
    * { margin: 0; padding: 0; box-sizing: border-box; }
      body {
      font-family: "Segoe UI", system-ui, sans-serif;
      background: radial-gradient(900px 520px at 18% -10%, rgba(132, 94, 247, 0.22), rgba(0,0,0,0) 62%),
                  radial-gradient(720px 420px at 88% 0%, rgba(76, 197, 255, 0.18), rgba(0,0,0,0) 60%),
                  radial-gradient(820px 540px at 50% 110%, rgba(64, 192, 87, 0.12), rgba(0,0,0,0) 58%),
                  var(--bg);
      color: var(--text);
      min-height: 100vh;
      overflow: hidden;
    }
    .layout { display: flex; min-height: 100vh; }
    
    /* 侧边栏 */
    .sidebar {
        width: 220px;
      background: var(--surface);
      border-right: 1px solid var(--border);
      padding: 20px 0;
      flex-shrink: 0;
    }
    .logo {
      padding: 0 20px 20px;
      border-bottom: 1px solid var(--border);
      margin-bottom: 20px;
    }
    .logo h1 { font-size: 18px; font-weight: 600; color: var(--text); }
    .logo p { font-size: 12px; color: var(--text-dim); margin-top: 4px; }
    .nav-item {
        display: flex;
        align-items: center;
      gap: 12px;
      padding: 12px 20px;
      color: var(--text-dim);
        cursor: pointer;
      transition: all 0.15s;
      border-left: 3px solid transparent;
      text-decoration: none;
    }
    .nav-item:visited { color: var(--text-dim); }
    .nav-item:hover { background: var(--surface-hover); color: var(--text); }
    .nav-item.active { 
      background: rgba(132, 94, 247, 0.1); 
      color: var(--primary);
      border-left-color: var(--primary);
    }
    .nav-icon { font-size: 18px; width: 24px; text-align: center; }
    .nav-label { font-size: 14px; }
    
    /* 主内容区 */
    .main { flex: 1; padding: 22px 26px; overflow-y: auto; overflow-x: hidden; }
    .content { padding-top: 14px; }
      .page { display: none; }
    .page.active { display: block; }
    .page-header { margin-bottom: 24px; }
    .page-title { font-size: 24px; font-weight: 600; margin-bottom: 8px; }
    .page-desc { color: var(--text-dim); font-size: 14px; }

    .topbar {
      position: sticky;
      top: 0;
      z-index: 20;
      padding: 14px 16px;
      border-radius: var(--radius);
      border: 1px solid var(--border);
      background: linear-gradient(180deg, rgba(20, 20, 28, 0.78), rgba(20, 20, 28, 0.58));
      backdrop-filter: blur(12px);
      box-shadow: var(--shadow-soft);
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 14px;
      flex-wrap: wrap;
    }
    .topbar-title { font-size: 14px; font-weight: 700; letter-spacing: 0.2px; }
    .topbar-sub { margin-top: 4px; font-size: 12px; color: var(--text-dim); }
    .topbar-left { min-width: 180px; }
    .topbar-right { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; justify-content: flex-end; }
    .pill {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 7px 10px;
      border-radius: 999px;
      border: 1px solid var(--border);
      background: rgba(0, 0, 0, 0.22);
      font-size: 12px;
      color: var(--text);
      max-width: 520px;
    }
    .pill .dot { width: 7px; height: 7px; border-radius: 999px; background: var(--text-dim); flex-shrink: 0; }
    .pill strong { font-weight: 700; }
    .pill.ok .dot { background: var(--success); }
    .pill.warn .dot { background: var(--warning); }
    .pill.bad .dot { background: var(--danger); }
    .pill .mono { font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; font-size: 11px; color: rgba(232, 232, 237, 0.76); }
    
    /* 卡片 */
      .card {
      background: var(--surface);
      border: 1px solid var(--border);
      border-radius: var(--radius);
      padding: 20px;
      margin-bottom: 16px;
      box-shadow: 0 1px 0 rgba(255,255,255,0.04) inset;
    }
    .card-title { font-size: 14px; font-weight: 600; margin-bottom: 16px; color: var(--text); }
    
    /* 表单 */
    .form-group { margin-bottom: 16px; }
    .form-label { 
      display: block; 
      font-size: 13px; 
      color: var(--text-dim); 
      margin-bottom: 6px; 
    }
    .form-input {
        width: 100%;
      padding: 10px 14px;
      background: rgba(0, 0, 0, 0.25);
      border: 1px solid var(--border);
      border-radius: 8px;
        color: var(--text);
      font-size: 14px;
        outline: none;
      transition: border-color 0.15s;
    }
    .form-input:focus { border-color: var(--primary); }
    .form-input::placeholder { color: var(--text-dim); }
    textarea.form-input { min-height: 100px; resize: vertical; }
    
    /* 按钮 */
    .btn {
      padding: 10px 20px;
      border-radius: 8px;
      font-size: 14px;
      font-weight: 500;
      cursor: pointer;
      border: none;
      transition: all 0.15s;
    }
    .btn-primary { 
      background: var(--primary); 
      color: white; 
    }
    .btn-primary:hover { background: var(--primary-hover); }
    .btn-secondary { 
      background: var(--surface-hover); 
        color: var(--text);
      border: 1px solid var(--border);
    }
    .btn-secondary:hover { border-color: var(--text-dim); }
    .btn-danger { 
      background: rgba(255, 107, 107, 0.15); 
      color: var(--danger);
      border: 1px solid rgba(255, 107, 107, 0.3);
    }
    .btn-danger:hover { background: rgba(255, 107, 107, 0.25); }
    
    .btn-row { display: flex; gap: 10px; margin-top: 20px; }
    .controls-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
    .controls-row .left { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
    .controls-row .right { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }

    .progress { height: 10px; background: var(--bg); border: 1px solid var(--border); border-radius: 999px; overflow: hidden; margin-top: 10px; position: relative; }
    .progress-fill { height: 100%; width: 0%; background: linear-gradient(90deg, var(--primary), var(--accent)); transition: width 0.15s ease; }
    .progress.indeterminate .progress-fill { width: 28%; position: absolute; left: 0; top: 0; animation: indet 1.1s linear infinite; transition: none; }
    @keyframes indet { 0% { transform: translateX(-120%); } 100% { transform: translateX(420%); } }
    .download-meta { display: flex; justify-content: space-between; align-items: center; gap: 10px; }
    .download-meta-main { font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .download-meta-percent { font-size: 12px; color: var(--text-dim); flex-shrink: 0; }
    .download-sub { margin-top: 6px; font-size: 12px; color: var(--text-dim); }
    .store-item { padding: 12px; background: rgba(0, 0, 0, 0.18); border: 1px solid var(--border); border-radius: 12px; margin-top: 10px; }
    .store-item-top { display: flex; justify-content: space-between; gap: 10px; align-items: center; }
    .store-item-name { font-size: 14px; font-weight: 600; color: var(--text); }
    .store-item-desc { margin-top: 6px; font-size: 12px; color: var(--text-dim); }
    .store-skins { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 10px; }
    .btn-sm { padding: 8px 12px; border-radius: 8px; font-size: 12px; }

    .owned-header { display: flex; justify-content: space-between; align-items: center; gap: 10px; margin-bottom: 12px; }
    .owned-title { font-size: 14px; font-weight: 600; color: var(--text); }
    .owned-sub { font-size: 12px; color: var(--text-dim); }
    .owned-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 18px; }
    @media (max-width: 980px) { .owned-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
    @media (max-width: 720px) { .owned-grid { grid-template-columns: repeat(1, minmax(0, 1fr)); } }
    .owned-card { background: rgba(255,255,255,0.03); border: 1px solid var(--border); border-radius: 16px; overflow: hidden; }
    .owned-cover { position: relative; height: 260px; background: var(--surface-hover); display: flex; align-items: center; justify-content: center; }
    .owned-cover img { width: 100%; height: 100%; object-fit: contain; object-position: 50% 50%; display: block; }
    .owned-badge { position: absolute; left: 12px; top: 12px; padding: 6px 10px; border-radius: 999px; background: rgba(255,255,255,0.08); border: 1px solid rgba(255,255,255,0.12); font-size: 12px; color: var(--text); backdrop-filter: blur(8px); }
    .owned-body { padding: 14px 16px 16px 16px; }
    .owned-name { font-size: 16px; font-weight: 700; color: var(--text); }
    .owned-desc { margin-top: 6px; font-size: 13px; color: var(--text-dim); min-height: 18px; }
    .owned-actions { margin-top: 12px; display: flex; gap: 10px; }
    .owned-actions .btn { flex: 1; }
    .owned-back { display: inline-flex; align-items: center; gap: 8px; cursor: pointer; color: var(--text-dim); font-size: 13px; }
    .owned-back:hover { color: var(--text); }

    .store-user-banner {
      margin-bottom: 18px;
      padding: 14px 18px;
      background: linear-gradient(135deg, rgba(132, 94, 247, 0.12), rgba(76, 197, 255, 0.08));
      border: 1px solid var(--border);
      border-radius: 12px;
    }
    .store-user-banner-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
    .store-user-banner-label { font-size: 12px; color: var(--text-dim); }
    .store-user-pill {
      display: inline-flex;
      align-items: center;
      padding: 6px 14px;
      border-radius: 999px;
      border: 1px solid rgba(132, 94, 247, 0.55);
      background: rgba(132, 94, 247, 0.14);
      color: var(--text);
      font-size: 13px;
      font-weight: 600;
    }
    .store-user-sub { margin-top: 6px; font-size: 12px; color: var(--text-dim); word-break: break-all; }
    
    /* 状态指示 */
    .status {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 10px;
      border-radius: 20px;
      font-size: 12px;
    }
    .status-dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
    }
    .status-on { background: rgba(64, 192, 87, 0.2); color: var(--success); }
    .status-on .status-dot { background: var(--success); }
    .status-off { background: rgba(255, 107, 107, 0.2); color: var(--danger); }
    .status-off .status-dot { background: var(--danger); }
    
    /* 列表 */
    .diary-list { max-height: 300px; overflow-y: auto; }
    .diary-item {
      padding: 12px;
      background: var(--bg);
      border-radius: 8px;
      margin-bottom: 8px;
    }
    .diary-time { font-size: 11px; color: var(--text-dim); margin-bottom: 4px; }
    .diary-text { font-size: 13px; line-height: 1.5; }
    
    /* 测试结果 */
    .test-result {
      margin-top: 16px;
      padding: 12px;
      background: var(--bg);
      border-radius: 8px;
      font-size: 13px;
      display: none;
    }
    .test-result.show { display: block; }
    .test-result.success { border-left: 3px solid var(--success); }
    .test-result.error { border-left: 3px solid var(--danger); }
    
    /* 监控数据 */
    .monitor-grid {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 12px;
    }
    @media (max-width: 980px) { .monitor-grid { grid-template-columns: repeat(2, 1fr); } }
    @media (max-width: 620px) { .monitor-grid { grid-template-columns: repeat(1, 1fr); } }
    .monitor-item {
      background: rgba(0, 0, 0, 0.18);
      padding: 12px;
      border-radius: 8px;
      border: 1px solid var(--border);
    }
    .monitor-label { font-size: 11px; color: var(--text-dim); }
    .monitor-value { font-size: 20px; font-weight: 600; margin-top: 4px; }
    .monitor-sub { margin-top: 8px; font-size: 12px; color: var(--text-dim); line-height: 1.4; word-break: break-word; }

    .table-wrap { width: 100%; overflow: auto; border: 1px solid var(--border); border-radius: var(--radius); background: rgba(0, 0, 0, 0.16); }
    table.table { width: 100%; border-collapse: collapse; min-width: 720px; }
    .table th, .table td { padding: 10px 12px; text-align: left; border-bottom: 1px solid rgba(255,255,255,0.06); font-size: 13px; }
    .table th { position: sticky; top: 0; background: rgba(20, 20, 28, 0.88); backdrop-filter: blur(8px); font-size: 12px; letter-spacing: 0.2px; color: rgba(232,232,237,0.78); }
    .table td.mono { font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; font-size: 12px; color: rgba(232,232,237,0.86); }
    .level { display: inline-flex; align-items: center; gap: 8px; }
    .level .dot { width: 8px; height: 8px; border-radius: 999px; background: rgba(232,232,237,0.28); }
    .level.info .dot { background: rgba(76, 197, 255, 0.9); }
    .level.warn .dot { background: rgba(255, 212, 59, 0.9); }
    .level.error .dot { background: rgba(255, 107, 107, 0.9); }

    .log-list { max-height: 340px; overflow: auto; border: 1px solid var(--border); border-radius: var(--radius); background: rgba(0, 0, 0, 0.16); }
    .log-item { padding: 10px 12px; border-bottom: 1px solid rgba(255,255,255,0.06); display: grid; grid-template-columns: 140px 92px 1fr; gap: 10px; align-items: start; }
    .log-item:last-child { border-bottom: none; }
    .log-time { font-size: 12px; color: rgba(232,232,237,0.72); }
    .log-msg { font-size: 13px; line-height: 1.45; word-break: break-word; }
    .log-empty { padding: 14px 12px; font-size: 13px; color: var(--text-dim); }

    .toast-host { position: fixed; right: 18px; bottom: 18px; z-index: 1000; display: flex; flex-direction: column; gap: 10px; pointer-events: none; }
    .toast { pointer-events: auto; min-width: 260px; max-width: 420px; padding: 12px 12px; border-radius: 12px; border: 1px solid var(--border); background: rgba(20,20,28,0.92); backdrop-filter: blur(10px); box-shadow: var(--shadow-soft); display: flex; gap: 10px; align-items: flex-start; }
    .toast .dot { width: 10px; height: 10px; border-radius: 999px; background: rgba(232,232,237,0.35); margin-top: 4px; flex-shrink: 0; }
    .toast.success .dot { background: var(--success); }
    .toast.error .dot { background: var(--danger); }
    .toast.info .dot { background: var(--accent); }
    .toast.warn .dot { background: var(--warning); }
    .toast .text { font-size: 13px; line-height: 1.45; color: rgba(232,232,237,0.9); }
    @media (prefers-reduced-motion: reduce) { .progress-fill { transition: none; } }
    
    /* 宠物个性预览 */
    .persona-preview {
      background: linear-gradient(135deg, rgba(132, 94, 247, 0.1), rgba(76, 197, 255, 0.1));
      border: 1px solid var(--border);
        border-radius: 12px;
      padding: 20px;
      margin-bottom: 20px;
    }
    .persona-avatar {
      width: 60px;
      height: 60px;
      border-radius: 50%;
      background: linear-gradient(135deg, var(--primary), var(--accent));
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 28px;
      margin-bottom: 12px;
    }
    .persona-name { font-size: 16px; font-weight: 600; }
    .persona-trait { font-size: 12px; color: var(--accent); margin-top: 4px; }
    </style>
  </head>
  <body>
    <div class="layout">
    <!-- 侧边栏 -->
    <div class="sidebar">
      <div class="logo">
        <h1>🐾 桌宠管理中心</h1>
        <p>v0.1.0</p>
      </div>
      <a class="nav-item active" data-page="api" href="/?page=api">
        <span class="nav-icon">🔗</span>
        <span class="nav-label">API 绑定</span>
      </a>
      <a class="nav-item" data-page="persona" href="/?page=persona">
        <span class="nav-icon">🎭</span>
        <span class="nav-label">宠物个性</span>
      </a>
      <a class="nav-item" data-page="characters" href="/?page=characters">
        <span class="nav-icon">🧩</span>
        <span class="nav-label">角色库</span>
      </a>
      <a class="nav-item" data-page="diary" href="/?page=diary">
        <span class="nav-icon">📔</span>
        <span class="nav-label">宠物日记</span>
      </a>
      <a class="nav-item" data-page="monitor" href="/?page=monitor">
        <span class="nav-icon">📊</span>
        <span class="nav-label">系统监控</span>
      </a>
    </div>
    
    <!-- 主内容 -->
    <div class="main">
      <div class="topbar">
        <div class="topbar-left">
          <div class="topbar-title" id="topbar-title">管理中心</div>
          <div class="topbar-sub" id="topbar-sub">后端状态与快捷信息</div>
        </div>
        <div class="topbar-right">
          <span class="pill" id="pill-backend"><span class="dot"></span><strong>Backend</strong><span class="mono" id="pill-backend-text">—</span></span>
          <span class="pill" id="pill-api"><span class="dot"></span><strong>AI</strong><span class="mono" id="pill-api-text">未配置</span></span>
          <span class="pill" id="pill-persona"><span class="dot"></span><strong>人设</strong><span class="mono" id="pill-persona-text">—</span></span>
          <span class="pill" id="pill-diary"><span class="dot"></span><strong>日记</strong><span class="mono" id="pill-diary-text">—</span></span>
        </div>
      </div>
      <div class="content">
      <!-- API 绑定页面 -->
          <div class="page active" id="page-api">
        <div class="page-header">
          <h2 class="page-title">🔗 API 绑定设置</h2>
          <p class="page-desc">配置 AI 服务商的 API 连接信息</p>
        </div>
        
            <div class="card">
          <div class="card-title">连接状态</div>
          <div id="api-status">
            <span class="status status-off"><span class="status-dot"></span>未配置</span>
          </div>
            </div>

            <div class="card">
          <div class="card-title">API 配置</div>
          <div class="form-group">
            <label class="form-label">Base URL</label>
            <input type="text" class="form-input" id="api-base-url" placeholder="https://api.deepseek.com/v1">
                </div>
          <div class="form-group">
            <label class="form-label">模型名称</label>
            <input type="text" class="form-input" id="api-model" placeholder="deepseek-chat">
                </div>
          <div class="form-group">
            <label class="form-label">API Key</label>
            <input type="password" class="form-input" id="api-key" placeholder="sk-xxxxxxxx">
                </div>
          <div class="btn-row">
            <button class="btn btn-primary" onclick="saveApiConfig()">保存配置</button>
            <button class="btn btn-secondary" onclick="testApi()">测试连接</button>
                </div>
          <div class="test-result" id="test-result"></div>
              </div>
              </div>
      
      <!-- 宠物个性页面 -->
      <div class="page" id="page-persona">
        <div class="page-header">
          <h2 class="page-title">🎭 宠物个性设置</h2>
          <p class="page-desc">定义宠物的性格、语气和行为模式</p>
        </div>
        
        <div class="persona-preview">
          <div class="persona-avatar">🐱</div>
          <div class="persona-name" id="persona-name-preview">可爱的小猫</div>
          <div class="persona-trait" id="persona-trait-preview">活泼 · 好奇 · 黏人</div>
            </div>

            <div class="card">
          <div class="card-title">基本设置</div>
          <div class="form-group">
            <label class="form-label">宠物名称</label>
            <input type="text" class="form-input" id="persona-name" placeholder="可爱的小猫" oninput="updatePreview()">
              </div>
          <div class="form-group">
            <label class="form-label">性格标签（用 · 分隔）</label>
            <input type="text" class="form-input" id="persona-traits" placeholder="活泼 · 好奇 · 黏人" oninput="updatePreview()">
          </div>
            </div>

            <div class="card">
          <div class="card-title">AI 角色设定</div>
          <div class="form-group">
            <label class="form-label">系统提示词</label>
            <textarea class="form-input" id="system-prompt" rows="4" placeholder="你是主人的贴心小宠物，活泼可爱，喜欢撒娇..."></textarea>
                </div>
              </div>
        
        <div class="btn-row">
          <button class="btn btn-primary" onclick="savePersona()">保存个性</button>
          <button class="btn btn-secondary" onclick="resetPersona()">重置默认</button>
          <button class="btn btn-secondary" onclick="refreshCurrentPersona(true)">刷新角色</button>
        </div>
      </div>

      <div class="page" id="page-characters">
        <div class="page-header">
          <h2 class="page-title">🧩 角色库</h2>
          <p class="page-desc">管理与安装角色模组（支持热切换）</p>
        </div>

        <div class="store-user-banner">
          <div class="store-user-banner-row">
            <div>
              <div class="store-user-banner-label">当前商城用户（桌宠菜单紫色角标同步此数据）</div>
              <div style="margin-top:8px;display:flex;align-items:center;gap:10px;flex-wrap:wrap;">
                <span class="store-user-pill" id="store-user-display">—</span>
                <span class="store-user-pill" id="store-coins-display">🪙 —</span>
              </div>
              <div class="store-user-sub" id="store-user-id-sub"></div>
            </div>
            <button type="button" class="btn btn-secondary btn-sm" onclick="refreshStoreUserBar(true)">刷新</button>
          </div>
        </div>

        <div class="card">
          <div class="owned-header">
            <div>
              <div class="owned-title">已拥有的角色</div>
              <div class="owned-sub">从本地 frontend/assets 扫描</div>
            </div>
            <button class="btn btn-secondary btn-sm" onclick="loadOwned()">刷新</button>
          </div>
          <div id="owned-characters">
            <div class="owned-grid" id="owned-character-grid"></div>
          </div>
          <div id="owned-skins" style="display:none;">
            <div class="owned-header" style="margin-bottom: 12px;">
              <div class="owned-back" onclick="backToOwned()">← 返回角色</div>
              <div style="flex: 1;"></div>
              <div class="owned-sub" id="owned-skins-title"></div>
            </div>
            <div class="owned-grid" id="owned-skin-grid"></div>
          </div>
        </div>

        <div class="card">
          <div class="card-title">从角色商城安装（下载 → 解压到 frontend/assets）</div>
          <div class="form-group">
            <label class="form-label">商城地址</label>
            <input class="form-input" id="store-base-url" placeholder="https://your-store.example.com">
          </div>
          <div class="btn-row" style="margin-top: 0;">
            <button class="btn btn-primary" onclick="loadStore()">加载商品</button>
            <button class="btn btn-secondary" onclick="openRoleStore()">打开商城</button>
          </div>
          <div id="store-list" style="margin-top: 12px;"></div>
          <div id="store-install-box" style="display:none; margin-top: 14px;">
            <div class="download-meta">
              <div class="download-meta-main" id="store-install-text">准备安装</div>
              <div class="download-meta-percent" id="store-install-percent">0%</div>
            </div>
            <div class="progress" id="store-install-progress">
              <div class="progress-fill" id="store-install-fill"></div>
            </div>
            <div class="download-sub" id="store-install-sub"></div>
          </div>
        </div>
      </div>
      
      <!-- 宠物日记页面 -->
      <div class="page" id="page-diary">
        <div class="page-header">
          <h2 class="page-title">📔 宠物日记</h2>
          <p class="page-desc">记录你和宠物的日常互动，AI 会自动总结</p>
        </div>
        
        <!-- 现实世界概览卡片 -->
        <div class="card" id="realworld-card">
          <div class="card-title">🌍 现实世界概览</div>
          <div style="display: flex; gap: 20px; align-items: center; flex-wrap: wrap;">
            <div style="flex: 1; min-width: 150px;">
              <div id="realworld-time" style="font-size: 36px; font-weight: 700; color: var(--primary);">--:--</div>
              <div id="realworld-date" style="font-size: 14px; color: var(--text-dim); margin-top: 4px;">--年--月--日 · 星期-</div>
            </div>
            <div id="realworld-weather-box" style="padding: 12px 20px; background: rgba(132, 94, 247, 0.1); border: 1px solid var(--border); border-radius: 12px; min-width: 120px; text-align: center;">
              <div id="realworld-weather-icon" style="font-size: 24px;">🌤️</div>
              <div id="realworld-weather-text" style="font-size: 14px; font-weight: 600; margin-top: 4px;">获取中...</div>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-title">记录新日记</div>
          <div class="form-group">
            <textarea class="form-input" id="diary-input" rows="3" placeholder="今天主人和我一起看了电影..."></textarea>
              </div>
          <div class="form-group">
            <label class="form-label">写作角色（可选，来自本地角色 character.json）</label>
            <select class="form-input" id="diary-persona-character">
              <option value="">使用当前系统提示词</option>
            </select>
          </div>
          <div class="btn-row">
            <button class="btn btn-primary" onclick="addDiary()">添加记录</button>
            <button class="btn btn-secondary" id="diary-generate-btn" onclick="generateDiaryFromProcesses()">观察后台写日记</button>
            <button class="btn btn-secondary" onclick="summarizeDiary()">AI 总结</button>
            </div>
          </div>

            <div class="card">
          <div class="card-title">日记历史</div>
          <div class="btn-row" style="margin-top: 0; margin-bottom: 16px;">
            <button class="btn btn-secondary" onclick="loadDiary()">刷新</button>
            <button class="btn btn-danger" onclick="clearDiary()">清空</button>
            </div>
          <div class="diary-list" id="diary-list">
            <p style="color: var(--text-dim); font-size: 13px;">暂无日记记录</p>
              </div>
              </div>
        
        <div class="card" id="summary-card" style="display: none;">
          <div class="card-title">📝 AI 总结</div>
          <div id="diary-summary"></div>
            </div>
          </div>

      <!-- 系统监控页面 -->
      <div class="page" id="page-monitor">
        <div class="page-header">
          <h2 class="page-title">📊 系统监控</h2>
          <p class="page-desc">实时查看系统资源使用情况</p>
            </div>
        
        <div class="card">
          <div class="controls-row">
            <div class="left">
              <div class="card-title" style="margin-bottom: 0;">资源概览</div>
              <span class="status status-on" id="monitor-live" style="display:none;"><span class="status-dot"></span>在线</span>
            </div>
            <div class="right">
              <button class="btn btn-secondary btn-sm" onclick="refreshMonitor(true)">刷新</button>
            </div>
          </div>
          <div class="monitor-grid" style="margin-top: 14px;">
            <div class="monitor-item">
              <div class="monitor-label">CPU 使用率</div>
              <div class="monitor-value" id="cpu-usage">--%</div>
              <div class="progress" style="margin-top: 10px;">
                <div class="progress-fill" id="cpu-bar"></div>
              </div>
            </div>
            <div class="monitor-item">
              <div class="monitor-label">内存使用</div>
              <div class="monitor-value" id="mem-usage">--</div>
              <div class="progress" style="margin-top: 10px;">
                <div class="progress-fill" id="mem-bar"></div>
              </div>
              <div class="monitor-sub" id="mem-sub">--</div>
            </div>
            <div class="monitor-item">
              <div class="monitor-label">前台窗口</div>
              <div class="monitor-value" style="font-size: 16px; font-weight: 700;" id="focus-window">—</div>
              <div class="monitor-sub" id="focus-sub">—</div>
            </div>
            <div class="monitor-item">
              <div class="monitor-label">进程数</div>
              <div class="monitor-value" id="proc-count">--</div>
              <div class="monitor-sub">系统进程总数</div>
            </div>
            <div class="monitor-item">
              <div class="monitor-label">后台自身内存</div>
              <div class="monitor-value" id="self-mem-usage">--</div>
              <div class="monitor-sub">gateway-api 进程占用</div>
            </div>
            <div class="monitor-item">
              <div class="monitor-label">自动日记</div>
              <div class="monitor-value" style="font-size: 16px; font-weight: 700;" id="auto-diary-status">—</div>
              <div class="monitor-sub" id="auto-diary-sub">默认每 10 分钟写一次</div>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="controls-row">
            <div class="left">
              <div class="card-title" style="margin-bottom: 0;">后台进程（Top）</div>
              <input class="form-input" style="width: 260px; padding: 8px 12px;" id="proc-filter" placeholder="过滤进程名，例如 chrome" oninput="renderProcesses()">
            </div>
            <div class="right">
              <button class="btn btn-secondary btn-sm" onclick="refreshProcesses()">刷新</button>
            </div>
          </div>
          <div class="table-wrap" style="margin-top: 14px;">
            <table class="table">
              <thead>
                <tr>
                  <th style="width: 54%;">进程</th>
                  <th style="width: 14%;">PID</th>
                  <th style="width: 16%;">CPU</th>
                  <th style="width: 16%;">内存</th>
                </tr>
              </thead>
              <tbody id="proc-table-body">
                <tr><td colspan="4" style="padding: 14px 12px; color: var(--text-dim);">点击“刷新”加载进程列表</td></tr>
              </tbody>
            </table>
          </div>
        </div>

        <div class="card">
          <div class="controls-row">
            <div class="left">
              <div class="card-title" style="margin-bottom: 0;">后台日志</div>
              <input class="form-input" style="width: 260px; padding: 8px 12px;" id="log-filter" placeholder="过滤关键字，例如 AI 错误" oninput="renderLogs()">
            </div>
            <div class="right">
              <button class="btn btn-secondary btn-sm" onclick="refreshLogs()">刷新</button>
            </div>
          </div>
          <div class="log-list" style="margin-top: 14px;" id="log-list">
            <div class="log-empty">点击“刷新”加载日志</div>
          </div>
        </div>
      </div>
    </div>
      </div>
      <div class="toast-host" id="toast-host"></div>
    </div>

    <script>
    const API_BASE = '';

    function escapeHtml(s) {
      return String(s)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
    }

    function showToast(text, type) {
      const host = document.getElementById('toast-host');
      if (!host) return;
      const t = document.createElement('div');
      t.className = 'toast ' + (type || 'info');
      const dot = document.createElement('div');
      dot.className = 'dot';
      const body = document.createElement('div');
      body.className = 'text';
      body.textContent = String(text || '');
      t.appendChild(dot);
      t.appendChild(body);
      host.appendChild(t);
      const ttl = (type === 'error') ? 5200 : 2800;
      setTimeout(() => {
        try { t.remove(); } catch (_) {}
      }, ttl);
    }

    function clamp01(v) {
      if (!Number.isFinite(v)) return 0;
      return Math.max(0, Math.min(1, v));
    }

    function timeAgo(tsMs) {
      const ts = Number(tsMs || 0);
      if (!ts) return '—';
      const diff = Date.now() - ts;
      if (!Number.isFinite(diff)) return '—';
      const s = Math.max(0, Math.floor(diff / 1000));
      if (s < 10) return '刚刚';
      if (s < 60) return s + ' 秒前';
      const m = Math.floor(s / 60);
      if (m < 60) return m + ' 分钟前';
      const h = Math.floor(m / 60);
      if (h < 24) return h + ' 小时前';
      const d = Math.floor(h / 24);
      return d + ' 天前';
    }

    const PAGE_META = {
      api: { title: 'API 绑定', sub: '配置 AI 服务连接与提示词' },
      persona: { title: '宠物个性', sub: '当前人设与系统提示词' },
      characters: { title: '角色库', sub: '本地角色与商城安装' },
      diary: { title: '宠物日记', sub: '记录与自动生成日记' },
      monitor: { title: '系统监控', sub: '资源、进程与后台日志' }
    };

    function setTopbar(page) {
      const meta = PAGE_META[page] || { title: '管理中心', sub: '后端状态与快捷信息' };
      const t = document.getElementById('topbar-title');
      const s = document.getElementById('topbar-sub');
      if (t) t.textContent = meta.title;
      if (s) s.textContent = meta.sub;
    }

    async function refreshGlobalPills() {
      try {
        const cfg = await fetch(API_BASE + '/api/config').then(r => r.json());
        const backendPill = document.getElementById('pill-backend');
        const backendText = document.getElementById('pill-backend-text');
        if (backendText) backendText.textContent = (cfg.bind || '—');
        if (backendPill) {
          backendPill.classList.remove('ok', 'warn', 'bad');
          backendPill.classList.add('ok');
        }

        const apiPill = document.getElementById('pill-api');
        const apiText = document.getElementById('pill-api-text');
        if (cfg.api_key_set) {
          if (apiText) apiText.textContent = '已配置';
          if (apiPill) { apiPill.classList.remove('ok', 'warn', 'bad'); apiPill.classList.add('ok'); }
        } else {
          if (apiText) apiText.textContent = '未配置';
          if (apiPill) { apiPill.classList.remove('ok', 'warn', 'bad'); apiPill.classList.add('warn'); }
        }
      } catch (_) {}

      try {
        const p = await fetch(API_BASE + '/api/persona/current').then(r => r.json());
        const pill = document.getElementById('pill-persona');
        const text = document.getElementById('pill-persona-text');
        const name = p && p.personality && p.personality.name ? String(p.personality.name) : '';
        if (text) text.textContent = name ? (name.length > 14 ? name.slice(0, 14) + '…' : name) : '未注入';
        if (pill) {
          pill.classList.remove('ok', 'warn', 'bad');
          pill.classList.add(name ? 'ok' : 'warn');
        }
      } catch (_) {}

      try {
        const d = await fetch(API_BASE + '/api/diary').then(r => r.json());
        const last = d && d.data ? Number(d.data.last_log_ts_ms || 0) : 0;
        const pill = document.getElementById('pill-diary');
        const text = document.getElementById('pill-diary-text');
        if (text) text.textContent = last ? ('自动：' + timeAgo(last)) : '未运行';
        if (pill) {
          pill.classList.remove('ok', 'warn', 'bad');
          pill.classList.add(last ? 'ok' : 'warn');
        }

        const autoStatus = document.getElementById('auto-diary-status');
        const autoSub = document.getElementById('auto-diary-sub');
        if (autoStatus) autoStatus.textContent = last ? timeAgo(last) : '暂无记录';
        if (autoSub) autoSub.textContent = last ? ('最后一次自动写入：' + new Date(last).toLocaleString()) : '默认每 10 分钟写一次（首次写入后开始计时）';
      } catch (_) {}
    }
    
    // 导航切换
    document.querySelectorAll('.nav-item').forEach(item => {
      item.addEventListener('click', (e) => {
        if (e && typeof e.preventDefault === 'function') e.preventDefault();
        document.querySelectorAll('.nav-item').forEach(i => i.classList.remove('active'));
        document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
        item.classList.add('active');
        document.getElementById('page-' + item.dataset.page).classList.add('active');
        setTopbar(item.dataset.page);
        if (item.dataset.page === 'api') loadApiConfig();
        if (item.dataset.page === 'characters') loadOwned();
        if (item.dataset.page === 'diary') { loadDiary(); loadDiaryPersonaOptions(); }
        if (item.dataset.page === 'monitor') { refreshMonitor(true); refreshProcesses(); refreshLogs(); }
        refreshGlobalPills();
      });
    });

    const initPage = new URLSearchParams(window.location.search).get('page');
    if (initPage) {
      const el = document.querySelector(`.nav-item[data-page="${initPage}"]`);
      if (el) el.click();
    }
    
    // 加载 API 配置
    async function loadApiConfig() {
      try {
        const res = await fetch(API_BASE + '/api/config');
        const data = await res.json();
        document.getElementById('api-base-url').value = data.base_url || '';
        document.getElementById('api-model').value = data.model || '';
        document.getElementById('api-key').value = '';
        document.getElementById('system-prompt').value = data.system_prompt || '';
        
        const statusEl = document.getElementById('api-status');
        if (data.api_key_set) {
          statusEl.innerHTML = '<span class="status status-on"><span class="status-dot"></span>已配置</span>';
        } else {
          statusEl.innerHTML = '<span class="status status-off"><span class="status-dot"></span>未配置</span>';
        }
      } catch (e) {
        console.error(e);
      }
    }
    
    // 保存 API 配置
    async function saveApiConfig() {
      const base_url = document.getElementById('api-base-url').value;
      const model = document.getElementById('api-model').value;
      const api_key = document.getElementById('api-key').value;
      const system_prompt = document.getElementById('system-prompt').value;
      
      const body = {};
      if (base_url) body.base_url = base_url;
      if (model) body.model = model;
      if (api_key) body.api_key = api_key;
      if (system_prompt) body.system_prompt = system_prompt;
      
      try {
        const res = await fetch(API_BASE + '/api/config', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify(body)
        });
        const data = await res.json();
        if (data.api_key_set) {
          document.getElementById('api-status').innerHTML = '<span class="status status-on"><span class="status-dot"></span>已配置</span>';
        }
        showToast('配置已保存', 'success');
        refreshGlobalPills();
      } catch (e) {
        showToast('保存失败: ' + e, 'error');
      }
    }
    
    // 测试 API
    async function testApi() {
      const resultEl = document.getElementById('test-result');
      resultEl.className = 'test-result show';
      resultEl.textContent = '正在测试...';
      
      try {
        const res = await fetch(API_BASE + '/api/test', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({text: '你好'})
        });
        const data = await res.json();
        if (data.ok) {
          resultEl.className = 'test-result show success';
          resultEl.textContent = '✓ 连接成功！AI 回复: ' + data.reply;
        } else {
          resultEl.className = 'test-result show error';
          resultEl.textContent = '✗ 连接失败: ' + (data.error || '未知错误');
        }
      } catch (e) {
        resultEl.className = 'test-result show error';
        resultEl.textContent = '✗ 请求失败: ' + e;
      }
    }

    async function refreshStoreCoins() {
      const pill = document.getElementById('store-coins-display');
      if (!pill) return;
      pill.textContent = '🪙 读取中…';
      try {
        const res = await fetch(API_BASE + '/api/pet_level');
        const data = await res.json();
        const coins = Math.max(0, Number(data.coins ?? 0));
        pill.textContent = '🪙 ' + coins;
      } catch (e) {
        pill.textContent = '🪙 读取失败';
      }
    }

    async function refreshStoreUserBar(withCoins) {
      const pill = document.getElementById('store-user-display');
      const sub = document.getElementById('store-user-id-sub');
      if (!pill) return;
      pill.textContent = '读取中…';
      if (sub) sub.textContent = '';
      try {
        const res = await fetch(API_BASE + '/api/store/user');
        const data = await res.json();
        const uid = (data && data.user_id != null) ? String(data.user_id).trim() : '';
        const dn = (data && data.display_name != null) ? String(data.display_name).trim() : '';
        const label = dn || uid;
        if (!label) {
          pill.textContent = '未设置';
          if (sub) sub.textContent = '请点下方「打开商城」完成登录；或 POST /api/store/user 设置 user_id / display_name';
        } else {
          pill.textContent = label.length > 16 ? label.slice(0, 16) + '…' : label;
          if (sub) {
            if (dn && uid) sub.innerHTML = 'userId: <code style="font-size:11px;">' + escapeHtml(uid) + '</code>';
            else if (uid) sub.innerHTML = 'userId: <code style="font-size:11px;">' + escapeHtml(uid) + '</code>';
            else sub.textContent = '';
          }
        }
      } catch (e) {
        pill.textContent = '读取失败';
        if (sub) sub.textContent = String(e);
      }
      if (withCoins) refreshStoreCoins();
    }

    function openRoleStore() {
      let base = (document.getElementById('store-base-url')?.value || '').trim();
      if (!base) {
        base = window.prompt('输入你的角色商城地址', 'http://localhost:8080') || '';
      }
      base = (base || '').trim();
      if (!base) return;
      const input = document.getElementById('store-base-url');
      if (input) input.value = base;

      const saved = localStorage.getItem('storeUserId') || 'alice';
      const userId = (window.prompt('输入商城 userId（用于联调登录）', saved) || '').trim();
      if (!userId) return;

      (async () => {
        try {
          let storeOrigin = '';
          try {
            storeOrigin = new URL(base).origin;
          } catch (e) {
            showToast('商城地址无效：' + e, 'error');
            return;
          }
          const res = await fetch(API_BASE + '/api/store/auth/login', {
            method: 'POST',
            headers: {'Content-Type': 'application/json'},
            body: JSON.stringify({ store_base_url: storeOrigin, user_id: userId })
          });
          const data = await res.json().catch(() => ({}));
          const token = data && data.token ? String(data.token) : '';
          if (!res.ok || !token) {
            showToast('商城登录失败：' + (data.error || res.status), 'error');
            return;
          }
          localStorage.setItem('storeBaseUrl', base);
          localStorage.setItem('storeUserId', userId);
          localStorage.setItem('storeToken', token);
          window.open(storeOrigin + '/store.html?token=' + encodeURIComponent(token) + '&userId=' + encodeURIComponent(userId), '_blank');
          refreshStoreUserBar(true);
          showToast('已打开商城（新标签页）', 'success');
        } catch (e) {
          showToast('商城登录失败：' + e, 'error');
        }
      })();
    }

    let storeInstallTimer = null;
    let storeInstallJobId = null;

    function formatBytes(n) {
      if (!Number.isFinite(n)) return '';
      const units = ['B', 'KB', 'MB', 'GB'];
      let v = n;
      let i = 0;
      while (v >= 1024 && i < units.length - 1) { v /= 1024; i += 1; }
      const s = i === 0 ? String(Math.floor(v)) : v.toFixed(1);
      return s + ' ' + units[i];
    }

    function jsArg(v) {
      return JSON.stringify(v).replace(/"/g, '&quot;');
    }

    let ownedCurrentItemId = null;

    function backToOwned() {
      ownedCurrentItemId = null;
      const a = document.getElementById('owned-characters');
      const b = document.getElementById('owned-skins');
      if (a) a.style.display = '';
      if (b) b.style.display = 'none';
    }

    async function loadOwned() {
      refreshStoreUserBar(true);
      backToOwned();
      const grid = document.getElementById('owned-character-grid');
      if (grid) grid.innerHTML = '<div class="download-sub">加载中...</div>';
      try {
        const res = await fetch(API_BASE + '/api/owned/items');
        const data = await res.json();
        const items = data && data.items ? data.items : [];
        if (!items.length) {
          if (grid) grid.innerHTML = '<div class="download-sub">没有已安装的角色</div>';
          return;
        }
        if (grid) grid.innerHTML = items.map(it => {
          const safeName = escapeHtml(it.name || it.id || '');
          const safeDesc = escapeHtml(it.description || '');
          const safeCover = escapeHtml(it.cover || '');
          const count = Number(it.skins_count || 0);
          const badge = count > 0 ? (count + ' 款皮肤') : '无皮肤';
          const hasSkins = count > 0;
          const btnClass = hasSkins ? 'btn btn-primary' : 'btn btn-secondary';
          const btnText = hasSkins ? '进入皮肤' : '无皮肤';
          const btnAttr = hasSkins ? (' onclick="openOwnedSkins(' + jsArg(it.id) + ')"') : ' disabled';
          const builtin = (it.id === 'debug_pet' || it.id === '夕');
          const delAttr = builtin ? ' disabled' : (' onclick="deleteOwned(' + jsArg(it.id) + ')"');
          return '<div class="owned-card">'
            + '<div class="owned-cover">'
            + '<img style="pointer-events:none;" src="' + safeCover + '" alt="" onerror="this.style.display=\'none\'; this.parentElement.style.background=\'var(--surface-hover)\';">'
            + '<div class="owned-badge">' + badge + '</div>'
            + '</div>'
            + '<div class="owned-body">'
            + '<div class="owned-name">' + safeName + '</div>'
            + '<div class="owned-desc">' + safeDesc + '</div>'
            + '<div class="owned-actions">'
            + '<button class="' + btnClass + '"' + btnAttr + '>' + btnText + '</button>'
            + '<button class="btn btn-danger"' + delAttr + '>删除</button>'
            + '</div>'
            + '</div>'
            + '</div>';
        }).join('');
      } catch (e) {
        if (grid) grid.innerHTML = '<div class="download-sub">加载失败：' + e + '</div>';
      }
    }

    async function deleteOwned(itemId) {
      if (!itemId) return;
      const ok = window.confirm('确定要删除该角色吗？\\n\\n这会删除本地 frontend/assets 下的角色模组文件。');
      if (!ok) return;
      try {
        const res = await fetch(API_BASE + '/api/owned/items/' + encodeURIComponent(itemId) + '/delete', { method: 'POST' });
        const data = await res.json().catch(() => ({}));
        if (!res.ok || !data.ok) {
          showToast('删除失败：' + (data.error || res.status), 'error');
          return;
        }
        if (ownedCurrentItemId === itemId) backToOwned();
        await loadOwned();
        await loadDiaryPersonaOptions();
        showToast('已删除：' + itemId, 'success');
      } catch (e) {
        showToast('删除失败：' + e, 'error');
      }
    }

    async function openOwnedSkins(itemId) {
      ownedCurrentItemId = itemId;
      const a = document.getElementById('owned-characters');
      const b = document.getElementById('owned-skins');
      if (a) a.style.display = 'none';
      if (b) b.style.display = '';

      const title = document.getElementById('owned-skins-title');
      const grid = document.getElementById('owned-skin-grid');
      if (grid) grid.innerHTML = '<div class="download-sub">加载中...</div>';
      if (title) title.textContent = '';
      try {
        const res = await fetch(API_BASE + '/api/owned/items/' + encodeURIComponent(itemId) + '/skins');
        const data = await res.json();
        const skins = data && data.skins ? data.skins : [];
        const itemName = data && data.item ? (data.item.name || data.item.id) : itemId;
        if (title) title.textContent = itemName;

        if (!skins.length) {
          if (grid) grid.innerHTML = '<div class="download-sub">没有皮肤</div>';
          return;
        }
        if (grid) grid.innerHTML = skins.map(s => {
          return '<div class="owned-card">'
            + '<div class="owned-cover">'
            + '<img style="pointer-events:none;" src="' + (s.cover || '') + '" alt="" onerror="this.style.display=\'none\'; this.parentElement.style.background=\'var(--surface-hover)\';">'
            + '<div class="owned-badge">' + (s.name || '') + '</div>'
            + '</div>'
            + '<div class="owned-body">'
            + '<div class="owned-name">' + (s.name || '') + '</div>'
            + '<div class="owned-desc"></div>'
            + '</div>'
            + '</div>';
        }).join('');
      } catch (e) {
        if (grid) grid.innerHTML = '<div class="download-sub">加载失败：' + e + '</div>';
      }
    }

    async function loadStore() {
      const base = (document.getElementById('store-base-url')?.value || '').trim() || window.prompt('输入商城地址', 'https://');
      if (!base) return;
      const input = document.getElementById('store-base-url');
      if (input) input.value = base;

      const list = document.getElementById('store-list');
      if (list) list.innerHTML = '<div class="download-sub">加载中...</div>';
      try {
        const token = localStorage.getItem('storeToken') || '';
        const headers = token ? { 'Authorization': 'Bearer ' + token } : undefined;
        const res = await fetch(API_BASE + '/api/store/items?base=' + encodeURIComponent(base), { headers });
        const data = await res.json();
        renderStore(data);
      } catch (e) {
        if (list) list.innerHTML = '<div class="download-sub">加载失败：' + e + '</div>';
      }
    }

    function renderStore(data) {
      const list = document.getElementById('store-list');
      if (!list) return;
      const items = (data && data.items) ? data.items : [];
      if (!items.length) {
        list.innerHTML = '<div class="download-sub">没有商品</div>';
        return;
      }
      list.innerHTML = items.map(it => {
        const safeName = escapeHtml(it.name || it.id || '');
        const safeDesc = escapeHtml(it.description || '');
        const skins = (it.skins || []).map(s => {
          const skinName = s.name || '';
          return '<button class="btn btn-secondary btn-sm" onclick="installSkin('
            + jsArg(it.id) + ', ' + jsArg(skinName) + ')">安装：' + skinName + '</button>';
        }).join('');
        return '<div class="store-item">'
          + '<div class="store-item-top">'
          + '<div class="store-item-name">' + safeName + '</div>'
          + '<div>'
          + '<button class="btn btn-primary btn-sm" onclick="installAll(' + jsArg(it.id) + ')">安装全部皮肤</button>'
          + '</div>'
          + '</div>'
          + '<div class="store-item-desc">' + safeDesc + '</div>'
          + '<div class="store-skins">' + skins + '</div>'
          + '</div>';
      }).join('');
    }

    async function installAll(itemId) {
      return startInstall(itemId, null);
    }

    async function installSkin(itemId, skinName) {
      return startInstall(itemId, skinName);
    }

    async function startInstall(itemId, skinName) {
      const base = (document.getElementById('store-base-url')?.value || '').trim();
      if (!base) { showToast('请先填写商城地址', 'warn'); return; }

      const box = document.getElementById('store-install-box');
      const textEl = document.getElementById('store-install-text');
      const percentEl = document.getElementById('store-install-percent');
      const subEl = document.getElementById('store-install-sub');
      const fillEl = document.getElementById('store-install-fill');
      const progressEl = document.getElementById('store-install-progress');

      if (box) box.style.display = 'block';
      if (textEl) textEl.textContent = '开始下载...';
      if (percentEl) percentEl.textContent = '0%';
      if (subEl) subEl.textContent = '';
      if (fillEl) fillEl.style.width = '0%';
      if (progressEl) progressEl.classList.add('indeterminate');

      if (storeInstallTimer) { clearInterval(storeInstallTimer); storeInstallTimer = null; }
      storeInstallJobId = null;

      try {
        const token = localStorage.getItem('storeToken') || '';
        const res = await fetch(API_BASE + '/api/store/install/start', {
          method: 'POST',
          headers: Object.assign({'Content-Type': 'application/json'}, token ? {'Authorization': 'Bearer ' + token} : {}),
          body: JSON.stringify({ store_base_url: base, item_id: itemId, skin_name: skinName })
        });
        const data = await res.json();
        storeInstallJobId = data.job_id;
        await refreshInstall();
        storeInstallTimer = setInterval(refreshInstall, 250);
      } catch (e) {
        if (textEl) textEl.textContent = '安装启动失败';
        if (subEl) subEl.textContent = String(e);
        if (progressEl) progressEl.classList.remove('indeterminate');
      }
    }

    async function refreshInstall() {
      if (!storeInstallJobId) return;
      const textEl = document.getElementById('store-install-text');
      const percentEl = document.getElementById('store-install-percent');
      const subEl = document.getElementById('store-install-sub');
      const fillEl = document.getElementById('store-install-fill');
      const progressEl = document.getElementById('store-install-progress');

      try {
        const res = await fetch(API_BASE + '/api/store/install/status?job_id=' + encodeURIComponent(storeInstallJobId));
        const data = await res.json();

        if (data.stage === 'extracting') {
          if (textEl) textEl.textContent = '下载完成，正在解压...';
          if (percentEl) percentEl.textContent = '--%';
          if (progressEl) progressEl.classList.add('indeterminate');
        } else if (data.stage === 'downloading') {
          if (textEl) textEl.textContent = '下载中...';
          if (data.percent !== null && data.percent !== undefined) {
            const p = Math.max(0, Math.min(100, Number(data.percent)));
            if (percentEl) percentEl.textContent = p.toFixed(1) + '%';
            if (fillEl) fillEl.style.width = p.toFixed(2) + '%';
            if (progressEl) progressEl.classList.remove('indeterminate');
          } else {
            if (percentEl) percentEl.textContent = '--%';
            if (progressEl) progressEl.classList.add('indeterminate');
          }
        }

        const dl = Number(data.downloaded_bytes || 0);
        const total = data.total_bytes === null || data.total_bytes === undefined ? null : Number(data.total_bytes);
        const left = total ? (' / ' + formatBytes(total)) : '';
        if (subEl) subEl.textContent = formatBytes(dl) + left;

        if (data.done) {
          if (storeInstallTimer) { clearInterval(storeInstallTimer); storeInstallTimer = null; }
          if (progressEl) progressEl.classList.remove('indeterminate');
          if (data.error) {
            if (textEl) textEl.textContent = '安装失败';
            if (percentEl) percentEl.textContent = '失败';
            if (subEl) subEl.textContent = data.error;
          } else {
            if (textEl) textEl.textContent = '安装完成';
            if (percentEl) percentEl.textContent = '完成';
            if (fillEl) fillEl.style.width = '100%';
            if (data.installed_character_id) {
              if (subEl) subEl.textContent = '已安装：' + data.installed_character_id + '。右键桌宠菜单即可热切换。';
            } else {
              if (subEl) subEl.textContent = '右键桌宠菜单即可热切换。';
            }
            // 安装成功后，自动刷新已拥有的角色列表
            loadOwned();
            loadDiaryPersonaOptions();
          }
        }
      } catch (e) {
        if (subEl) subEl.textContent = String(e);
      }
    }
    
    // 更新个性预览
    function updatePreview() {
      const name = document.getElementById('persona-name').value || '可爱的小猫';
      const traits = document.getElementById('persona-traits').value || '活泼 · 好奇 · 黏人';
      
      const avatarEl = document.querySelector('.persona-avatar');
      if (avatarEl) avatarEl.textContent = (name || '🐱').slice(0, 1);
      document.getElementById('persona-name-preview').textContent = name;
      document.getElementById('persona-trait-preview').textContent = traits;
    }

    let currentPersonaTsMs = 0;
    async function refreshCurrentPersona(force) {
      const isForce = !!force;
      try {
        const res = await fetch(API_BASE + '/api/persona/current');
        const data = await res.json();
        if (!data || !data.personality) return;
        if (!isForce && Number(data.ts_ms || 0) && Number(data.ts_ms || 0) === currentPersonaTsMs) return;
        currentPersonaTsMs = Number(data.ts_ms || Date.now());

        const p = data.personality || {};
        const name = (p.name || p.id || '').trim();

        let traitsText = '';
        const traits = p.traits;
        if (traits && typeof traits === 'object') {
          const entries = Object.entries(traits)
            .filter((kv) => typeof kv[1] === 'number')
            .sort((a, b) => (b[1] - a[1]))
            .slice(0, 3)
            .map((kv) => kv[0]);
          if (entries.length) traitsText = entries.join(' · ');
        }
        if (!traitsText) {
          const bio = p.biography || {};
          traitsText = String(bio.identity || bio.experience || bio.belief || bio.goal || '').trim();
        }

        const nameInput = document.getElementById('persona-name');
        const traitsInput = document.getElementById('persona-traits');
        const promptInput = document.getElementById('system-prompt');
        const avatarEl = document.querySelector('.persona-avatar');
        
        if (nameInput && name) nameInput.value = name;
        if (traitsInput && traitsText) traitsInput.value = traitsText;
        if (avatarEl && name) avatarEl.textContent = name.slice(0, 1);
        
        // 动态更新系统提示词：根据角色 character.json 自动生成
        if (promptInput) {
          let dynamicPrompt = `你是「${name}」。\n`;
          if (p.biography) {
            const bio = p.biography;
            if (bio.identity) dynamicPrompt += `身份：${bio.identity}\n`;
            if (bio.experience) dynamicPrompt += `经历：${bio.experience}\n`;
            if (bio.belief) dynamicPrompt += `信念：${bio.belief}\n`;
          }
          if (p.speech_style && p.speech_style.default_phrases) {
            dynamicPrompt += `你的常用口头禅包括：${p.speech_style.default_phrases.slice(0, 3).join('、')}。\n`;
          }
          dynamicPrompt += `请以该角色的身份、语气和性格与我交流，保持自然且符合人设。`;
          promptInput.value = dynamicPrompt;
        }
        
        updatePreview();
      } catch (_) {}
    }
    
    // 每 1.5 秒检查一次当前角色是否发生变化（与下方初始化保持一致）
    // setInterval 放在初始化段统一设置
    
    // 保存个性
    async function savePersona() {
      await saveApiConfig();
      showToast('个性设置已保存', 'success');
    }
    
    // 重置个性
    function resetPersona() {
      document.getElementById('persona-name').value = '可爱的小猫';
      document.getElementById('persona-traits').value = '活泼 · 好奇 · 黏人';
      document.getElementById('system-prompt').value = '你是主人的贴心小宠物，活泼可爱，喜欢撒娇。';
      updatePreview();
    }
    
    // 加载写作角色
    async function loadDiaryPersonaOptions() {
      const sel = document.getElementById('diary-persona-character');
      if (!sel) return;
      const keep = sel.value || '';
      try {
        const res = await fetch(API_BASE + '/api/owned/items');
        const data = await res.json();
        const items = Array.isArray(data.items) ? data.items : [];
        sel.innerHTML = '<option value="">使用当前系统提示词</option>' + items.map(it => {
          const id = escapeHtml(it.id || '');
          const name = escapeHtml(it.name || it.id || '');
          return `<option value="${id}">${name}</option>`;
        }).join('');
        sel.value = keep;
      } catch (e) {
        console.error(e);
      }
    }
    
    // 观察后台写日记
    async function generateDiaryFromProcesses() {
      const btn = document.getElementById('diary-generate-btn');
      if (btn) btn.disabled = true;
      try {
        const characterId = (document.getElementById('diary-persona-character')?.value || '').trim();
        const res = await fetch(API_BASE + '/api/diary/auto_processes', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({ character_id: characterId || null, max_processes: 25 })
        });
        const data = await res.json();
        if (!data.ok) {
          showToast('生成失败: ' + (data.error || 'unknown'), 'error');
          return;
        }
        loadDiary();
        showToast('已生成并写入日记', 'success');
      } catch (e) {
        showToast('生成失败: ' + e, 'error');
      } finally {
        if (btn) btn.disabled = false;
      }
    }
    
    // 加载日记
    async function loadDiary() {
      try {
        const res = await fetch(API_BASE + '/api/diary');
        const data = await res.json();
        const list = document.getElementById('diary-list');
        
        if (data.data.entries && data.data.entries.length > 0) {
          list.innerHTML = data.data.entries.slice().reverse().map(e => {
            const timeStr = escapeHtml(new Date(e.ts_ms).toLocaleString());
            const textHtml = escapeHtml(e.text || '').replace(/\n/g, '<br>');
            return `<div class="diary-item"><div class="diary-time">${timeStr}</div><div class="diary-text">${textHtml}</div></div>`;
          }).join('');
        } else {
          list.innerHTML = '<p style="color: var(--text-dim); font-size: 13px;">暂无日记记录</p>';
        }
        refreshGlobalPills();
      } catch (e) {
        console.error(e);
      }
    }
    
    // 添加日记
    async function addDiary() {
      const text = document.getElementById('diary-input').value.trim();
      if (!text) return;
      
      try {
        await fetch(API_BASE + '/api/diary/append', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({text})
        });
        document.getElementById('diary-input').value = '';
        loadDiary();
        showToast('已添加日记', 'success');
      } catch (e) {
        showToast('添加失败: ' + e, 'error');
      }
    }
    
    // 清空日记
    async function clearDiary() {
      if (!confirm('确定要清空所有日记吗？')) return;
      try {
        await fetch(API_BASE + '/api/diary/clear', {method: 'POST'});
        loadDiary();
        document.getElementById('summary-card').style.display = 'none';
        showToast('已清空日记', 'success');
      } catch (e) {
        showToast('清空失败: ' + e, 'error');
      }
    }
    
    // AI 总结
    async function summarizeDiary() {
      try {
        const res = await fetch(API_BASE + '/api/diary/summarize', {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({})
        });
        const data = await res.json();
        if (data.summary) {
          document.getElementById('summary-card').style.display = 'block';
          document.getElementById('diary-summary').textContent = data.summary;
          showToast('已生成总结', 'success');
        }
      } catch (e) {
        showToast('总结失败: ' + e, 'error');
      }
    }
    
    // 刷新监控
    async function refreshMonitor(showToastOnOk) {
      try {
        const res = await fetch(API_BASE + '/api/monitor');
        const data = await res.json();
        const cpu = Number(data.cpu_usage || 0);
        document.getElementById('cpu-usage').textContent = cpu.toFixed(1) + '%';
        const cpuBar = document.getElementById('cpu-bar');
        if (cpuBar) cpuBar.style.width = (clamp01(cpu / 100) * 100).toFixed(1) + '%';

        const memGB = (data.memory_used / 1024 / 1024 / 1024).toFixed(1);
        const totalGB = (data.memory_total / 1024 / 1024 / 1024).toFixed(1);
        document.getElementById('mem-usage').textContent = memGB + ' / ' + totalGB + ' GB';
        const memPct = Number(data.memory_percent || 0);
        const memBar = document.getElementById('mem-bar');
        if (memBar) memBar.style.width = (clamp01(memPct / 100) * 100).toFixed(1) + '%';
        const memSub = document.getElementById('mem-sub');
        if (memSub) memSub.textContent = '占用 ' + memPct.toFixed(1) + '%';

        const selfMB = (data.self_memory_used / 1024 / 1024).toFixed(1);
        document.getElementById('self-mem-usage').textContent = selfMB + ' MB';
        document.getElementById('proc-count').textContent = data.process_count;

        const focus = data.focused_window ? String(data.focused_window) : '无';
        const focusEl = document.getElementById('focus-window');
        const focusSub = document.getElementById('focus-sub');
        if (focusEl) focusEl.textContent = focus.length > 22 ? focus.slice(0, 22) + '…' : focus;
        if (focusSub) focusSub.textContent = focus;

        const live = document.getElementById('monitor-live');
        if (live) live.style.display = '';
        if (showToastOnOk) showToast('监控已刷新', 'info');
      } catch (e) {
        const live = document.getElementById('monitor-live');
        if (live) live.style.display = 'none';
        console.error(e);
      }
    }

    let PROC_CACHE = [];

    async function refreshProcesses() {
      const body = document.getElementById('proc-table-body');
      if (body) body.innerHTML = '<tr><td colspan="4" style="padding: 14px 12px; color: var(--text-dim);">加载中...</td></tr>';
      try {
        const res = await fetch(API_BASE + '/api/processes?limit=120');
        const data = await res.json();
        PROC_CACHE = Array.isArray(data.processes) ? data.processes : [];
        renderProcesses();
      } catch (e) {
        PROC_CACHE = [];
        if (body) body.innerHTML = '<tr><td colspan="4" style="padding: 14px 12px; color: var(--text-dim);">加载失败：' + escapeHtml(String(e)) + '</td></tr>';
      }
    }

    function renderProcesses() {
      const body = document.getElementById('proc-table-body');
      if (!body) return;
      const q = (document.getElementById('proc-filter')?.value || '').trim().toLowerCase();
      const rows = (PROC_CACHE || []).filter(p => {
        if (!q) return true;
        const name = (p && p.name != null) ? String(p.name).toLowerCase() : '';
        return name.includes(q);
      }).slice(0, 80);
      if (!rows.length) {
        body.innerHTML = '<tr><td colspan="4" style="padding: 14px 12px; color: var(--text-dim);">没有匹配的进程</td></tr>';
        return;
      }
      body.innerHTML = rows.map(p => {
        const name = escapeHtml(String(p.name || ''));
        const pid = escapeHtml(String(p.pid || ''));
        const cpu = Number(p.cpu_usage || 0);
        const memMb = Number(p.memory_kb || 0) / 1024;
        const cpuText = cpu.toFixed(1) + '%';
        const memText = memMb.toFixed(0) + ' MB';
        return '<tr>'
          + '<td>' + name + '</td>'
          + '<td class="mono">' + pid + '</td>'
          + '<td class="mono">' + escapeHtml(cpuText) + '</td>'
          + '<td class="mono">' + escapeHtml(memText) + '</td>'
          + '</tr>';
      }).join('');
    }

    let LOG_CACHE = [];

    function normalizeLevel(s) {
      const v = String(s || '').toLowerCase();
      if (v.includes('error')) return 'error';
      if (v.includes('warn')) return 'warn';
      return 'info';
    }

    async function refreshLogs() {
      const box = document.getElementById('log-list');
      if (box && (!LOG_CACHE || !LOG_CACHE.length)) box.innerHTML = '<div class="log-empty">加载中...</div>';
      try {
        const res = await fetch(API_BASE + '/api/logs');
        const data = await res.json();
        LOG_CACHE = Array.isArray(data) ? data : [];
        renderLogs();
      } catch (e) {
        LOG_CACHE = [];
        if (box) box.innerHTML = '<div class="log-empty">加载失败：' + escapeHtml(String(e)) + '</div>';
      }
    }

    function renderLogs() {
      const box = document.getElementById('log-list');
      if (!box) return;
      const q = (document.getElementById('log-filter')?.value || '').trim().toLowerCase();
      const items = (LOG_CACHE || []).slice().reverse().filter(it => {
        if (!q) return true;
        const msg = (it && it.message != null) ? String(it.message).toLowerCase() : '';
        const lvl = (it && it.level != null) ? String(it.level).toLowerCase() : '';
        return msg.includes(q) || lvl.includes(q);
      }).slice(0, 120);
      if (!items.length) {
        box.innerHTML = '<div class="log-empty">没有日志</div>';
        return;
      }
      box.innerHTML = items.map(it => {
        const ts = Number(it.ts_ms || 0);
        const timeStr = ts ? new Date(ts).toLocaleString() : '—';
        const lvl = normalizeLevel(it.level);
        const msg = escapeHtml(String(it.message || ''));
        const lvlLabel = lvl === 'error' ? 'ERROR' : (lvl === 'warn' ? 'WARN' : 'INFO');
        return '<div class="log-item">'
          + '<div class="log-time mono">' + escapeHtml(timeStr) + '</div>'
          + '<div class="level ' + lvl + '"><span class="dot"></span><span class="mono">' + lvlLabel + '</span></div>'
          + '<div class="log-msg">' + msg + '</div>'
          + '</div>';
      }).join('');
    }

    async function refreshRealWorldInfo() {
      try {
        const res = await fetch(API_BASE + '/api/realworld/info');
        const data = await res.json();
        const timeEl = document.getElementById('realworld-time');
        if (timeEl) {
          timeEl.textContent = data.time;
          document.getElementById('realworld-date').textContent = `${data.date} · ${data.weekday}`;
          if (data.weather) {
            const parts = data.weather.split(' ');
            document.getElementById('realworld-weather-icon').textContent = parts[0] || '🌤️';
            document.getElementById('realworld-weather-text').textContent = parts.slice(1).join(' ') || data.weather;
          }
        }
      } catch (e) {}
    }
    
    // 初始化
    loadApiConfig();
    resetPersona();
    refreshCurrentPersona();
    setTopbar(new URLSearchParams(window.location.search).get('page') || 'api');
    refreshGlobalPills();
    setInterval(refreshGlobalPills, 5000);
    setInterval(refreshCurrentPersona, 1500);
    setInterval(refreshRealWorldInfo, 60000);
    refreshRealWorldInfo();
  </script>
</body>
</html>"#;

    let mut html = html.to_string();
    html = html.replace(
        "class=\"nav-item active\" data-page=\"api\"",
        "class=\"nav-item\" data-page=\"api\"",
    );
    html = html.replace(
        "class=\"page active\" id=\"page-api\"",
        "class=\"page\" id=\"page-api\"",
    );
    html = html.replace(
        &format!("class=\"nav-item\" data-page=\"{}\"", page),
        &format!("class=\"nav-item active\" data-page=\"{}\"", page),
    );
    html = html.replace(
        &format!("class=\"page\" id=\"page-{}\"", page),
        &format!("class=\"page active\" id=\"page-{}\"", page),
    );

    Html(html)
}

async fn get_config(State(state): State<Arc<AppState>>) -> Json<BackendConfigPublic> {
    let cfg = state.config.lock().unwrap();
    Json(BackendConfigPublic {
        bind: cfg.bind.clone(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        system_prompt: cfg.system_prompt.clone(),
        api_key_set: cfg.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
    })
}

#[derive(serde::Serialize)]
struct CurrentPersonaResp {
    ts_ms: u64,
    personality: Option<serde_json::Value>,
}

async fn get_current_persona(State(state): State<Arc<AppState>>) -> Json<CurrentPersonaResp> {
    let ts = *state.current_personality_ts_ms.lock().unwrap();
    let v = state.current_personality.lock().unwrap().clone();
    Json(CurrentPersonaResp {
        ts_ms: ts,
        personality: v,
    })
}

async fn post_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackendConfigUpdate>,
) -> Json<BackendConfigPublic> {
    let mut cfg = state.config.lock().unwrap();
    if let Some(v) = req.bind {
        cfg.bind = v;
    }
    if let Some(v) = req.base_url {
        cfg.base_url = v;
    }
    if let Some(v) = req.model {
        cfg.model = v;
    }
    if let Some(v) = req.system_prompt {
        cfg.system_prompt = v;
    }
    if let Some(v) = req.api_key {
        cfg.api_key = if v.is_empty() { None } else { Some(v) };
    }
    Json(BackendConfigPublic {
        bind: cfg.bind.clone(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        system_prompt: cfg.system_prompt.clone(),
        api_key_set: cfg.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false),
    })
}

async fn get_logs(State(state): State<Arc<AppState>>) -> Json<Vec<BackendLog>> {
    let logs = state.logs.lock().unwrap().clone();
    Json(logs)
}

async fn get_monitor(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let mut monitor = state.monitor.lock().unwrap();
    let data = monitor.get_data();
    Json(serde_json::to_value(data).unwrap_or_default())
}

#[derive(serde::Deserialize)]
struct ProcessesQuery {
    limit: Option<usize>,
}

#[derive(serde::Serialize)]
struct ProcessesResp {
    total: u32,
    processes: Vec<ProcessInfo>,
}

async fn get_processes(
    Query(q): Query<ProcessesQuery>,
    State(state): State<Arc<AppState>>,
) -> Json<ProcessesResp> {
    let mut monitor = state.monitor.lock().unwrap();
    let total = monitor.get_data().process_count;
    let processes = monitor.list_processes(Some(q.limit.unwrap_or(200)));
    Json(ProcessesResp { total, processes })
}

async fn get_memory(State(state): State<Arc<AppState>>) -> Json<MemoryResp> {
    let mem = state.memory.lock().unwrap();
    Json(MemoryResp {
        path: "memory/memory.json".to_string(),
        data: serde_json::to_value(mem.snapshot()).unwrap_or_default(),
    })
}

#[derive(serde::Deserialize)]
struct PetLevelUpdate {
    user_id: Option<String>,
    level: Option<u32>,
    xp: Option<u32>,
    hunger: Option<i32>,
    coins: Option<u32>,
}

#[derive(serde::Deserialize)]
struct PetLevelQuery {
    user_id: Option<String>,
}

async fn get_pet_level(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PetLevelQuery>,
) -> Json<PetLevel> {
    let user_id = q
        .user_id
        .or_else(|| {
            state
                .store_user
                .lock()
                .ok()
                .map(|u| u.user_id.clone())
        })
        .unwrap_or_else(|| "guest".to_string());

    Json(state.get_pet_level(&user_id))
}

#[derive(serde::Serialize)]
struct PetLevelUpdateResp {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<PetLevel>,
}

async fn post_pet_level(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PetLevelUpdate>,
) -> (StatusCode, Json<PetLevelUpdateResp>) {
    let user_id = req
        .user_id
        .clone()
        .or_else(|| {
            state
                .store_user
                .lock()
                .ok()
                .map(|u| u.user_id.clone())
        })
        .unwrap_or_else(|| "guest".to_string());

    let mut current_level = None;
    let ok = state.update_pet_level(&user_id, |level| {
        level.apply_update(req.level, req.xp, req.hunger, req.coins);
        current_level = Some(level.clone());
    });

    if !ok {
        push_log(&state, "error", format!("保存用户 {} 的 pet_level 失败", user_id));
        (StatusCode::INTERNAL_SERVER_ERROR, Json(PetLevelUpdateResp { ok: false, level: None }))
    } else {
        (StatusCode::OK, Json(PetLevelUpdateResp { ok: true, level: current_level }))
    }
}

#[derive(serde::Deserialize)]
struct CoinDeductReq {
    user_id: Option<String>,
    delta: i32,
}

#[derive(serde::Serialize)]
struct CoinDeductResp {
    ok: bool,
    coins: u32,
    error: Option<String>,
}

async fn post_pet_level_coins(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CoinDeductReq>,
) -> Json<CoinDeductResp> {
    let user_id = req
        .user_id
        .clone()
        .or_else(|| {
            state
                .store_user
                .lock()
                .ok()
                .map(|u| u.user_id.clone())
        })
        .unwrap_or_else(|| "guest".to_string());

    let mut coins = 0;
    let mut error = None;

    let ok = state.update_pet_level(&user_id, |level| {
        let current = level.coins;
        // delta > 0：扣金币（购买）；delta < 0：加金币（退款）
        if req.delta > 0 {
            if current < req.delta as u32 {
                error = Some("金币不足".to_string());
                coins = current;
                return;
            }
            level.coins = current - req.delta as u32;
        } else {
            level.coins = current.wrapping_add((-req.delta) as u32);
        }
        coins = level.coins;
    });

    let ok = ok && error.is_none();
    Json(CoinDeductResp {
        ok,
        coins,
        error: error.or_else(|| if !ok { Some("保存失败".to_string()) } else { None }),
    })
}

async fn clear_memory(State(state): State<Arc<AppState>>) -> (StatusCode, Json<ApiOk>) {
    if let Ok(mut mem) = state.memory.lock() {
        mem.clear();
    }
    (StatusCode::OK, Json(ApiOk { ok: true }))
}

async fn get_diary(State(state): State<Arc<AppState>>) -> Json<DiaryResp> {
    let diary = state.diary.lock().unwrap();
    Json(DiaryResp {
        path: "memory/diary.json".to_string(),
        data: serde_json::to_value(diary.snapshot()).unwrap_or_default(),
    })
}

async fn post_diary_append(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DiaryAppendReq>,
) -> Json<DiaryResp> {
    let mut diary = state.diary.lock().unwrap();
    diary.append(req.text.clone());
    Json(DiaryResp {
        path: "memory/diary.json".to_string(),
        data: serde_json::to_value(diary.snapshot()).unwrap_or_default(),
    })
}

async fn clear_diary(State(state): State<Arc<AppState>>) -> (StatusCode, Json<ApiOk>) {
    if let Ok(mut diary) = state.diary.lock() {
        diary.clear();
    }
    (StatusCode::OK, Json(ApiOk { ok: true }))
}

async fn post_diary_summarize(
    State(state): State<Arc<AppState>>,
    Json(_req): Json<serde_json::Value>,
) -> Json<DiarySummarizeResp> {
    let cfg = state.config.lock().unwrap().clone();
    let diary = state.diary.lock().unwrap().snapshot();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let mut content = String::new();
    for e in diary.entries.iter().rev().take(50) {
        content.push_str(&format!("[{}] {}\n", e.ts_ms, e.text));
    }
    let msgs = build_chat_messages(
        &cfg.system_prompt,
        &format!("请总结以下日记要点，50字内：\n{}", content),
    );
    match call_ai_openai_compat(&ai_cfg, &msgs).await {
        Ok(s) => Json(DiarySummarizeResp {
            summary: Some(s),
            error: None,
        }),
        Err(e) => Json(DiarySummarizeResp {
            summary: None,
            error: Some(e.to_string()),
        }),
    }
}

#[derive(serde::Deserialize)]
struct DiaryAutoProcessesReq {
    character_id: Option<String>,
    max_processes: Option<usize>,
}

#[derive(serde::Serialize)]
struct DiaryAutoProcessesResp {
    ok: bool,
    entry: Option<serde_json::Value>,
    error: Option<String>,
}

async fn generate_diary_from_processes_text(
    state: &Arc<AppState>,
    character_id: Option<&str>,
    max_processes: usize,
) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);

    let personality = match character_id {
        Some(id) if !id.trim().is_empty() => load_character_personality(id.trim()),
        _ => state.current_personality.lock().unwrap().clone(),
    };
    let system_prompt = system_prompt_with_personality(&cfg.system_prompt, personality.as_ref());

    let max = max_processes.clamp(5, 200);
    let (sys, procs) = {
        let mut monitor = state.monitor.lock().unwrap();
        let sys = monitor.get_data();
        let procs = monitor.list_processes(Some(max));
        (sys, procs)
    };

    let mut proc_lines = String::new();
    for p in procs.iter() {
        let mem_mb = (p.memory_kb as f64) / 1024.0;
        proc_lines.push_str(&format!(
            "- {} (pid {}) cpu {:.1}% mem {:.0}MB\n",
            p.name, p.pid, p.cpu_usage, mem_mb
        ));
    }

    let focus = sys
        .focused_window
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("无");
    let mem_used_gb = (sys.memory_used as f64) / 1024.0 / 1024.0 / 1024.0;
    let mem_total_gb = (sys.memory_total as f64) / 1024.0 / 1024.0 / 1024.0;
    let self_mb = (sys.self_memory_used as f64) / 1024.0;

    let user_prompt = format!(
        "请你基于以下“电脑后台观察”写一段桌宠日记（第一人称中文，语气自然，80~160字，像在陪主人说话；不要罗列表格，不要逐条复述 pid/进程明细）。\n\n系统概览：CPU {:.1}% / 内存 {:.1}GB/{:.1}GB（{:.1}%）/ 后台自身内存 {:.0}MB / 进程数 {} / 当前前台窗口：{}\n\n进程列表（按占用排序，最多 {} 条）：\n{}",
        sys.cpu_usage,
        mem_used_gb,
        mem_total_gb,
        sys.memory_percent,
        self_mb,
        sys.process_count,
        focus,
        max,
        proc_lines
    );

    let msgs = build_chat_messages(&system_prompt, &user_prompt);
    call_ai_openai_compat(&ai_cfg, &msgs)
        .await
        .map_err(|e| e.to_string())
}

async fn post_diary_auto_processes(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DiaryAutoProcessesReq>,
) -> Json<DiaryAutoProcessesResp> {
    let max = req.max_processes.unwrap_or(25).clamp(5, 200);
    let text = generate_diary_from_processes_text(&state, req.character_id.as_deref(), max).await;
    match text {
        Ok(s) => {
            let entry = {
                let mut diary = state.diary.lock().unwrap();
                diary.append_auto(s)
            };
            Json(DiaryAutoProcessesResp {
                ok: true,
                entry: Some(serde_json::to_value(entry).unwrap_or_default()),
                error: None,
            })
        }
        Err(e) => Json(DiaryAutoProcessesResp {
            ok: false,
            entry: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn post_test(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackendChatReq>,
) -> Json<BackendTestResp> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let msgs = build_test_message(&req.text);

    let reply = call_ai_openai_compat(&ai_cfg, &msgs).await.ok();

    Json(BackendTestResp {
        ok: reply.is_some(),
        reply,
    })
}

async fn post_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BackendChatReq>,
) -> Json<BackendChatResp> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    
    // 优先从请求中获取人设（前端传参），如果没有，则尝试从当前全局状态获取
    let personality_owned = {
        let guard = state.current_personality.lock().unwrap();
        guard.clone()
    };
    let personality = req.personality.as_ref().or(personality_owned.as_ref());

    if let Some(p) = personality {
        let persona_name = p.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
        push_log(&state, "info", format!("人设已注入: name={}", persona_name));
    }
    
    let system_prompt = system_prompt_with_personality(&cfg.system_prompt, personality);
    let msgs = build_chat_messages(&system_prompt, &req.text);

    match call_ai_openai_compat(&ai_cfg, &msgs).await {
        Ok(reply) => {
            push_log(&state, "info", format!("用户: {}", req.text));
            push_log(&state, "info", format!("AI: {}", reply));
            if let Ok(mut mem) = state.memory.lock() {
                mem.apply_exchange(&req.text, &reply);
            }
            Json(BackendChatResp {
                ok: true,
                reply: Some(reply),
                error: None,
            })
        }
        Err(e) => {
            push_log(&state, "error", format!("AI 错误: {}", e));
            Json(BackendChatResp {
                ok: false,
                reply: None,
                error: Some(e.to_string()),
            })
        }
    }
}

fn get_realworld_context_sync() -> String {
    let now = chrono::Local::now();
    let date = now.format("%Y年%m月%d日").to_string();
    let weekday = match now.weekday() {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    };
    let time = now.format("%H:%M").to_string();
    format!("今天是 {} {}，现在时间是 {}。", date, weekday, time)
}

fn system_prompt_with_personality(
    base: &str,
    personality: Option<&serde_json::Value>,
) -> String {
    let realworld = get_realworld_context_sync();
    let Some(p) = personality else {
        return format!("{}\n\n现实世界信息：{}", base, realworld);
    };
    let persona_json = serde_json::to_string_pretty(p).unwrap_or_default();
    format!(
        "{}\n\n现实世界信息：{}\n\n你正在扮演以下桌宠角色。回答要符合人设与语气，不要提及你在扮演。\n人设（JSON）：\n{}",
        base, realworld, persona_json
    )
}

#[derive(serde::Deserialize)]
struct FileSummarizeReq {
    path: String,
}

#[derive(serde::Serialize)]
struct FileSummarizeResp {
    ok: bool,
    summary: Option<String>,
    error: Option<String>,
}

async fn post_file_summarize(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FileSummarizeReq>,
) -> Json<FileSummarizeResp> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let requested = PathBuf::from(&req.path);
    let requested = match std::fs::canonicalize(&requested) {
        Ok(p) => p,
        Err(e) => {
            return Json(FileSummarizeResp {
                ok: false,
                summary: None,
                error: Some(format!("path 无效: {}", e)),
            })
        }
    };

    let mut allow_roots: Vec<PathBuf> = vec![];
    allow_roots.push(assets_dir());
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            allow_roots.push(dir.join("memory"));
            allow_roots.push(dir.join("data"));
        }
    }
    allow_roots.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("frontend")
            .join("assets"),
    );
    allow_roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("memory"));

    let mut allowed = false;
    for r in allow_roots {
        if let Ok(rc) = std::fs::canonicalize(&r) {
            if requested.starts_with(&rc) {
                allowed = true;
                break;
            }
        }
    }
    if !allowed {
        return Json(FileSummarizeResp {
            ok: false,
            summary: None,
            error: Some("禁止访问该路径（仅允许 assets/memory/data 目录）".to_string()),
        });
    }

    let content = std::fs::read_to_string(&requested).unwrap_or_default();
    let mem_block = {
        let mem = state.memory.lock().unwrap();
        mem.build_memory_block("文件摘要")
    };
    let input = format!(
        "根据用户记忆与上下文，摘要此文件：\n{}\n---\n{}",
        mem_block, content
    );
    let msgs = build_chat_messages(&cfg.system_prompt, &input);
    match call_ai_openai_compat(&ai_cfg, &msgs).await {
        Ok(s) => {
            if let Ok(mut diary) = state.diary.lock() {
                diary.append(format!("文件摘要: {} => {}", req.path, s));
            }
            if let Ok(mut mem) = state.memory.lock() {
                mem.apply_user_message(&format!("文件摘要: {}", s));
            }
            Json(FileSummarizeResp {
                ok: true,
                summary: Some(s),
                error: None,
            })
        }
        Err(e) => Json(FileSummarizeResp {
            ok: false,
            summary: None,
            error: Some(e.to_string()),
        }),
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn now_ts_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(serde::Serialize)]
struct WsPetStats {
    hunger: i32,
}

#[derive(serde::Serialize)]
struct WsAutoTalkEvent {
    event: &'static str,
    source: &'static str,
    ok: bool,
    level: Option<u32>,
    text: String,
    stats: Option<WsPetStats>,
    error: Option<String>,
    timestamp: i64,
}

#[derive(serde::Deserialize)]
struct WsClientEvent {
    event: String,
    level: Option<u32>,
    delta: Option<i32>,
    text: Option<String>,
    #[serde(default)]
    personality: Option<serde_json::Value>,
}

fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        }
        Err(_) => false,
    }
}

fn start_auto_diary_writer(state: Arc<AppState>) {
    if env_flag("DIARY_AUTO_DISABLED") {
        return;
    }

    let interval_secs = std::env::var("DIARY_AUTO_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600)
        .clamp(60, 24 * 60 * 60);
    let max_processes = std::env::var("DIARY_AUTO_MAX_PROCESSES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(25)
        .clamp(5, 200);

    tokio::spawn(async move {
        loop {
            let now = now_ms();
            let last = state
                .diary
                .lock()
                .ok()
                .map(|d| d.snapshot().last_log_ts_ms)
                .unwrap_or(0);

            let interval_ms = interval_secs.saturating_mul(1000);
            let elapsed_ms = now.saturating_sub(last);

            if last == 0 || elapsed_ms >= interval_ms {
                let cfg = state.config.lock().unwrap().clone();
                let has_key = cfg
                    .api_key
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                if !has_key {
                    push_log(&state, "info", "自动日记已跳过：未配置 AI_API_KEY".to_string());
                } else {
                    match generate_diary_from_processes_text(&state, None, max_processes).await {
                        Ok(s) => {
                            let _ = state
                                .diary
                                .lock()
                                .map(|mut d| d.append_auto(s));
                            push_log(&state, "info", "自动日记已写入（后台进程观察）".to_string());
                        }
                        Err(e) => {
                            push_log(&state, "error", format!("自动日记失败: {e}"));
                        }
                    }
                }
            }

            let now2 = now_ms();
            let last2 = state
                .diary
                .lock()
                .ok()
                .map(|d| d.snapshot().last_log_ts_ms)
                .unwrap_or(0);
            let elapsed2 = now2.saturating_sub(last2);
            let sleep_ms = interval_ms.saturating_sub(elapsed2).clamp(5_000, interval_ms);
            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
        }
    });
}

async fn ws_send_auto_talk_event(
    socket: &mut WebSocket,
    msg: &WsAutoTalkEvent,
    force_text: bool,
) -> bool {
    if !force_text {
        match bincode::serialize(msg) {
            Ok(bin) => {
                println!(
                    "[ws_auto_talk] send binary len={} source={} ok={}",
                    bin.len(),
                    msg.source,
                    msg.ok
                );
                return socket.send(Message::Binary(bin)).await.is_ok();
            }
            Err(e) => {
                println!("[ws_auto_talk] send binary serialize_err={}", e);
            }
        }
    } else {
        println!(
            "[ws_auto_talk] send text forced source={} ok={}",
            msg.source, msg.ok
        );
    }

    match serde_json::to_string(msg) {
        Ok(s) => {
            println!(
                "[ws_auto_talk] send text fallback len={} source={} ok={}",
                s.len(),
                msg.source,
                msg.ok
            );
            socket.send(Message::Text(s)).await.is_ok()
        }
        Err(e) => {
            println!("[ws_auto_talk] send text serialize_err={}", e);
            false
        }
    }
}

async fn generate_auto_talk_text(
    state: &Arc<AppState>,
    personality: Option<&serde_json::Value>,
) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let mem_block = {
        let mem = state.memory.lock().unwrap();
        mem.build_memory_block("自治闲聊")
    };
    let mon = {
        let mut m = state.monitor.lock().unwrap();
        serde_json::to_string(&m.get_data()).unwrap_or_default()
    };
    let prompt = format!(
        "根据监控与记忆，生成一句简短自然的自言自语：\n{}\n{}",
        mem_block, mon
    );
    let system_prompt = system_prompt_with_personality(&cfg.system_prompt, personality);
    let msgs = build_chat_messages(&system_prompt, &prompt);
    call_ai_openai_compat(&ai_cfg, &msgs)
        .await
        .map_err(|e| e.to_string())
}

async fn generate_interaction_text(
    state: &Arc<AppState>,
    kind: &str,
    level: Option<u32>,
    personality: Option<&serde_json::Value>,
) -> Result<String, String> {
    let cfg = state.config.lock().unwrap().clone();
    let ai_cfg = AiConfig::new(cfg.base_url, cfg.model, cfg.api_key);
    let mem_block = {
        let mem = state.memory.lock().unwrap();
        mem.build_memory_block("互动")
    };
    let mon = {
        let mut m = state.monitor.lock().unwrap();
        serde_json::to_string(&m.get_data()).unwrap_or_default()
    };

    let hint = match kind {
        "pet_clicked" => "用户点击了桌宠，请用一句简短可爱自然的话回应（不超过15字）。",
        "level_up" => {
            let lv = level.unwrap_or(0);
            return Ok(format!("升级啦！现在是 {lv} 级"));
        }
        _ => "请用一句简短自然的话回应。",
    };
    let prompt = format!("{}\n{}\n{}", hint, mem_block, mon);
    let system_prompt = system_prompt_with_personality(&cfg.system_prompt, personality);
    let msgs = build_chat_messages(&system_prompt, &prompt);
    call_ai_openai_compat(&ai_cfg, &msgs)
        .await
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct AutoTalkResp {
    ok: bool,
    text: Option<String>,
    error: Option<String>,
}

async fn post_auto_talk(State(state): State<Arc<AppState>>) -> Json<AutoTalkResp> {
    match generate_auto_talk_text(&state, None).await {
        Ok(s) => Json(AutoTalkResp {
            ok: true,
            text: Some(s),
            error: None,
        }),
        Err(e) => Json(AutoTalkResp {
            ok: false,
            text: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn ws_auto_talk(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_auto_talk_loop(socket, state))
}

async fn ws_auto_talk_loop(mut socket: WebSocket, state: Arc<AppState>) {
    let interval_ms = std::env::var("WS_AUTO_TALK_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30_000);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));

    let mock_text = std::env::var("WS_AUTO_TALK_MOCK_TEXT")
        .ok()
        .filter(|s| !s.is_empty());
    let mut mock_fail_once = env_flag("WS_AUTO_TALK_MOCK_FAIL_ONCE");
    let mut force_text_once = env_flag("WS_AUTO_TALK_FORCE_TEXT_ONCE");
    let mut last_personality: Option<serde_json::Value> = None;

    let first = if mock_fail_once {
        mock_fail_once = false;
        Err("mock ai failed".to_string())
    } else if let Some(t) = &mock_text {
        Ok(t.clone())
    } else {
        generate_auto_talk_text(&state, last_personality.as_ref()).await
    };
    let first_msg = WsAutoTalkEvent {
        event: "auto_talk",
        source: "timer",
        ok: first.is_ok(),
        level: None,
        text: first.clone().unwrap_or_default(),
        stats: None,
        error: first.err(),
        timestamp: now_ts(),
    };
    let force_text = if force_text_once {
        force_text_once = false;
        true
    } else {
        false
    };
    if !ws_send_auto_talk_event(&mut socket, &first_msg, force_text).await {
        return;
    }

    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let text = if mock_fail_once {
                    mock_fail_once = false;
                    Err("mock ai failed".to_string())
                } else if let Some(t) = &mock_text {
                    Ok(t.clone())
                } else {
                    generate_auto_talk_text(&state, last_personality.as_ref()).await
                };
                let msg = WsAutoTalkEvent {
                    event: "auto_talk",
                    source: "timer",
                    ok: text.is_ok(),
                    level: None,
                    text: text.clone().unwrap_or_default(),
                    stats: None,
                    error: text.err(),
                    timestamp: now_ts(),
                };
                let force_text = if force_text_once {
                    force_text_once = false;
                    true
                } else {
                    false
                };
                if !ws_send_auto_talk_event(&mut socket, &msg, force_text).await {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(s))) => {
                        if let Ok(evt) = serde_json::from_str::<WsClientEvent>(&s) {
                            println!("[ws_auto_talk] recv event={} level={:?}", evt.event, evt.level);
                            if let Some(p) = evt.personality.as_ref() {
                                last_personality = Some(p.clone());
                                if let Ok(mut g) = state.current_personality.lock() {
                                    *g = Some(p.clone());
                                }
                                if let Ok(mut g) = state.current_personality_ts_ms.lock() {
                                    *g = now_ts_ms();
                                }
                            }
                            match evt.event.as_str() {
                                "persona" => {}
                                "pet_clicked" => {
                                    let persona = evt.personality.as_ref().or(last_personality.as_ref());
                                    let (ok, text, err) = match generate_interaction_text(&state, "pet_clicked", None, persona).await {
                                        Ok(t) => (true, t, None),
                                        Err(e) => (false, String::new(), Some(e)),
                                    };
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "pet_clicked",
                                        ok,
                                        level: None,
                                        text,
                                        stats: None,
                                        error: err,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "level_up" => {
                                    let lvl = evt.level.unwrap_or(0);
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "level_up",
                                        ok: true,
                                        level: Some(lvl),
                                        text: String::new(),
                                        stats: None,
                                        error: None,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "feed" => {
                                    let delta = evt.delta.unwrap_or(25);
                                    let user_id = state
                                        .store_user
                                        .lock()
                                        .ok()
                                        .map(|u| u.user_id.clone())
                                        .unwrap_or_else(|| "guest".to_string());
                                    let mut h = 100;
                                    let ok_saved = state.update_pet_level(&user_id, |g| {
                                        let next = g.hunger + delta;
                                        g.apply_update(None, None, Some(next), None);
                                        h = g.hunger;
                                    });
                                    let hunger = h;
                                    let text = evt.text.unwrap_or_else(|| "博士，谢谢款待~".to_string());
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "feed",
                                        ok: ok_saved,
                                        level: None,
                                        text,
                                        stats: Some(WsPetStats { hunger }),
                                        error: if ok_saved { None } else { Some("save_failed".to_string()) },
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bin))) => {
                        if let Ok(evt) = bincode::deserialize::<WsClientEvent>(&bin) {
                            println!("[ws_auto_talk] recv event={} level={:?}", evt.event, evt.level);
                            if let Some(p) = evt.personality.as_ref() {
                                last_personality = Some(p.clone());
                                if let Ok(mut g) = state.current_personality.lock() {
                                    *g = Some(p.clone());
                                }
                                if let Ok(mut g) = state.current_personality_ts_ms.lock() {
                                    *g = now_ts_ms();
                                }
                            }
                            match evt.event.as_str() {
                                "persona" => {}
                                "pet_clicked" => {
                                    let persona = evt.personality.as_ref().or(last_personality.as_ref());
                                    let (ok, text, err) = match generate_interaction_text(&state, "pet_clicked", None, persona).await {
                                        Ok(t) => (true, t, None),
                                        Err(e) => (false, String::new(), Some(e)),
                                    };
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "pet_clicked",
                                        ok,
                                        level: None,
                                        text,
                                        stats: None,
                                        error: err,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "level_up" => {
                                    let lvl = evt.level.unwrap_or(0);
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "level_up",
                                        ok: true,
                                        level: Some(lvl),
                                        text: String::new(),
                                        stats: None,
                                        error: None,
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                "feed" => {
                                    let delta = evt.delta.unwrap_or(25);
                                    let user_id = state
                                        .store_user
                                        .lock()
                                        .ok()
                                        .map(|u| u.user_id.clone())
                                        .unwrap_or_else(|| "guest".to_string());
                                    let mut h = 100;
                                    let ok_saved = state.update_pet_level(&user_id, |g| {
                                        let next = g.hunger + delta;
                                        g.apply_update(None, None, Some(next), None);
                                        h = g.hunger;
                                    });
                                    let hunger = h;
                                    let text = evt.text.unwrap_or_else(|| "博士，谢谢款待~".to_string());
                                    let resp = WsAutoTalkEvent {
                                        event: "auto_talk",
                                        source: "feed",
                                        ok: ok_saved,
                                        level: None,
                                        text,
                                        stats: Some(WsPetStats { hunger }),
                                        error: if ok_saved { None } else { Some("save_failed".to_string()) },
                                        timestamp: now_ts(),
                                    };
                                    let _ = ws_send_auto_talk_event(&mut socket, &resp, false).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

#[derive(serde::Serialize)]
struct RealWorldInfo {
    date: String,
    time: String,
    weekday: String,
    weather: Option<String>,
    location: Option<String>,
    lunar: Option<String>,
    festivals: Vec<String>,
}

async fn get_realworld_info() -> Json<RealWorldInfo> {
    let now = chrono::Local::now();
    let date = now.format("%Y年%m月%d日").to_string();
    let time = now.format("%H:%M").to_string();
    let weekday = match now.weekday() {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    }.to_string();

    // 尝试获取天气 (使用 wttr.in 简单的文本格式)
    let weather = match reqwest::get("https://wttr.in?format=%c+%t+%C").await {
        Ok(resp) => resp.text().await.ok().map(|s| s.trim().to_string()),
        Err(_) => None,
    };

    Json(RealWorldInfo {
        date,
        time,
        weekday,
        weather,
        location: Some("未知".to_string()),
        lunar: None, // 农历计算较复杂，暂缺
        festivals: vec![],
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let bind = std::env::var("BACKEND_BIND").unwrap_or_else(|_| "127.0.0.1:4317".to_string());

    let state = Arc::new(AppState::new(BackendConfig {
        bind: bind.clone(),
        base_url: std::env::var("AI_BASE_URL")
            .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string()),
        model: std::env::var("AI_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string()),
        system_prompt: std::env::var("AI_SYSTEM")
            .unwrap_or_else(|_| "你是桌宠的聊天助手，回答简洁自然。".to_string()),
        api_key: std::env::var("AI_API_KEY").ok(),
    }));

    start_auto_diary_writer(state.clone());

    tracing::info!("后台平台启动：http://{}", bind);

    let app = Router::new()
        .route("/", get(index))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/persona/current", get(get_current_persona))
        .route("/api/realworld/info", get(get_realworld_info))
        .route("/api/logs", get(get_logs))
        .route("/api/monitor", get(get_monitor))
        .route("/api/processes", get(get_processes))
        .route("/api/memory", get(get_memory))
        .route("/api/memory/clear", post(clear_memory))
        .route("/api/pet_level", get(get_pet_level).post(post_pet_level))
        .route("/api/pet_level/coins", post(post_pet_level_coins))
        .route("/api/owned/items", get(get_owned_items))
        .route("/api/owned/items/:item_id/delete", post(post_delete_owned_item))
        .route("/api/owned/items/:item_id/skins", get(get_owned_item_skins))
        .route("/api/owned/item-covers/:item_id", get(get_owned_item_cover))
        .route(
            "/api/owned/covers/:item_id/:skin_name",
            get(get_owned_skin_cover),
        )
        .route("/api/store/auth/login", post(post_store_auth_login))
        .route("/api/store/user", get(get_store_user).post(post_store_user))
        .route("/api/store/items", get(get_store_items))
        .route("/api/store/install/start", post(post_store_install_start))
        .route("/api/store/install/status", get(get_store_install_status))
        .route("/api/diary", get(get_diary))
        .route("/api/diary/append", post(post_diary_append))
        .route("/api/diary/clear", post(clear_diary))
        .route("/api/diary/summarize", post(post_diary_summarize))
        .route("/api/diary/auto_processes", post(post_diary_auto_processes))
        .route("/api/file/summarize", post(post_file_summarize))
        .route("/api/auto_talk", post(post_auto_talk))
        .route("/ws/auto_talk", get(ws_auto_talk))
        .route("/api/test", post(post_test))
        .route("/api/chat", post(post_chat))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::AllowMethods::any())
                .allow_headers(tower_http::cors::AllowHeaders::any()),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let addr: std::net::SocketAddr = bind.parse().expect("无效的地址");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("监听地址: http://{}", addr);

    // 启动资产目录监控任务：自动解压手动放入的 .zip 文件
    let state_for_waiter = state.clone();
    tokio::spawn(async move {
        let assets = assets_dir();
        tracing::info!("资产监控任务启动，正在监听目录: {:?}", assets);
        println!("资产监控任务启动，正在监听目录: {:?}", assets);
        loop {
            if let Ok(entries) = std::fs::read_dir(&assets) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("zip") {
                        tracing::info!("检测到新 ZIP 压缩包: {:?}，正在自动解压...", path);
                        let assets_clone = assets.clone();
                        let path_clone = path.clone();
                        
                        // 执行解压
                        let result = tokio::task::spawn_blocking(move || {
                            unzip_to_dir(&path_clone, &assets_clone)
                        }).await;

                        match result {
                            Ok(Ok(_)) => {
                                tracing::info!("自动解压成功: {:?}", path);
                                // 解压成功后删除 zip 文件
                                let _ = std::fs::remove_file(&path);
                                push_log(&state_for_waiter, "info", format!("自动安装成功: {:?}", path.file_name().unwrap_or_default()));
                            }
                            Ok(Err(e)) => {
                                tracing::error!("自动解压失败: {}", e);
                                // 为了防止死循环，失败的 zip 改个名字
                                let mut new_path = path.clone();
                                new_path.set_extension("zip.error");
                                let _ = std::fs::rename(&path, new_path);
                            }
                            Err(e) => tracing::error!("解压任务崩溃: {}", e),
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    });

    axum::serve(listener, app).await.unwrap();
}
