//! Character Store（角色下载/商城）
use crate::mod_loader;
use reqwest::Url;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    AlreadyInstalled,
    Installed,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sanitize_child_name(s: &str) -> Result<&str, String> {
    if s.trim().is_empty() {
        return Err("为空".to_string());
    }
    if s.contains('/') || s.contains('\\') || s.contains("..") {
        return Err("包含非法路径字符".to_string());
    }
    Ok(s)
}

fn sanitize_file_component(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        let bad = matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
        if bad || ch.is_control() {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn store_url(base: &str, segments: &[&str]) -> Result<Url, String> {
    let mut url = Url::parse(base).map_err(|e| format!("store_base_url 无效: {}", e))?;
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|_| "store_base_url 不支持作为 base".to_string())?;
        segs.pop_if_empty();
        segs.extend(segments.iter().copied());
    }
    Ok(url)
}

fn extract_first_u32(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().and_then(|x| u32::try_from(x).ok()),
        serde_json::Value::String(s) => s.trim().parse::<u32>().ok(),
        serde_json::Value::Object(map) => {
            for k in ["coins", "coin", "balance", "money", "amount"] {
                if let Some(v2) = map.get(k) {
                    if let Some(n) = extract_first_u32(v2) {
                        return Some(n);
                    }
                }
            }
            for v2 in map.values() {
                if let Some(n) = extract_first_u32(v2) {
                    return Some(n);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => {
            for v2 in arr {
                if let Some(n) = extract_first_u32(v2) {
                    return Some(n);
                }
            }
            None
        }
        _ => None,
    }
}

pub fn fetch_account_coins(store_base_url: &str) -> Result<u32, String> {
    let store_base_url = store_base_url.trim();
    if store_base_url.is_empty() {
        return Err("store_base_url 为空".to_string());
    }
    let client = reqwest::blocking::Client::new();
    let candidates: [&[&str]; 6] = [
        &["api", "user", "balance"],
        &["api", "wallet", "balance"],
        &["api", "wallet"],
        &["api", "balance"],
        &["api", "coins"],
        &["api", "me"],
    ];
    let mut last_err = None::<String>;
    for segs in candidates {
        let url = match store_url(store_base_url, segs) {
            Ok(u) => u,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };
        let resp = match client.get(url.clone()).send() {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(format!("请求失败: {}", e));
                continue;
            }
        };
        if !resp.status().is_success() {
            last_err = Some(format!("HTTP {}", resp.status()));
            continue;
        }
        let bytes = match resp.bytes() {
            Ok(b) => b,
            Err(e) => {
                last_err = Some(format!("读取失败: {}", e));
                continue;
            }
        };
        let v: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                last_err = Some("响应不是 JSON".to_string());
                continue;
            }
        };
        if let Some(n) = extract_first_u32(&v) {
            return Ok(n);
        }
        last_err = Some("响应中未找到金币字段".to_string());
    }
    Err(last_err.unwrap_or_else(|| "获取金币失败".to_string()))
}

fn download_zip_bytes(url: Url) -> Result<Vec<u8>, String> {
    println!(
        "[store] downloading {}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or(""),
        url.path()
    );
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(url.clone())
        .send()
        .map_err(|e| format!("下载失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }
    let first = resp
        .bytes()
        .map_err(|e| format!("读取下载内容失败: {}", e))?
        .to_vec();
    println!("[store] downloaded {} bytes (first response)", first.len());

    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&first) {
        if let Some(dl) = v.get("download_url").and_then(|x| x.as_str()) {
            let dl_url = Url::parse(dl).map_err(|e| format!("download_url 无效: {}", e))?;
            println!(
                "[store] resolved download_url {}://{}{}",
                dl_url.scheme(),
                dl_url.host_str().unwrap_or(""),
                dl_url.path()
            );
            let resp2 = client
                .get(dl_url)
                .send()
                .map_err(|e| format!("下载失败: {}", e))?;
            if !resp2.status().is_success() {
                return Err(format!("下载失败: HTTP {}", resp2.status()));
            }
            let bytes = resp2
                .bytes()
                .map_err(|e| format!("读取下载内容失败: {}", e))?
                .to_vec();
            println!("[store] downloaded {} bytes (download_url)", bytes.len());
            return Ok(bytes);
        }
    }

    Ok(first)
}

fn temp_dirs() -> (PathBuf, PathBuf) {
    let assets = mod_loader::assets_dir();
    let base = assets.parent().unwrap_or(&assets).to_path_buf();
    (base.join("temp_downloads"), base.join("temp_extract"))
}

fn extract_zip_safe(zip_path: &Path, out_dir: &Path) -> Result<(), String> {
    let f = fs::File::open(zip_path).map_err(|e| format!("打开 zip 失败: {}", e))?;
    let mut archive = ZipArchive::new(f).map_err(|e| format!("读取 zip 失败: {}", e))?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {}", e))?;
        let name = file.name().to_string();
        let rel = Path::new(&name);
        if rel.components().any(|c| {
            matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        }) {
            return Err("zip 包含不安全路径".to_string());
        }
        let out_path = out_dir.join(rel);
        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("创建目录失败: {}", e))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| format!("读取 zip 内容失败: {}", e))?;
        let mut of = fs::File::create(&out_path).map_err(|e| format!("写入文件失败: {}", e))?;
        of.write_all(&buf)
            .map_err(|e| format!("写入文件失败: {}", e))?;
    }
    Ok(())
}

