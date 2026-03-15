mod animation;
mod state_machine;

use animation::loader::load_animation;
use animation::AnimationPlayer;
use state_machine::{Actor, Mover, MoverState, Position, Target};

use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use image::RgbaImage;
use axum::{
    extract::State,
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use webview2::{Controller, Environment, WebView};
use webview2_sys::Color;
use winapi::shared::windef::{HDC as HDC_WINAPI, HWND as HWND_WINAPI, RECT as RECT_WINAPI};
use winapi::um::wingdi::CreateRoundRectRgn as CreateRoundRectRgnWinapi;
use winapi::um::winuser::{
    DrawTextW as DrawTextWWinapi, DT_LEFT, DT_NOPREFIX, DT_TOP, DT_WORDBREAK,
};
use winapi::um::winuser::SetWindowRgn as SetWindowRgnWinapi;
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
        Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetStockObject,
            ReleaseDC, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DEFAULT_GUI_FONT, DIB_RGB_COLORS,
            TRANSPARENT,
        },
        System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED},
        UI::HiDpi::{
            GetDpiForWindow, SetProcessDpiAwarenessContext,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        },
        UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
        UI::Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
            GetSystemMetrics, GetWindowRect, IsWindowVisible, LoadIconW, PeekMessageW,
            PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage, UpdateLayeredWindow,
            CS_HREDRAW, CS_VREDRAW, IDI_APPLICATION, MSG, PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN,
            SW_HIDE, SW_SHOW, ULW_ALPHA, WM_APP, WM_CLOSE, WM_DESTROY, WM_KILLFOCUS,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
};

static TALK_TRIGGERED: AtomicBool = AtomicBool::new(false);
static DRAGGING: AtomicBool = AtomicBool::new(false);
static DRAG_OFFSET_X: AtomicI32 = AtomicI32::new(0);
static DRAG_OFFSET_Y: AtomicI32 = AtomicI32::new(0);
static DRAG_POS_X: AtomicI32 = AtomicI32::new(0);
static DRAG_POS_Y: AtomicI32 = AtomicI32::new(0);
static MENU_REQUESTED: AtomicBool = AtomicBool::new(false);
static MENU_POS_X: AtomicI32 = AtomicI32::new(0);
static MENU_POS_Y: AtomicI32 = AtomicI32::new(0);
static CHAT_REQUESTED: AtomicBool = AtomicBool::new(false);
static CHAT_POS_X: AtomicI32 = AtomicI32::new(0);
static CHAT_POS_Y: AtomicI32 = AtomicI32::new(0);

static BUBBLE_TEXT: Mutex<Option<String>> = Mutex::new(None);

#[derive(Clone)]
struct AiRequest {
    user_text: String,
}

#[derive(Clone)]
struct AiResponse {
    assistant_text: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct BackendConfig {
    bind: String,
    base_url: String,
    model: String,
    system_prompt: String,
    api_key: Option<String>,
}

struct BackendState {
    config: Mutex<BackendConfig>,
    logs: Mutex<Vec<BackendLog>>,
}

#[derive(Clone, serde::Serialize)]
struct BackendLog {
    ts_ms: u64,
    level: String,
    message: String,
}

#[derive(serde::Serialize)]
struct BackendConfigPublic {
    bind: String,
    base_url: String,
    model: String,
    system_prompt: String,
    api_key_set: bool,
}

#[derive(serde::Deserialize)]
struct BackendConfigUpdate {
    bind: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    system_prompt: Option<String>,
    api_key: Option<String>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn push_log(state: &Arc<BackendState>, level: &str, message: String) {
    if let Ok(mut logs) = state.logs.lock() {
        logs.push(BackendLog {
            ts_ms: now_ms(),
            level: level.to_string(),
            message,
        });
        if logs.len() > 300 {
            let drain = logs.len().saturating_sub(300);
            logs.drain(0..drain);
        }
    }
}

const WM_TRAYICON: u32 = WM_APP + 1;
const MENU_W: i32 = 112;
const MENU_H: i32 = 184;
const CHAT_W: i32 = 360;
const CHAT_H: i32 = 220;
const BUBBLE_W: i32 = 180;
const BUBBLE_H: i32 = 64;
const PET_RANGE_MARGIN: i32 = 80;
const MENU_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
    <style>
      :root { color-scheme: dark; }
      html, body {
        width: 112px;
        height: 184px;
        margin: 0;
        padding: 0;
        background: rgba(30, 30, 30, 0.86);
        overflow: hidden;
        font-family: system-ui, "Segoe UI", Arial, sans-serif;
      }
      .menu {
        width: 112px;
        height: 184px;
        margin: 0;
        padding: 10px;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 12px;
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
        display: flex;
        flex-direction: column;
        gap: 6px;
        box-sizing: border-box;
        animation: pop 120ms ease-out;
      }
      @keyframes pop {
        from { transform: translateY(-4px); opacity: 0; }
        to   { transform: translateY(0); opacity: 1; }
      }
      .item {
        height: 34px;
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 0 10px;
        border-radius: 10px;
        cursor: default;
        user-select: none;
        color: rgba(255,255,255,0.92);
        font-size: 13px;
        letter-spacing: 0.2px;
      }
      .item:hover { background: rgba(255,255,255,0.10); }
      .item:active { background: rgba(255,255,255,0.16); }
      .icon {
        width: 18px;
        height: 18px;
        border-radius: 6px;
        background: rgba(255,255,255,0.12);
        display: grid;
        place-items: center;
        font-size: 12px;
      }
      .sep {
        height: 1px;
        background: rgba(255,255,255,0.08);
        margin: 4px 2px;
      }
      .hint {
        margin-top: auto;
        padding: 6px 8px 2px;
        font-size: 11px;
        color: rgba(255,255,255,0.55);
      }
    </style>
  </head>
  <body>
    <div class="menu">
      <div class="item" data-cmd="settings"><div class="icon">⚙</div>设置</div>
      <div class="item" data-cmd="talk"><div class="icon">💬</div>对话</div>
      <div class="item" data-cmd="toggle"><div class="icon">👁</div>隐藏/显示</div>
      <div class="sep"></div>
      <div class="item" data-cmd="exit"><div class="icon">⏻</div>退出</div>
      <div class="hint">Esc 关闭</div>
    </div>
    <script>
      const post = (cmd) => {
        try { window.chrome.webview.postMessage(cmd); } catch (_) {}
      };
      document.querySelectorAll('.item').forEach(el => {
        el.addEventListener('click', () => post(el.dataset.cmd));
      });
      window.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') post('close');
      });
    </script>
  </body>
</html>
"#;