fn find_skins_root(extract_root: &Path) -> Result<PathBuf, String> {
    if extract_root.join("skins").is_dir() {
        return Ok(extract_root.to_path_buf());
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(extract_root) {
        for ent in entries.flatten() {
            let p = ent.path();
            if !p.is_dir() {
                continue;
            }
            if p.join("skins").is_dir() {
                candidates.push(p);
            }
        }
    }
    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }
    Err("未找到有效皮肤目录（缺少 skins/）".to_string())
}

fn move_dir_children(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("创建目录失败: {}", e))?;
    let entries = fs::read_dir(src).map_err(|e| format!("读取目录失败: {}", e))?;
    for ent in entries.flatten() {
        let p = ent.path();
        let Some(name) = p.file_name().and_then(|x| x.to_str()).map(|s| s.to_string()) else {
            continue;
        };
        let to = dest.join(name);
        if to.exists() {
            return Err("安装失败：目标目录存在同名文件/目录".to_string());
        }
        fs::rename(&p, &to).map_err(|e| format!("移动文件失败: {}", e))?;
    }
    Ok(())
}

fn ensure_character_json(dest: &Path, item_id: &str) -> Result<(), String> {
    let p = dest.join("character.json");
    if p.is_file() {
        return Ok(());
    }
    let v = serde_json::json!({ "name": item_id });
    let s = serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{\"name\":\"\"}".to_string());
    fs::write(&p, s).map_err(|e| format!("写入 character.json 失败: {}", e))?;
    Ok(())
}

fn ensure_dir_empty_or_create(p: &Path) -> Result<(), String> {
    if p.is_dir() {
        let ok_empty = fs::read_dir(p).map_err(|e| e.to_string())?.next().is_none();
        if ok_empty {
            return Ok(());
        }
        return Err("临时目录被占用".to_string());
    }
    fs::create_dir_all(p).map_err(|e| format!("创建目录失败: {}", e))?;
    Ok(())
}

pub fn download_and_use_character(store_base_url: &str, item_id: &str) -> Result<InstallOutcome, String> {
    let item_id = sanitize_child_name(item_id)?;
    let assets = mod_loader::assets_dir();
    let dest = assets.join(item_id);
    let already_ok = dest.join("character.json").is_file() && dest.join("skins").is_dir();
    if already_ok {
        println!(
            "[store] character already installed item_id={} dest={}",
            item_id,
            dest.display()
        );
        mod_loader::request_character(item_id.to_string());
        return Ok(InstallOutcome::AlreadyInstalled);
    }
    if dest.join("skins").is_dir() && !dest.join("character.json").is_file() {
        ensure_character_json(&dest, item_id)?;
        println!(
            "[store] character had skins but missing character.json, repaired item_id={} dest={}",
            item_id,
            dest.display()
        );
        mod_loader::request_character(item_id.to_string());
        return Ok(InstallOutcome::AlreadyInstalled);
    }

    if dest.exists() {
        return Err("目标目录已存在但不完整，已拒绝覆盖".to_string());
    }

    let (dl_dir, ex_dir) = temp_dirs();
    fs::create_dir_all(&dl_dir).map_err(|e| format!("创建 temp_downloads 失败: {}", e))?;
    fs::create_dir_all(&ex_dir).map_err(|e| format!("创建 temp_extract 失败: {}", e))?;
    println!(
        "[store] character install start item_id={} dl_dir={} ex_dir={}",
        item_id,
        dl_dir.display(),
        ex_dir.display()
    );

    let zip_name = format!("{}-all.zip", sanitize_file_component(item_id));
    let zip_path = dl_dir.join(zip_name);

    let url = store_url(store_base_url, &["api", "store", "packages", item_id])?;
    let bytes = download_zip_bytes(url)?;
    fs::write(&zip_path, &bytes).map_err(|e| format!("保存 zip 失败: {}", e))?;
    println!(
        "[store] saved character zip {} ({} bytes)",
        zip_path.display(),
        bytes.len()
    );

    let extract_root = ex_dir.join(format!("{}_{}", sanitize_file_component(item_id), now_ms()));
    ensure_dir_empty_or_create(&extract_root)?;
    println!("[store] extracting to {}", extract_root.display());

    let extracted = (|| {
        extract_zip_safe(&zip_path, &extract_root)?;
        let root = find_skins_root(&extract_root)?;
        if !root.join("skins").is_dir() {
            return Err("校验失败：资源包缺少 skins/".to_string());
        }
        println!(
            "[store] validation ok (skins/ found at {}), moving to {}",
            root.display(),
            dest.display()
        );
        fs::create_dir_all(&dest).map_err(|e| format!("创建角色目录失败: {}", e))?;
        move_dir_children(&root, &dest)?;
        ensure_character_json(&dest, item_id)?;
        Ok(())
    })();

    let _ = fs::remove_file(&zip_path);
    let _ = fs::remove_dir_all(&extract_root);

    extracted?;
    mod_loader::request_character(item_id.to_string());
    println!("[store] character installed dest={}", dest.display());
    Ok(InstallOutcome::Installed)
}