const CHAT_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
    <style>
      :root { color-scheme: dark; }
      html, body {
        width: 360px;
        height: 220px;
        margin: 0;
        padding: 0;
        background: rgba(24, 24, 24, 0.92);
        font-family: system-ui, "Segoe UI", Arial, sans-serif;
        overflow: hidden;
      }
      .wrap {
        width: 360px;
        height: 220px;
        box-sizing: border-box;
        padding: 10px;
        display: flex;
        flex-direction: column;
        gap: 8px;
        border: 1px solid rgba(255,255,255,0.10);
        border-radius: 14px;
        backdrop-filter: blur(12px);
        -webkit-backdrop-filter: blur(12px);
      }
      .title {
        display: flex;
        align-items: center;
        justify-content: space-between;
        color: rgba(255,255,255,0.92);
        font-size: 13px;
        letter-spacing: 0.2px;
        padding: 2px 2px 0;
      }
      .close {
        width: 26px;
        height: 26px;
        border-radius: 10px;
        display: grid;
        place-items: center;
        background: rgba(255,255,255,0.08);
        cursor: default;
        user-select: none;
      }
      .close:hover { background: rgba(255,255,255,0.12); }

      .msgs {
        flex: 1;
        overflow: auto;
        padding: 6px 4px;
        border-radius: 12px;
        border: 1px solid rgba(255,255,255,0.08);
        background: rgba(0,0,0,0.12);
        display: flex;
        flex-direction: column;
        gap: 8px;
      }
      .row { display: flex; }
      .row.user { justify-content: flex-end; }
      .row.ai { justify-content: flex-start; }
      .bubble {
        max-width: 270px;
        padding: 8px 10px;
        border-radius: 12px;
        font-size: 12.5px;
        line-height: 1.35;
        white-space: pre-wrap;
        word-break: break-word;
      }
      .row.user .bubble {
        background: rgba(120, 190, 255, 0.22);
        border: 1px solid rgba(120, 190, 255, 0.28);
        color: rgba(255,255,255,0.94);
      }
      .row.ai .bubble {
        background: rgba(255,255,255,0.08);
        border: 1px solid rgba(255,255,255,0.10);
        color: rgba(255,255,255,0.92);
      }

      .bar {
        display: flex;
        gap: 8px;
        align-items: center;
      }
      input {
        flex: 1;
        height: 34px;
        border-radius: 12px;
        border: 1px solid rgba(255,255,255,0.10);
        background: rgba(0,0,0,0.16);
        color: rgba(255,255,255,0.92);
        padding: 0 10px;
        font-size: 13px;
        outline: none;
      }
      .btn {
        height: 34px;
        padding: 0 12px;
        border-radius: 12px;
        background: rgba(255,255,255,0.10);
        color: rgba(255,255,255,0.92);
        display: flex;
        align-items: center;
        gap: 8px;
        cursor: default;
        user-select: none;
        font-size: 13px;
        white-space: nowrap;
      }
      .btn:hover { background: rgba(255,255,255,0.14); }
    </style>
  </head>
  <body>
    <div class="wrap">
      <div class="title">
        <div>聊天室</div>
        <div class="close" id="close">✕</div>
      </div>
      <div class="msgs" id="msgs"></div>
      <div class="bar">
        <div class="btn" id="mic">🎙 语音</div>
        <input id="input" placeholder="输入内容，Enter 发送，Esc 关闭"/>
        <div class="btn" id="send">发送</div>
      </div>
    </div>
    <script>
      const post = (payload) => {
        try { window.chrome.webview.postMessage(payload); } catch (_) {}
      };

      const msgs = document.getElementById('msgs');
      const input = document.getElementById('input');
      const btnSend = document.getElementById('send');
      const btnMic = document.getElementById('mic');
      const btnClose = document.getElementById('close');

      const addMsg = (role, text) => {
        const row = document.createElement('div');
        row.className = 'row ' + role;
        const b = document.createElement('div');
        b.className = 'bubble';
        b.textContent = text;
        row.appendChild(b);
        msgs.appendChild(row);
        msgs.scrollTop = msgs.scrollHeight;
      };

      if (window.chrome && window.chrome.webview) {
        window.chrome.webview.addEventListener('message', (ev) => {
          const d = ev.data;
          if (!d || !d.type) return;
          if (d.type === 'ai') addMsg('ai', d.text || '');
          if (d.type === 'user') addMsg('user', d.text || '');
        });
      }

      let rec = null;
      let listening = false;
      const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
      if (SR) {
        rec = new SR();
        rec.lang = 'zh-CN';
        rec.interimResults = true;
        rec.continuous = false;
        rec.onresult = (ev) => {
          let text = '';
          for (let i = ev.resultIndex; i < ev.results.length; i++) {
            text += ev.results[i][0].transcript;
          }
          input.value = text.trim();
        };
        rec.onend = () => {
          listening = false;
          btnMic.textContent = '🎙 语音';
        };
      } else {
        btnMic.style.opacity = '0.45';
        btnMic.textContent = '🎙 不支持';
      }

      const send = () => {
        const text = (input.value || '').trim();
        if (!text) return;
        addMsg('user', text);
        post(JSON.stringify({ type: 'send', text }));
        input.value = '';
        input.focus();
      };

      btnSend.addEventListener('click', send);
      btnClose.addEventListener('click', () => post('close'));

      btnMic.addEventListener('click', () => {
        if (!rec) return;
        if (!listening) {
          listening = true;
          btnMic.textContent = '■ 停止';
          try { rec.start(); } catch (_) { listening = false; btnMic.textContent = '🎙 语音'; }
        } else {
          try { rec.stop(); } catch (_) {}
        }
      });

      window.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') post('close');
        if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
      });

      addMsg('ai', '你好，我在这。');
      input.focus();
    </script>
  </body>
</html>
"#;

const BACKEND_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
    <style>
      :root { color-scheme: dark; }
      html, body { margin: 0; padding: 0; background: #0f0f12; font-family: system-ui, "Segoe UI", Arial, sans-serif; color: rgba(255,255,255,0.92); }
      .wrap { max-width: 980px; margin: 24px auto; padding: 0 16px; display: grid; gap: 14px; }
      .card { border: 1px solid rgba(255,255,255,0.10); background: rgba(255,255,255,0.04); border-radius: 14px; padding: 14px; }
      h1 { margin: 0 0 6px; font-size: 18px; }
      .sub { color: rgba(255,255,255,0.62); font-size: 12px; }
      .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
      .row { display: grid; gap: 6px; }
      label { color: rgba(255,255,255,0.72); font-size: 12px; }
      input, textarea { width: 100%; box-sizing: border-box; border-radius: 12px; border: 1px solid rgba(255,255,255,0.12); background: rgba(0,0,0,0.18); color: rgba(255,255,255,0.92); padding: 10px; outline: none; font-size: 13px; }
      textarea { resize: vertical; min-height: 84px; }
      .bar { display: flex; gap: 10px; align-items: center; }
      button { height: 34px; padding: 0 14px; border: 1px solid rgba(255,255,255,0.12); border-radius: 12px; background: rgba(255,255,255,0.10); color: rgba(255,255,255,0.92); cursor: pointer; }
      button:hover { background: rgba(255,255,255,0.14); }
      .mono { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
      .logs { max-height: 320px; overflow: auto; padding: 10px; border-radius: 12px; background: rgba(0,0,0,0.20); border: 1px solid rgba(255,255,255,0.08); }
      .log { padding: 6px 0; border-bottom: 1px solid rgba(255,255,255,0.06); white-space: pre-wrap; word-break: break-word; }
      .tag { display: inline-block; min-width: 44px; text-align: center; border: 1px solid rgba(255,255,255,0.14); border-radius: 999px; padding: 2px 8px; margin-right: 10px; font-size: 11px; color: rgba(255,255,255,0.80); }
      .err { border-color: rgba(255,80,80,0.35); color: rgba(255,180,180,0.95); }
      .ok  { border-color: rgba(100,220,160,0.25); color: rgba(190,255,220,0.95); }
      .hint { color: rgba(255,255,255,0.55); font-size: 12px; }
    </style>
  </head>
  <body>
    <div class="wrap">
      <div class="card">
        <h1>桌宠后台平台</h1>
        <div class="sub">本页面只在本机监听（默认 127.0.0.1）。不要在这里保存或分享你的 Key。</div>
      </div>

      <div class="card">
        <div class="grid">
          <div class="row">
            <label>监听地址（启动时读取，修改后需重启）</label>
            <input id="bind" class="mono" placeholder="127.0.0.1:4317"/>
          </div>
          <div class="row">
            <label>模型</label>
            <input id="model" class="mono" placeholder="gpt-4o-mini"/>
          </div>
          <div class="row">
            <label>Base URL（OpenAI 兼容）</label>
            <input id="baseUrl" class="mono" placeholder="https://api.openai.com/v1"/>
          </div>
          <div class="row">
            <label>API Key（仅内存保存，不回显）</label>
            <input id="apiKey" class="mono" type="password" placeholder="sk-..."/>
          </div>
        </div>
        <div class="row" style="margin-top: 10px;">
          <label>System Prompt</label>
          <textarea id="systemPrompt" placeholder="你是桌宠的聊天助手，回答简洁自然。"></textarea>
        </div>
        <div class="bar" style="margin-top: 10px;">
          <button id="save">保存配置</button>
          <div class="hint" id="cfgHint"></div>
        </div>
      </div>

      <div class="card">
        <div class="bar">
          <input id="testText" placeholder="发一条测试消息给 AI（不会写入代码）"/>
          <button id="testBtn">测试</button>
        </div>
        <div class="hint" id="testHint" style="margin-top: 8px;"></div>
      </div>

      <div class="card">
        <div class="bar" style="justify-content: space-between;">
          <div>运行日志</div>
          <button id="refresh">刷新</button>
        </div>
        <div class="logs" id="logs" style="margin-top: 10px;"></div>
      </div>
    </div>

    <script>
      const el = (id) => document.getElementById(id);
      const bind = el('bind');
      const baseUrl = el('baseUrl');
      const model = el('model');
      const systemPrompt = el('systemPrompt');
      const apiKey = el('apiKey');
      const cfgHint = el('cfgHint');
      const testText = el('testText');
      const testHint = el('testHint');
      const logsEl = el('logs');

      const fmtTime = (ms) => {
        const d = new Date(ms);
        const p2 = (n) => (n < 10 ? '0' + n : '' + n);
        return `${p2(d.getHours())}:${p2(d.getMinutes())}:${p2(d.getSeconds())}`;
      };

      const loadConfig = async () => {
        const r = await fetch('/api/config');
        const j = await r.json();
        bind.value = j.bind || '';
        baseUrl.value = j.base_url || '';
        model.value = j.model || '';
        systemPrompt.value = j.system_prompt || '';
        cfgHint.textContent = j.api_key_set ? 'Key：已设置（不回显）' : 'Key：未设置';
      };

      const saveConfig = async () => {
        cfgHint.textContent = '保存中...';
        const payload = {
          bind: bind.value.trim(),
          base_url: baseUrl.value.trim(),
          model: model.value.trim(),
          system_prompt: systemPrompt.value,
          api_key: apiKey.value.trim()
        };
        const r = await fetch('/api/config', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload) });
        const j = await r.json();
        cfgHint.textContent = j.ok ? '保存成功' : ('保存失败：' + (j.error || 'unknown'));
        apiKey.value = '';
        await loadConfig();
      };

      const loadLogs = async () => {
        const r = await fetch('/api/logs');
        const j = await r.json();
        logsEl.innerHTML = '';
        for (const it of j) {
          const div = document.createElement('div');
          div.className = 'log';
          const tag = document.createElement('span');
          tag.className = 'tag ' + (it.level === 'error' ? 'err' : 'ok');
          tag.textContent = it.level.toUpperCase();
          div.appendChild(tag);
          const t = document.createElement('span');
          t.textContent = `${fmtTime(it.ts_ms)}  ${it.message}`;
          div.appendChild(t);
          logsEl.appendChild(div);
        }
        logsEl.scrollTop = logsEl.scrollHeight;
      };

      const test = async () => {
        testHint.textContent = '请求中...';
        const text = (testText.value || '').trim();
        if (!text) { testHint.textContent = '请输入测试内容'; return; }
        const r = await fetch('/api/test', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ text }) });
        const j = await r.json();
        testHint.textContent = j.ok ? ('AI：' + j.reply) : ('失败：' + (j.error || 'unknown'));
        await loadLogs();
      };

      el('save').addEventListener('click', saveConfig);
      el('refresh').addEventListener('click', loadLogs);
      el('testBtn').addEventListener('click', test);

      loadConfig().then(loadLogs);
      setInterval(loadLogs, 1500);
    </script>
  </body>
</html>
"#;

unsafe fn fill_wide(dst: &mut [u16], s: &str) {
    dst.fill(0);
    for (i, u) in s
        .encode_utf16()
        .take(dst.len().saturating_sub(1))
        .enumerate()
    {
        dst[i] = u;
    }
}

unsafe fn tray_add(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = LoadIconW(None, IDI_APPLICATION).unwrap();
    fill_wide(&mut nid.szTip, "DesktopPet");
    let _ = Shell_NotifyIconW(NIM_ADD, &mut nid);
}

unsafe fn tray_remove(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
}

#[derive(Clone, serde::Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
}

#[derive(serde::Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(serde::Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
}

#[derive(serde::Deserialize)]
struct OpenAiChoiceMessage {
    content: Option<String>,
}

async fn call_ai_openai_with_config(
    cfg: &BackendConfig,
    messages: &[OpenAiMessage],
) -> Result<String, String> {
    let api_key = if let Some(k) = cfg.api_key.as_ref().map(|s| s.trim().to_string()) {
        if k.is_empty() {
            None
        } else {
            Some(k)
        }
    } else {
        None
    }
    .or_else(|| env::var("AI_API_KEY").ok())
    .ok_or_else(|| "未设置 AI_API_KEY（环境变量或后台平台配置）".to_string())?;

    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));

    let client = reqwest::Client::new();
    let req = OpenAiChatRequest {
        model: cfg.model.clone(),
        messages: messages.to_vec(),
    };

    let resp = client
        .post(url)
        .bearer_auth(api_key)
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        return Err(format!("AI 接口返回 {}：{}", status.as_u16(), body));
    }

    let parsed: OpenAiChatResponse =
        serde_json::from_str(&body).map_err(|e| format!("解析响应失败：{e}"))?;
    let content = parsed
        .choices
        .get(0)
        .and_then(|c| c.message.content.clone())
        .unwrap_or_else(|| "".to_string())
        .trim()
        .to_string();

    if content.is_empty() {
        return Err("AI 返回为空".to_string());
    }
    Ok(content)
}