pub fn download_and_use_skin(
    store_base_url: &str,
    item_id: &str,
    skin_name: &str,
) -> Result<InstallOutcome, String> {
    let item_id = sanitize_child_name(item_id)?;
    let skin_name = sanitize_child_name(skin_name)?;
    let assets = mod_loader::assets_dir();
    let character_dir = assets.join(item_id);
    if character_dir.join("skins").is_dir() && !character_dir.join("character.json").is_file() {
        ensure_character_json(&character_dir, item_id)?;
        println!(
            "[store] character had skins but missing character.json, repaired item_id={} dest={}",
            item_id,
            character_dir.display()
        );
    }
    if !(character_dir.join("character.json").is_file() && character_dir.join("skins").is_dir()) {
        return Err("角色未安装，请先下载并使用角色整包".to_string());
    }
    let dest = character_dir.join("skins").join(skin_name);
    if dest.is_dir() {
        println!(
            "[store] skin already installed item_id={} skin_name={} dest={}",
            item_id,
            skin_name,
            dest.display()
        );
        mod_loader::request_character(item_id.to_string());
        mod_loader::request_skin(skin_name.to_string());
        return Ok(InstallOutcome::AlreadyInstalled);
    }

    let (dl_dir, ex_dir) = temp_dirs();
    fs::create_dir_all(&dl_dir).map_err(|e| format!("创建 temp_downloads 失败: {}", e))?;
    fs::create_dir_all(&ex_dir).map_err(|e| format!("创建 temp_extract 失败: {}", e))?;
    println!(
        "[store] skin install start item_id={} skin_name={} dl_dir={} ex_dir={}",
        item_id,
        skin_name,
        dl_dir.display(),
        ex_dir.display()
    );

    let zip_name = format!(
        "{}-{}.zip",
        sanitize_file_component(item_id),
        sanitize_file_component(skin_name)
    );
    let zip_path = dl_dir.join(zip_name);

    let url = store_url(store_base_url, &["api", "store", "packages", item_id, skin_name])?;
    let bytes = download_zip_bytes(url)?;
    fs::write(&zip_path, &bytes).map_err(|e| format!("保存 zip 失败: {}", e))?;
    println!(
        "[store] saved skin zip {} ({} bytes)",
        zip_path.display(),
        bytes.len()
    );

    let extract_root = ex_dir.join(format!(
        "{}_skin_{}_{}",
        sanitize_file_component(item_id),
        sanitize_file_component(skin_name),
        now_ms()
    ));
    ensure_dir_empty_or_create(&extract_root)?;
    println!("[store] extracting to {}", extract_root.display());

    let extracted = (|| {
        extract_zip_safe(&zip_path, &extract_root)?;
        let root = find_skins_root(&extract_root)?;
        let skin_src = root.join("skins").join(skin_name);
        if !skin_src.is_dir() {
            return Err(format!("校验失败：皮肤包缺少 skins/{}/", skin_name));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建 skins 目录失败: {}", e))?;
        }
        println!("[store] validation ok, moving to {}", dest.display());
        fs::rename(&skin_src, &dest).map_err(|e| format!("安装失败（移动目录失败）: {}", e))?;
        Ok(())
    })();

    let _ = fs::remove_file(&zip_path);
    let _ = fs::remove_dir_all(&extract_root);

    extracted?;
    mod_loader::request_character(item_id.to_string());
    mod_loader::request_skin(skin_name.to_string());
    println!(
        "[store] skin installed item_id={} skin_name={} dest={}",
        item_id,
        skin_name,
        dest.display()
    );
    Ok(InstallOutcome::Installed)
}