fn spawn_ai_worker(state: Arc<BackendState>, rx: Receiver<AiRequest>, tx: Sender<AiResponse>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let Ok(rt) = rt else {
            return;
        };

        let initial_prompt = state
            .config
            .lock()
            .map(|c| c.system_prompt.clone())
            .unwrap_or_else(|_| "你是桌宠的聊天助手，回答简洁自然。".to_string());
        let mut history: Vec<OpenAiMessage> = vec![OpenAiMessage {
            role: "system".to_string(),
            content: initial_prompt,
        }];

        loop {
            let Ok(req) = rx.recv() else {
                break;
            };
            let cfg = state.config.lock().map(|c| c.clone()).unwrap_or_else(|_| BackendConfig {
                bind: "127.0.0.1:4317".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4o-mini".to_string(),
                system_prompt: "你是桌宠的聊天助手，回答简洁自然。".to_string(),
                api_key: None,
            });
            history[0].content = cfg.system_prompt.clone();
            history.push(OpenAiMessage {
                role: "user".to_string(),
                content: req.user_text,
            });

            let assistant = rt.block_on(call_ai_openai_with_config(&cfg, &history));
            let assistant_text = match assistant {
                Ok(t) => t,
                Err(e) => format!("（AI 错误）{e}"),
            };

            history.push(OpenAiMessage {
                role: "assistant".to_string(),
                content: assistant_text.clone(),
            });

            if history.len() > 21 {
                let mut new_hist = Vec::with_capacity(21);
                new_hist.push(history[0].clone());
                let start = history.len().saturating_sub(20);
                new_hist.extend_from_slice(&history[start..]);
                history = new_hist;
            }

            let _ = tx.send(AiResponse {
                assistant_text,
            });
        }
    });
}

#[derive(serde::Serialize)]
struct ApiOk {
    ok: bool,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
struct BackendTestReq {
    text: String,
}

#[derive(serde::Serialize)]
struct BackendTestResp {
    ok: bool,
    reply: Option<String>,
    error: Option<String>,
}

async fn backend_index() -> Html<&'static str> {
    Html(BACKEND_HTML)
}

async fn backend_get_config(State(state): State<Arc<BackendState>>) -> Json<BackendConfigPublic> {
    let cfg = state.config.lock().map(|c| c.clone()).unwrap_or_else(|_| BackendConfig {
        bind: "127.0.0.1:4317".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        model: "gpt-4o-mini".to_string(),
        system_prompt: "你是桌宠的聊天助手，回答简洁自然。".to_string(),
        api_key: None,
    });
    Json(BackendConfigPublic {
        bind: cfg.bind,
        base_url: cfg.base_url,
        model: cfg.model,
        system_prompt: cfg.system_prompt,
        api_key_set: cfg.api_key.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false)
            || env::var("AI_API_KEY").ok().map(|s| !s.trim().is_empty()).unwrap_or(false),
    })
}

async fn backend_post_config(
    State(state): State<Arc<BackendState>>,
    Json(update): Json<BackendConfigUpdate>,
) -> (StatusCode, Json<ApiOk>) {
    let mut ok = true;
    let mut err: Option<String> = None;
    if let Ok(mut cfg) = state.config.lock() {
        if let Some(v) = update.bind {
            if !v.trim().is_empty() {
                cfg.bind = v.trim().to_string();
            }
        }
        if let Some(v) = update.base_url {
            if !v.trim().is_empty() {
                cfg.base_url = v.trim().to_string();
            }
        }
        if let Some(v) = update.model {
            if !v.trim().is_empty() {
                cfg.model = v.trim().to_string();
            }
        }
        if let Some(v) = update.system_prompt {
            cfg.system_prompt = v;
        }
        if let Some(v) = update.api_key {
            let v = v.trim().to_string();
            if !v.is_empty() {
                cfg.api_key = Some(v);
            }
        }
    } else {
        ok = false;
        err = Some("无法写入配置（锁失败）".to_string());
    }

    if ok {
        push_log(&state, "info", "更新后台配置".to_string());
        (StatusCode::OK, Json(ApiOk { ok: true, error: None }))
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiOk { ok: false, error: err }),
        )
    }
}

async fn backend_get_logs(State(state): State<Arc<BackendState>>) -> Json<Vec<BackendLog>> {
    let logs = state.logs.lock().map(|l| l.clone()).unwrap_or_default();
    Json(logs)
}

async fn backend_post_test(
    State(state): State<Arc<BackendState>>,
    Json(req): Json<BackendTestReq>,
) -> (StatusCode, Json<BackendTestResp>) {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(BackendTestResp {
                ok: false,
                reply: None,
                error: Some("text 不能为空".to_string()),
            }),
        );
    }

    let cfg = state.config.lock().map(|c| c.clone()).unwrap_or_else(|_| BackendConfig {
        bind: "127.0.0.1:4317".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        model: "gpt-4o-mini".to_string(),
        system_prompt: "你是桌宠的聊天助手，回答简洁自然。".to_string(),
        api_key: None,
    });

    push_log(
        &state,
        "info",
        format!("测试请求：model={} base_url={}", cfg.model, cfg.base_url),
    );

    let msgs = vec![
        OpenAiMessage {
            role: "system".to_string(),
            content: cfg.system_prompt.clone(),
        },
        OpenAiMessage {
            role: "user".to_string(),
            content: text,
        },
    ];

    match call_ai_openai_with_config(&cfg, &msgs).await {
        Ok(reply) => {
            push_log(&state, "info", "测试成功".to_string());
            (
                StatusCode::OK,
                Json(BackendTestResp {
                    ok: true,
                    reply: Some(reply),
                    error: None,
                }),
            )
        }
        Err(e) => {
            push_log(&state, "error", format!("测试失败：{e}"));
            (
                StatusCode::OK,
                Json(BackendTestResp {
                    ok: false,
                    reply: None,
                    error: Some(e),
                }),
            )
        }
    }
}

fn spawn_backend_server(state: Arc<BackendState>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build();
        let Ok(rt) = rt else {
            return;
        };

        let bind = state
            .config
            .lock()
            .map(|c| c.bind.clone())
            .unwrap_or_else(|_| "127.0.0.1:4317".to_string());

        let app = Router::new()
            .route("/", get(backend_index))
            .route("/api/config", get(backend_get_config).post(backend_post_config))
            .route("/api/logs", get(backend_get_logs))
            .route("/api/test", post(backend_post_test))
            .with_state(state.clone());

        push_log(&state, "info", format!("后台平台启动：http://{bind}"));

        let _ = rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            axum::serve(listener, app).await?;
            Ok::<(), std::io::Error>(())
        });
    });
}

struct WebMenu {
    hwnd: HWND,
    controller: Rc<RefCell<Option<Controller>>>,
    webview: Rc<RefCell<Option<WebView>>>,
    pending_show: Rc<RefCell<Option<(i32, i32)>>>,
}

struct WebChat {
    hwnd: HWND,
    controller: Rc<RefCell<Option<Controller>>>,
    webview: Rc<RefCell<Option<WebView>>>,
    pending_show: Rc<RefCell<Option<(i32, i32)>>>,
}

impl WebMenu {
    unsafe fn new(hinstance: HINSTANCE) -> Self {
        let class_name = w!("DesktopPetMenuClass");
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(menu_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&wnd_class);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("DesktopPetMenu"),
            WS_POPUP,
            0,
            0,
            MENU_W,
            MENU_H,
            None,
            None,
            hinstance,
            None,
        );

        let rgn = CreateRoundRectRgnWinapi(0, 0, MENU_W, MENU_H, 24, 24);
        let _ = SetWindowRgnWinapi(hwnd.0 as HWND_WINAPI, rgn, 1);

        Self {
            hwnd,
            controller: Rc::new(RefCell::new(None)),
            webview: Rc::new(RefCell::new(None)),
            pending_show: Rc::new(RefCell::new(None)),
        }
    }

    unsafe fn init(&mut self, pet_hwnd: HWND) {
        let menu_hwnd_windows = self.hwnd;
        let menu_hwnd: HWND_WINAPI = menu_hwnd_windows.0 as HWND_WINAPI;
        let controller_cell_env = self.controller.clone();
        let webview_cell_env = self.webview.clone();
        let pending_show_env = self.pending_show.clone();

        let _ = Environment::builder().build(move |env| {
            let env = env?;
            let controller_cell_controller = controller_cell_env.clone();
            let webview_cell_controller = webview_cell_env.clone();
            let pending_show_controller = pending_show_env.clone();
            let pet_hwnd2 = pet_hwnd;
            let menu_hwnd2 = menu_hwnd_windows;

            env.create_controller(menu_hwnd, move |c| {
                let controller = c?;
                let rect = RECT_WINAPI {
                    left: 0,
                    top: 0,
                    right: MENU_W,
                    bottom: MENU_H,
                };
                controller.put_bounds(rect)?;
                controller.put_is_visible(true)?;
                if let Ok(c2) = controller.get_controller2() {
                    let _ = c2.put_default_background_color(Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 0,
                    });
                }

                let webview = controller.get_webview()?;
                webview.navigate_to_string(MENU_HTML)?;

                webview.add_web_message_received(move |_w, msg| {
                    let msg = msg.try_get_web_message_as_string()?;
                    match msg.as_str() {
                        "talk" => {
                            let mut pt = POINT::default();
                            let _ = GetCursorPos(&mut pt);
                            CHAT_POS_X.store(pt.x, Ordering::Relaxed);
                            CHAT_POS_Y.store(pt.y, Ordering::Relaxed);
                            CHAT_REQUESTED.store(true, Ordering::Relaxed);
                        }
                        "toggle" => {
                            if IsWindowVisible(pet_hwnd2).as_bool() {
                                let _ = ShowWindow(pet_hwnd2, SW_HIDE);
                            } else {
                                let _ = ShowWindow(pet_hwnd2, SW_SHOW);
                            }
                        }
                        "exit" => PostQuitMessage(0),
                        "settings" => {}
                        "close" => {}
                        _ => {}
                    }
                    let _ = ShowWindow(menu_hwnd2, SW_HIDE);
                    Ok(())
                })?;

                *controller_cell_controller.borrow_mut() = Some(controller.clone());
                *webview_cell_controller.borrow_mut() = Some(webview.clone());

                if let Some((x, y)) = pending_show_controller.borrow_mut().take() {
                    let screen_w = GetSystemMetrics(SM_CXSCREEN);
                    let screen_h = GetSystemMetrics(SM_CYSCREEN);
                    let clamped_x = x.clamp(0, screen_w - MENU_W);
                    let clamped_y = y.clamp(0, screen_h - MENU_H);
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        menu_hwnd2,
                        HWND(0),
                        clamped_x,
                        clamped_y,
                        MENU_W,
                        MENU_H,
                        windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                            | windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
                    );
                    let _ = ShowWindow(menu_hwnd2, SW_SHOW);
                }

                Ok(())
            })
        });
    }

    unsafe fn show_at(&mut self, x: i32, y: i32) {
        if self.controller.borrow().is_none() {
            *self.pending_show.borrow_mut() = Some((x, y));
            return;
        }
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let clamped_x = x.clamp(0, screen_w - MENU_W);
        let clamped_y = y.clamp(0, screen_h - MENU_H);
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            self.hwnd,
            HWND(0),
            clamped_x,
            clamped_y,
            MENU_W,
            MENU_H,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                | windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(self.hwnd, SW_SHOW);
    }
}

impl WebChat {
    unsafe fn new(hinstance: HINSTANCE) -> Self {
        let class_name = w!("DesktopPetChatClass");
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(chat_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&wnd_class);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("DesktopPetChat"),
            WS_POPUP,
            0,
            0,
            CHAT_W,
            CHAT_H,
            None,
            None,
            hinstance,
            None,
        );

        let rgn = CreateRoundRectRgnWinapi(0, 0, CHAT_W, CHAT_H, 28, 28);
        let _ = SetWindowRgnWinapi(hwnd.0 as HWND_WINAPI, rgn, 1);

        Self {
            hwnd,
            controller: Rc::new(RefCell::new(None)),
            webview: Rc::new(RefCell::new(None)),
            pending_show: Rc::new(RefCell::new(None)),
        }
    }

    unsafe fn init(&mut self, ai_tx: Sender<AiRequest>) {
        let chat_hwnd_windows = self.hwnd;
        let chat_hwnd: HWND_WINAPI = chat_hwnd_windows.0 as HWND_WINAPI;
        let controller_cell_env = self.controller.clone();
        let webview_cell_env = self.webview.clone();
        let pending_show_env = self.pending_show.clone();

        let _ = Environment::builder().build(move |env| {
            let env = env?;
            let controller_cell_controller = controller_cell_env.clone();
            let webview_cell_controller = webview_cell_env.clone();
            let pending_show_controller = pending_show_env.clone();
            let chat_hwnd2 = chat_hwnd_windows;

            env.create_controller(chat_hwnd, move |c| {
                let controller = c?;
                let rect = RECT_WINAPI {
                    left: 0,
                    top: 0,
                    right: CHAT_W,
                    bottom: CHAT_H,
                };
                controller.put_bounds(rect)?;
                controller.put_is_visible(true)?;
                if let Ok(c2) = controller.get_controller2() {
                    let _ = c2.put_default_background_color(Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 0,
                    });
                }

                let webview = controller.get_webview()?;
                webview.navigate_to_string(CHAT_HTML)?;

                let ai_tx2 = ai_tx.clone();
                webview.add_web_message_received(move |_w, msg| {
                    let msg = msg.try_get_web_message_as_string()?;
                    if msg == "close" {
                        let _ = ShowWindow(chat_hwnd2, SW_HIDE);
                        return Ok(());
                    }

                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg) {
                        if v.get("type").and_then(|t| t.as_str()) == Some("send") {
                            let text = v
                                .get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            if !text.is_empty() {
                                if let Ok(mut g) = BUBBLE_TEXT.lock() {
                                    *g = Some(text.clone());
                                }
                                TALK_TRIGGERED.store(true, Ordering::Relaxed);
                                let _ = ai_tx2.send(AiRequest { user_text: text });
                            }
                        }
                    }
                    Ok(())
                })?;

                *controller_cell_controller.borrow_mut() = Some(controller.clone());
                *webview_cell_controller.borrow_mut() = Some(webview.clone());

                if let Some((x, y)) = pending_show_controller.borrow_mut().take() {
                    let screen_x =
                        GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN);
                    let screen_y =
                        GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN);
                    let screen_w =
                        GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN);
                    let screen_h =
                        GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN);
                    let max_x = screen_x + (screen_w - CHAT_W).max(0);
                    let max_y = screen_y + (screen_h - CHAT_H).max(0);
                    let clamped_x = x.clamp(screen_x, max_x);
                    let clamped_y = y.clamp(screen_y, max_y);
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        chat_hwnd2,
                        HWND(0),
                        clamped_x,
                        clamped_y,
                        CHAT_W,
                        CHAT_H,
                        windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                            | windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
                    );
                    let _ = ShowWindow(chat_hwnd2, SW_SHOW);
                }

                Ok(())
            })
        });
    }

    unsafe fn show_at(&mut self, x: i32, y: i32) {
        if self.controller.borrow().is_none() {
            *self.pending_show.borrow_mut() = Some((x, y));
            return;
        }

        let screen_x =
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN);
        let screen_y =
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN);
        let screen_w =
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN);
        let screen_h =
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN);
        let max_x = screen_x + (screen_w - CHAT_W).max(0);
        let max_y = screen_y + (screen_h - CHAT_H).max(0);

        let clamped_x = x.clamp(screen_x, max_x);
        let clamped_y = y.clamp(screen_y, max_y);
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            self.hwnd,
            HWND(0),
            clamped_x,
            clamped_y,
            CHAT_W,
            CHAT_H,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER
                | windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(self.hwnd, SW_SHOW);
    }
}

extern "system" fn menu_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_KILLFOCUS => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn chat_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
fn premul_u8(c: u8, a: u8) -> u8 {
    ((c as u16 * a as u16 + 127) / 255) as u8
}

fn inside_round_rect(x: i32, y: i32, w: i32, h: i32, r: i32) -> bool {
    if x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    if r <= 0 {
        return true;
    }

    let r2 = r * r;

    if x >= r && x < (w - r) {
        return true;
    }
    if y >= r && y < (h - r) {
        return true;
    }

    let (cx, cy) = if x < r && y < r {
        (r, r)
    } else if x >= (w - r) && y < r {
        (w - r - 1, r)
    } else if x < r && y >= (h - r) {
        (r, h - r - 1)
    } else {
        (w - r - 1, h - r - 1)
    };

    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r2
}

fn draw_chat_bubble_bg(bgra: &mut [u8], w: i32, h: i32, tail_x: i32) {
    bgra.fill(0);

    let tail_h = 10;
    let body_h = h - tail_h;
    let radius = 12;
    let pad = 1;

    let bg = (30_u8, 30_u8, 30_u8, 220_u8);
    let border = (255_u8, 255_u8, 255_u8, 40_u8);

    for y in 0..body_h {
        for x in 0..w {
            let inside_outer = inside_round_rect(x, y, w, body_h, radius);
            if !inside_outer {
                continue;
            }

            let inside_inner = inside_round_rect(x - pad, y - pad, w - pad * 2, body_h - pad * 2, radius - pad);
            let (r, g, b, a) = if inside_inner { bg } else { border };

            let idx = ((y * w + x) * 4) as usize;
            bgra[idx] = premul_u8(b, a);
            bgra[idx + 1] = premul_u8(g, a);
            bgra[idx + 2] = premul_u8(r, a);
            bgra[idx + 3] = a;
        }
    }

    let tail_center_x = tail_x.clamp(16, w - 16);
    let tail_top = body_h - 1;
    let tail_bottom = h - 1;
    for y in tail_top..=tail_bottom {
        let t = (y - tail_top) as f32 / tail_h as f32;
        let half_w = (9.0 * (1.0 - t)).max(0.0);
        let x0 = (tail_center_x as f32 - half_w).floor() as i32;
        let x1 = (tail_center_x as f32 + half_w).ceil() as i32;
        for x in x0..=x1 {
            if x < 0 || x >= w {
                continue;
            }
            let idx = ((y * w + x) * 4) as usize;
            bgra[idx] = premul_u8(bg.2, bg.3);
            bgra[idx + 1] = premul_u8(bg.1, bg.3);
            bgra[idx + 2] = premul_u8(bg.0, bg.3);
            bgra[idx + 3] = bg.3;
        }
    }
}

struct ChatBubble {
    hwnd: HWND,
    w: i32,
    h: i32,
    mem_dc: windows::Win32::Graphics::Gdi::HDC,
    dib: windows::Win32::Graphics::Gdi::HBITMAP,
    bits: *mut u8,
    visible: bool,
}

impl ChatBubble {
    unsafe fn new(hinstance: HINSTANCE, screen_dc: windows::Win32::Graphics::Gdi::HDC) -> Self {
        let class_name = w!("DesktopPetBubbleClass");
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(bubble_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&wnd_class);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            class_name,
            w!("DesktopPetBubble"),
            WS_POPUP,
            0,
            0,
            BUBBLE_W,
            BUBBLE_H,
            None,
            None,
            hinstance,
            None,
        );

        let mem_dc = CreateCompatibleDC(screen_dc);

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: BUBBLE_W,
                biHeight: -BUBBLE_H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut core::ffi::c_void = null_mut();
        let dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        let _ = SelectObject(mem_dc, dib);

        Self {
            hwnd,
            w: BUBBLE_W,
            h: BUBBLE_H,
            mem_dc,
            dib,
            bits: bits as *mut u8,
            visible: false,
        }
    }

    unsafe fn set_visible(&mut self, show: bool) {
        if show && !self.visible {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            self.visible = true;
        } else if !show && self.visible {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            self.visible = false;
        }
    }

    unsafe fn render_and_present(
        &mut self,
        screen_dc: windows::Win32::Graphics::Gdi::HDC,
        dst_x: i32,
        dst_y: i32,
        tail_x: i32,
        text: &str,
    ) {
        let len = (self.w * self.h * 4) as usize;
        let buf = std::slice::from_raw_parts_mut(self.bits, len);
        draw_chat_bubble_bg(buf, self.w, self.h, tail_x);

        let mut before = vec![0_u8; len];
        before.copy_from_slice(buf);

        let old_font = SelectObject(self.mem_dc, GetStockObject(DEFAULT_GUI_FONT));
        let _ = SetBkMode(self.mem_dc, TRANSPARENT);
        let _ = SetTextColor(self.mem_dc, COLORREF(0x00FFFFFF));

        let mut text_rect = RECT_WINAPI {
            left: 12,
            top: 8,
            right: self.w - 12,
            bottom: self.h - 14,
        };

        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        let _ = DrawTextWWinapi(
            self.mem_dc.0 as HDC_WINAPI,
            wide.as_ptr(),
            -1,
            &mut text_rect,
            (DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX) as u32,
        );
        let _ = SelectObject(self.mem_dc, old_font);

        for i in (0..len).step_by(4) {
            if before[i] != buf[i]
                || before[i + 1] != buf[i + 1]
                || before[i + 2] != buf[i + 2]
                || before[i + 3] != buf[i + 3]
            {
                buf[i + 3] = 255;
            }
        }

        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let dst_pt = POINT { x: dst_x, y: dst_y };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        let _ = UpdateLayeredWindow(
            self.hwnd,
            screen_dc,
            Some(&dst_pt),
            Some(&size),
            self.mem_dc,
            Some(&src_pt),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
    }
}

extern "system" fn bubble_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_LBUTTONDOWN => {
                let _ = SetCapture(hwnd);

                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);

                let mut rect = windows::Win32::Foundation::RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);

                DRAG_OFFSET_X.store(pt.x - rect.left, Ordering::Relaxed);
                DRAG_OFFSET_Y.store(pt.y - rect.top, Ordering::Relaxed);
                DRAG_POS_X.store(rect.left, Ordering::Relaxed);
                DRAG_POS_Y.store(rect.top, Ordering::Relaxed);
                DRAGGING.store(true, Ordering::Relaxed);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if DRAGGING.load(Ordering::Relaxed) {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let ox = DRAG_OFFSET_X.load(Ordering::Relaxed);
                    let oy = DRAG_OFFSET_Y.load(Ordering::Relaxed);
                    DRAG_POS_X.store(pt.x - ox, Ordering::Relaxed);
                    DRAG_POS_Y.store(pt.y - oy, Ordering::Relaxed);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                DRAGGING.store(false, Ordering::Relaxed);
                let _ = ReleaseCapture();
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                MENU_POS_X.store(pt.x, Ordering::Relaxed);
                MENU_POS_Y.store(pt.y, Ordering::Relaxed);
                MENU_REQUESTED.store(true, Ordering::Relaxed);
                LRESULT(0)
            }
            WM_TRAYICON => {
                let mouse_msg = lparam.0 as u32;
                if mouse_msg == WM_RBUTTONUP {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    MENU_POS_X.store(pt.x, Ordering::Relaxed);
                    MENU_POS_Y.store(pt.y, Ordering::Relaxed);
                    MENU_REQUESTED.store(true, Ordering::Relaxed);
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                tray_remove(hwnd);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn alpha_over(dst: &mut [u8; 4], src: &[u8; 4]) {
    let sa = src[3] as u32;
    if sa == 0 {
        return;
    }
    if sa == 255 {
        *dst = *src;
        return;
    }
    let inv = 255 - sa;
    dst[0] = ((src[0] as u32 * sa + dst[0] as u32 * inv) / 255) as u8;
    dst[1] = ((src[1] as u32 * sa + dst[1] as u32 * inv) / 255) as u8;
    dst[2] = ((src[2] as u32 * sa + dst[2] as u32 * inv) / 255) as u8;
    dst[3] = (sa + (dst[3] as u32 * inv) / 255) as u8;
}

fn to_premultiplied_bgra(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height() * 4) as usize);
    for p in img.pixels() {
        let a = p[3] as u32;
        let r = (p[0] as u32 * a + 127) / 255;
        let g = (p[1] as u32 * a + 127) / 255;
        let b = (p[2] as u32 * a + 127) / 255;
        out.push(b as u8);
        out.push(g as u8);
        out.push(r as u8);
        out.push(a as u8);
    }
    out
}

fn load_cached_rgba(cache: &mut HashMap<String, RgbaImage>, path: &str) -> RgbaImage {
    if let Some(img) = cache.get(path) {
        return img.clone();
    }
    let img = image::open(path).expect("无法打开 PNG").to_rgba8();
    cache.insert(path.to_string(), img.clone());
    img
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let backend_state = Arc::new(BackendState {
        config: Mutex::new(BackendConfig {
            bind: env::var("BACKEND_BIND").unwrap_or_else(|_| "127.0.0.1:4317".to_string()),
            base_url: env::var("AI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            model: env::var("AI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            system_prompt: env::var("AI_SYSTEM")
                .unwrap_or_else(|_| "你是桌宠的聊天助手，回答简洁自然。".to_string()),
            api_key: env::var("AI_API_KEY").ok(),
        }),
        logs: Mutex::new(Vec::new()),
    });
    spawn_backend_server(backend_state.clone());

    let animations_dir: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "assets",
        "debug_pet",
        "animations",
    ]
    .into_iter()
    .collect();

    let walk_dir = animations_dir.join("walk");
    let idle_dir = animations_dir.join("idle");
    let blink_dir = animations_dir.join("blink");
    let talk_dir = animations_dir.join("talk");

    let walk = AnimationPlayer::new(load_animation(walk_dir.to_str().unwrap(), 12, true));
    let idle = AnimationPlayer::new(load_animation(idle_dir.to_str().unwrap(), 6, true));
    let blink = AnimationPlayer::new(load_animation(blink_dir.to_str().unwrap(), 15, true));
    let talk = AnimationPlayer::new(load_animation(talk_dir.to_str().unwrap(), 12, true));

    let mut pet_actor = Actor {
        walk,
        idle,
        blink,
        talk,
        blink_timer_ms: 0,
        talk_timer_ms: 0,
        talk_cooldown_ms: 0,
    };

    let hinstance: HINSTANCE = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap()
            .into()
    };
    let class_name = w!("DesktopPetClass");
    let wnd_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: class_name,
        ..Default::default()
    };
    unsafe {
        let _ = RegisterClassW(&wnd_class);
    }

    let base = pet_actor.walk.get_current_frame();
    let base_img = image::open(&base.path).expect("无法打开 PNG").to_rgba8();
    let w = base_img.width() as i32;
    let h = base_img.height() as i32;

    let base_speed_dip = 24.0_f32;
    let mut pet_mover = Mover {
        pos: Position { x: 100.0, y: 100.0 },
        target: Target { x: 400.0, y: 300.0 },
        speed: base_speed_dip,
        state: MoverState::Moving,
        bounds_x: unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN) as f32 }
            - PET_RANGE_MARGIN as f32,
        bounds_y: unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN) as f32 }
            - PET_RANGE_MARGIN as f32,
        bounds_w: unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN) as f32 }
            + (PET_RANGE_MARGIN * 2) as f32,
        bounds_h: unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN) as f32 }
            + (PET_RANGE_MARGIN * 2) as f32,
        sprite_w: w as f32,
        sprite_h: h as f32,
    };

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("DesktopPet"),
            WS_POPUP,
            pet_mover.pos.x as i32,
            pet_mover.pos.y as i32,
            w,
            h,
            None,
            None,
            hinstance,
            None,
        )
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    unsafe {
        tray_add(hwnd);
    }

    let mut web_menu = unsafe { WebMenu::new(hinstance) };
    unsafe {
        web_menu.init(hwnd);
    }

    let (ai_req_tx, ai_req_rx) = channel::<AiRequest>();
    let (ai_resp_tx, ai_resp_rx) = channel::<AiResponse>();
    spawn_ai_worker(backend_state.clone(), ai_req_rx, ai_resp_tx);

    let mut web_chat = unsafe { WebChat::new(hinstance) };
    unsafe {
        web_chat.init(ai_req_tx.clone());
    }

    let screen_dc = unsafe { GetDC(HWND(0)) };
    let mem_dc = unsafe { CreateCompatibleDC(screen_dc) };

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = null_mut();
    let dib =
        unsafe { CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }.unwrap();
    unsafe {
        let _ = SelectObject(mem_dc, dib);
    }

    let mut chat_bubble = unsafe { ChatBubble::new(hinstance, screen_dc) };

    let mut cache: HashMap<String, RgbaImage> = HashMap::new();
    let mut msg = MSG::default();
    let mut last_tick = Instant::now();
    let mut prev_dragging = false;

    'outer: loop {
        unsafe {
            let pump_start = Instant::now();
            loop {
                if !PeekMessageW(&mut msg, HWND(0), 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    break 'outer;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if pump_start.elapsed() >= std::time::Duration::from_millis(2) {
                    break;
                }
            }
        }

        if MENU_REQUESTED.swap(false, Ordering::Relaxed) {
            let x = MENU_POS_X.load(Ordering::Relaxed);
            let y = MENU_POS_Y.load(Ordering::Relaxed);
            unsafe {
                web_menu.show_at(x, y);
            }
        }

        if CHAT_REQUESTED.swap(false, Ordering::Relaxed) {
            let x = CHAT_POS_X.load(Ordering::Relaxed);
            let y = CHAT_POS_Y.load(Ordering::Relaxed);
            unsafe {
                web_chat.show_at(x, y);
            }
        }

        let menu_visible = unsafe { IsWindowVisible(web_menu.hwnd).as_bool() };
        let chat_visible = unsafe { IsWindowVisible(web_chat.hwnd).as_bool() };
        let ui_visible = menu_visible || chat_visible;

        let now = Instant::now();
        let delta = now.saturating_duration_since(last_tick);
        last_tick = now;

        let dt_ms = delta.as_millis().min(u128::from(u32::MAX)) as u32;
        let mut talk_trigger = TALK_TRIGGERED.swap(false, Ordering::Relaxed);

        while let Ok(resp) = ai_resp_rx.try_recv() {
            let reply = resp.assistant_text;
            if let Ok(mut g) = BUBBLE_TEXT.lock() {
                *g = Some(reply.clone());
            }
            talk_trigger = true;

            if let Some(wv) = web_chat.webview.borrow().as_ref() {
                let payload = serde_json::json!({ "type": "ai", "text": reply });
                let json_str = payload.to_string();
                let _ = wv.post_web_message_as_json(&json_str);
            }
        }
        let will_auto_talk = !talk_trigger
            && pet_actor
                .talk_cooldown_ms
                .saturating_add(dt_ms)
                >= 10_000;
        if will_auto_talk {
            if let Ok(mut g) = BUBBLE_TEXT.lock() {
                *g = Some("你好".to_string());
            }
        } else if talk_trigger {
            if let Ok(mut g) = BUBBLE_TEXT.lock() {
                if g.is_none() {
                    *g = Some("你好".to_string());
                }
            }
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
        pet_mover.speed = base_speed_dip * (dpi / 96.0);
        pet_mover.bounds_x =
            unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN) as f32 }
                - PET_RANGE_MARGIN as f32;
        pet_mover.bounds_y =
            unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN) as f32 }
                - PET_RANGE_MARGIN as f32;
        pet_mover.bounds_w =
            unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN) as f32 }
                + (PET_RANGE_MARGIN * 2) as f32;
        pet_mover.bounds_h =
            unsafe { GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN) as f32 }
                + (PET_RANGE_MARGIN * 2) as f32;

        let dragging = DRAGGING.load(Ordering::Relaxed);
        if dragging {
            pet_mover.pos.x = DRAG_POS_X.load(Ordering::Relaxed) as f32;
            pet_mover.pos.y = DRAG_POS_Y.load(Ordering::Relaxed) as f32;
        } else if ui_visible {
            if prev_dragging {
                pet_mover.target = Target {
                    x: pet_mover.pos.x,
                    y: pet_mover.pos.y,
                };
                pet_mover.state = MoverState::Resting { timer_ms: 0 };
            }
        } else {
            if prev_dragging {
                pet_mover.target = Target {
                    x: pet_mover.pos.x,
                    y: pet_mover.pos.y,
                };
                pet_mover.state = MoverState::Resting { timer_ms: 0 };
            }
            pet_mover.update(delta);
        }
        prev_dragging = dragging;

        let rest_state = MoverState::Resting { timer_ms: 0 };
        let mover_state = if dragging || ui_visible {
            &rest_state
        } else {
            &pet_mover.state
        };

        pet_actor.update(mover_state, delta, talk_trigger);

        let bubble_show = pet_actor.talk_timer_ms > 0;
        if bubble_show {
            let screen_x = unsafe {
                GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN)
            };
            let screen_y = unsafe {
                GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN)
            };
            let screen_w = unsafe {
                GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN)
            };
            let screen_h = unsafe {
                GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN)
            };

            let pet_x = pet_mover.pos.x as i32;
            let pet_y = pet_mover.pos.y as i32;
            let pet_center_x = pet_x + w / 2;

            let max_x = screen_x + (screen_w - BUBBLE_W).max(0);
            let max_y = screen_y + (screen_h - BUBBLE_H).max(0);

            let preferred_x = pet_center_x - BUBBLE_W / 2;
            let bubble_x = if preferred_x < screen_x {
                (pet_x + w + 8).clamp(screen_x, max_x)
            } else if preferred_x > max_x {
                (pet_x - BUBBLE_W - 8).clamp(screen_x, max_x)
            } else {
                preferred_x
            };

            let preferred_y = pet_y - BUBBLE_H - 8;
            let bubble_y = if preferred_y < screen_y {
                (pet_y + h + 8).clamp(screen_y, max_y)
            } else {
                preferred_y.clamp(screen_y, max_y)
            };

            let tail_x = (pet_center_x - bubble_x).clamp(16, BUBBLE_W - 16);

            let bubble_text = BUBBLE_TEXT
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .unwrap_or_else(|| "你好".to_string());

            unsafe {
                chat_bubble.render_and_present(screen_dc, bubble_x, bubble_y, tail_x, &bubble_text);
                chat_bubble.set_visible(true);
            }
        } else {
            unsafe {
                chat_bubble.set_visible(false);
            }
        }

        let base_frame = match mover_state {
            MoverState::Moving => pet_actor.walk.get_current_frame(),
            MoverState::Resting { .. } => pet_actor.idle.get_current_frame(),
        };

        let mut composed = load_cached_rgba(&mut cache, &base_frame.path);

        if pet_actor.blink_timer_ms > 0 {
            let blink_frame = pet_actor.blink.get_current_frame();
            let blink_img = load_cached_rgba(&mut cache, &blink_frame.path);

            let min_w = composed.width().min(blink_img.width());
            let min_h = composed.height().min(blink_img.height());
            for y in 0..min_h {
                for x in 0..min_w {
                    let mut d = composed.get_pixel(x, y).0;
                    let s = blink_img.get_pixel(x, y).0;
                    alpha_over(&mut d, &s);
                    composed.get_pixel_mut(x, y).0 = d;
                }
            }
        }

        if pet_actor.talk_timer_ms > 0 {
            let talk_frame = pet_actor.talk.get_current_frame();
            let talk_img = load_cached_rgba(&mut cache, &talk_frame.path);
            let min_w = composed.width().min(talk_img.width());
            let min_h = composed.height().min(talk_img.height());
            for y in 0..min_h {
                for x in 0..min_w {
                    let mut d = composed.get_pixel(x, y).0;
                    let s = talk_img.get_pixel(x, y).0;
                    alpha_over(&mut d, &s);
                    composed.get_pixel_mut(x, y).0 = d;
                }
            }
        }

        let bgra = to_premultiplied_bgra(&composed);
        unsafe {
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        }

        let size = SIZE { cx: w, cy: h };
        let src_pt = POINT { x: 0, y: 0 };
        let dst_pt = POINT {
            x: pet_mover.pos.x as i32,
            y: pet_mover.pos.y as i32,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                screen_dc,
                Some(&dst_pt),
                Some(&size),
                mem_dc,
                Some(&src_pt),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }

        let overlays_active = pet_actor.blink_timer_ms > 0 || pet_actor.talk_timer_ms > 0;
        let delay = if ui_visible {
            std::time::Duration::from_millis(1000 / 60)
        } else if dragging {
            std::time::Duration::from_millis(1000 / 60)
        } else if overlays_active {
            std::time::Duration::from_millis(1000 / 15)
        } else {
            match mover_state {
                MoverState::Moving => std::time::Duration::from_millis(1000 / 12),
                MoverState::Resting { .. } => std::time::Duration::from_millis(1000 / 6),
            }
        };

        std::thread::sleep(delay);
    }

    unsafe {
        chat_bubble.set_visible(false);
        let _ = DestroyWindow(chat_bubble.hwnd);
        let _ = DeleteObject(chat_bubble.dib);
        let _ = DeleteDC(chat_bubble.mem_dc);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(HWND(0), screen_dc);
    }
}
