mod ai_persona;
mod animation;
mod character;
mod character_store;
mod interaction;
mod scene;
#[allow(dead_code, unused_imports)]
mod ipc;
mod mod_loader;
mod physics;
mod pet_level_sync;
mod pet_stats;
mod phrases;
mod realworld;
mod render;
mod renderer;
mod scripting;
mod state_machine;
mod ws_auto_talk;

use animation::animation::Animation;
use animation::frame::Frame;
use animation::loader::load_animation;
use animation::AnimationPlayer;
use ipc::{global_client, IpcMessage};
use pet_level_sync::{fetch_store_user_id, BackendPetLevelResp, BackendPetLevelUpdate};
use pet_stats::PET_STATS;
use phrases::pick_event_phrase;
use realworld::REALWORLD;
use physics::{Bounds as PhysicsBounds, PhysicsBody};
use scene::Scene;
use scripting::{AiRequest, AiResponse};
use state_machine::{Actor, ActorAssets, ActorState, Facing, Mover, MoverState, Position, Target};
use ws_auto_talk::{spawn_auto_talk_ws_listener, AutoTalkWsMsg, WsClientCmd};

use std::collections::VecDeque;
use std::path::PathBuf;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, TryRecvError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use image::GenericImageView;
use rand::Rng;
use windows::{
    core::w,
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED},
        UI::HiDpi::{
            GetDpiForWindow, SetProcessDpiAwarenessContext,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        },
        UI::Shell::{
            DragAcceptFiles, DragFinish, DragQueryFileW, Shell_NotifyIconW, HDROP,
            NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetSystemMetrics,
            IsWindowVisible, LoadIconW, PeekMessageW,
            PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
            IDI_APPLICATION, MSG, PM_REMOVE, SW_SHOW, WM_CLOSE,
            WM_DESTROY, WM_DROPFILES, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
            WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
        },
    },
};

static BUBBLE_TEXT: Mutex<Option<String>> = Mutex::new(None);
static CHARACTER_TEXTS: Mutex<character::CharacterTexts> = Mutex::new(character::CharacterTexts {
    hello: None,
    snack_received: None,
    event_phrases: None,
    pet_clicked_phrases: None,
    feed_phrases: None,
    level_up_template: None,
});

// reserved

const WM_TRAYICON: u32 = interaction::WM_TRAYICON;
const BUBBLE_W: i32 = 180;
const BUBBLE_H: i32 = 64;
const PET_RANGE_MARGIN: i32 = 80;
const MENU_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
    <style>
      :root {
        color-scheme: dark;
        --fg: rgba(255,255,255,0.92);
        --fg2: rgba(255,255,255,0.62);
        --fg3: rgba(255,255,255,0.46);
        --card: rgba(255,255,255,0.045);
        --card2: rgba(255,255,255,0.065);
        --stroke: rgba(255,255,255,0.10);
        --stroke2: rgba(255,255,255,0.14);
        --accent: rgba(132, 94, 247, 0.92);
        --accent2: rgba(76, 197, 255, 0.92);
      }
      html, body {
        width: 196px;
        height: 360px;
        margin: 0;
        padding: 0;
        background: rgba(8, 8, 12, 0.92);
        overflow: hidden;
        font-family: system-ui, "Segoe UI", Arial, sans-serif;
      }
      .shell {
        width: 196px;
        height: 360px;
        padding: 10px;
        box-sizing: border-box;
        background:
          radial-gradient(120px 90px at 30px 18px, rgba(132, 94, 247, 0.30), rgba(0,0,0,0) 60%),
          radial-gradient(140px 90px at 170px 30px, rgba(76, 197, 255, 0.22), rgba(0,0,0,0) 60%),
          linear-gradient(180deg, rgba(255,255,255,0.05), rgba(255,255,255,0.02));
        border: 1px solid rgba(255, 255, 255, 0.12);
        border-radius: 14px;
        backdrop-filter: blur(16px);
        -webkit-backdrop-filter: blur(16px);
        box-shadow:
          0 18px 58px rgba(0,0,0,0.58),
          0 1px 0 rgba(255,255,255,0.06) inset;
        display: flex;
        flex-direction: column;
        gap: 10px;
        animation: pop 120ms ease-out;
        overflow: hidden;
        cursor: move;
      }
      #content {
        flex: 1;
        overflow: auto;
        padding-bottom: 8px;
        scrollbar-color: rgba(255,255,255,0.18) rgba(0,0,0,0);
        scrollbar-width: thin;
      }
      #content::-webkit-scrollbar { width: 8px; }
      #content::-webkit-scrollbar-thumb {
        background: rgba(255,255,255,0.16);
        border-radius: 999px;
        border: 2px solid rgba(0,0,0,0);
        background-clip: padding-box;
      }
      #content::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,0.22); }
      #content::-webkit-scrollbar-track { background: rgba(0,0,0,0); }
      }
      @keyframes pop {
        from { transform: translateY(-6px) scale(0.985); opacity: 0; }
        to   { transform: translateY(0) scale(1); opacity: 1; }
      }
      .title {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 8px 10px;
        border-radius: 12px;
        background: rgba(255,255,255,0.05);
        border: 1px solid rgba(255,255,255,0.10);
      }
      .brand {
        display: flex;
        flex-direction: column;
        gap: 2px;
        line-height: 1.1;
      }
      .brand b {
        font-size: 13px;
        color: var(--fg);
        letter-spacing: 0.2px;
      }
      .brand span {
        font-size: 11px;
        color: var(--fg2);
      }
      .badge {
        font-size: 11px;
        padding: 5px 8px;
        border-radius: 999px;
        color: rgba(255,255,255,0.86);
        background: linear-gradient(135deg, rgba(132, 94, 247, 0.22), rgba(76, 197, 255, 0.14));
        border: 1px solid rgba(132, 94, 247, 0.26);
      }
      .quickbar {
        display: flex;
        gap: 8px;
        padding: 0 6px;
      }
      .quick {
        flex: 1;
        height: 38px;
        border-radius: 12px;
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 0 12px;
        box-sizing: border-box;
        user-select: none;
        cursor: pointer;
        background: var(--card);
        border: 1px solid var(--stroke);
        color: var(--fg);
        transition: transform 120ms ease, background 120ms ease, border-color 120ms ease;
      }
      .quick:hover { background: var(--card2); border-color: var(--stroke2); transform: translateY(-1px); }
      .quick:active { background: rgba(255,255,255,0.10); transform: translateY(0); }
      .quick .ico {
        width: 22px;
        height: 22px;
        border-radius: 8px;
        display: grid;
        place-items: center;
        font-size: 12px;
        flex: 0 0 auto;
      }
      .quick.talk .ico {
        background: rgba(76, 197, 255, 0.14);
        border: 1px solid rgba(76, 197, 255, 0.22);
      }
      .quick.exit .ico {
        background: rgba(255, 96, 130, 0.14);
        border: 1px solid rgba(255, 96, 130, 0.22);
      }
      .quick .label {
        font-size: 12px;
        letter-spacing: 0.2px;
      }
      .section {
        padding: 0 2px;
      }
      .section h3 {
        margin: 0 0 8px;
        padding: 0 8px;
        font-size: 11px;
        letter-spacing: 0.4px;
        color: rgba(255,255,255,0.58);
      }
      .grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 8px;
        padding: 0 6px;
      }
      .btn {
        height: 44px;
        border-radius: 12px;
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 0 12px;
        box-sizing: border-box;
        user-select: none;
        cursor: pointer;
        background: var(--card);
        border: 1px solid var(--stroke);
        color: var(--fg);
        transition: transform 120ms ease, background 120ms ease, border-color 120ms ease;
      }
      .btn:hover { background: var(--card2); border-color: var(--stroke2); transform: translateY(-1px); }
      .btn:active { background: rgba(255,255,255,0.10); transform: translateY(0); }
      .btn .ico {
        width: 22px;
        height: 22px;
        border-radius: 8px;
        display: grid;
        place-items: center;
        font-size: 12px;
        background: rgba(76, 197, 255, 0.14);
        border: 1px solid rgba(76, 197, 255, 0.22);
        flex: 0 0 auto;
      }
      .btn .label {
        font-size: 12px;
        letter-spacing: 0.2px;
      }
      .list {
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 0 6px;
      }
      .item {
        height: 38px;
        border-radius: 12px;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        padding: 0 12px;
        box-sizing: border-box;
        user-select: none;
        cursor: pointer;
        background: var(--card);
        border: 1px solid var(--stroke);
        color: var(--fg);
        transition: transform 120ms ease, background 120ms ease, border-color 120ms ease;
      }
      .item:hover { background: var(--card2); border-color: var(--stroke2); transform: translateY(-1px); }
      .item:active { background: rgba(255,255,255,0.10); transform: translateY(0); }
      .item.active {
        background: linear-gradient(135deg, rgba(132, 94, 247, 0.16), rgba(76, 197, 255, 0.10));
        border-color: rgba(132, 94, 247, 0.32);
      }
      .item .left {
        display: flex;
        align-items: center;
        gap: 10px;
      }
      .item .icon {
        width: 22px;
        height: 22px;
        border-radius: 8px;
        display: grid;
        place-items: center;
        font-size: 12px;
        background: rgba(132, 94, 247, 0.14);
        border: 1px solid rgba(132, 94, 247, 0.22);
      }
      .item .hint2 {
        font-size: 11px;
        color: var(--fg3);
      }
      .item.active .hint2 {
        color: rgba(255,255,255,0.72);
      }
      .statbox {
        height: auto;
        padding: 10px 12px;
        flex-direction: column;
        align-items: stretch;
        gap: 6px;
      }
      .statbox .top {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
      }
      .meter {
        height: 6px;
        border-radius: 999px;
        background: rgba(255,255,255,0.08);
        border: 1px solid rgba(255,255,255,0.10);
        overflow: hidden;
      }
      .meter .fill {
        height: 100%;
        width: 0%;
        background: linear-gradient(90deg, rgba(76,197,255,0.92), rgba(132,94,247,0.92));
      }
      .meter.hunger .fill {
        background: linear-gradient(90deg, rgba(64, 192, 87, 0.95), rgba(47, 158, 68, 0.95));
      }
      .sep {
        height: 1px;
        background: rgba(255,255,255,0.08);
        margin: 2px 8px;
      }
      .hint {
        margin-top: auto;
        padding: 0 10px 6px;
        font-size: 11px;
        color: rgba(255,255,255,0.55);
      }
    </style>
  </head>
  <body>
    <div class="shell">
      <div class="title">
        <div class="brand">
          <b>桌宠菜单</b>
          <span id="brand-sub">右键 · 互动与工具</span>
        </div>
        <div class="badge" id="badge-role">v0</div>
      </div>

      <div class="quickbar">
        <div class="quick talk" data-cmd="talk"><div class="ico">💬</div><div class="label">对话</div></div>
        <div class="quick exit" data-cmd="exit"><div class="ico">⏻</div><div class="label">退出</div></div>
      </div>

      <div id="content">
        <div class="section">
          <h3>状态</h3>
          <div class="list">
            <div class="item statbox">
              <div class="top">
                <div class="left"><div class="icon">⭐</div><div id="stat-level">Lv.1</div></div>
                <div class="hint2" id="stat-xp-text">0/50</div>
              </div>
              <div class="meter"><div class="fill" id="stat-xp-fill"></div></div>
            </div>
            <div class="item statbox">
              <div class="top">
                <div class="left"><div class="icon">🍗</div><div id="stat-hunger">饥饿 100/100</div></div>
                <div class="hint2" id="stat-hunger-hint">满</div>
              </div>
              <div class="meter hunger"><div class="fill" id="stat-hunger-fill"></div></div>
            </div>
            <div class="item" data-cmd="coins_refresh">
              <div class="left"><div class="icon">🪙</div><div id="stat-coins">金币 0</div></div>
              <div class="hint2">点击刷新</div>
            </div>
          </div>
        </div>

        <div class="sep"></div>

        <div class="section">
          <h3>养成</h3>
          <div class="grid">
            <div class="btn" data-cmd="feed"><div class="ico">🍗</div><div class="label">喂食</div></div>
            <div class="btn" data-cmd="play"><div class="ico">🎲</div><div class="label">玩耍</div></div>
          </div>
        </div>

        <div class="sep"></div>

        <div class="section">
          <h3>互动</h3>
          <div class="grid">
            <div class="btn" data-cmd="act:idle"><div class="ico">🧍</div><div class="label">待机</div></div>
            <div class="btn" data-cmd="act:walk"><div class="ico">🚶</div><div class="label">走路</div></div>
            <div class="btn" data-cmd="act:relax"><div class="ico">✨</div><div class="label">小动作</div></div>
            <div class="btn" data-cmd="act:sleep"><div class="ico">💤</div><div class="label">睡觉</div></div>
            <div class="btn" data-cmd="act:drag"><div class="ico">🫳</div><div class="label">拖拽</div></div>
          </div>
        </div>

      <div class="sep"></div>

        <div class="section">
          <h3>工具</h3>
          <div class="list">
            <div class="item" data-cmd="open_backend"><div class="left"><div class="icon">⚙</div><div>桌宠后台</div></div><div class="hint2">Admin</div></div>
            <div class="item" data-cmd="reset"><div class="left"><div class="icon">⚠</div><div>重置</div></div><div class="hint2">Reset</div></div>
          </div>
        </div>

        <div class="sep"></div>
        <div class="section">
          <h3>切换角色</h3>
          <div class="list" id="role-list"></div>
        </div>

        <div class="sep"></div>
        <div class="section">
          <h3>皮肤</h3>
          <div class="list" id="skin-list"></div>
        </div>
        <div class="hint">Esc 关闭</div>
      </div>
    </div>
    <script>
      const post = (cmd) => {
        try { window.chrome.webview.postMessage(cmd); } catch (_) {}
      };
      document.querySelectorAll('[data-cmd]').forEach(el => {
        el.addEventListener('click', (ev) => {
          const cmd = el.dataset.cmd;
          if (cmd === 'reset') {
            const ok = window.confirm('确定要重置桌宠吗？\\n\\n这会清空等级、经验、饥饿、金币等养成数据。');
            if (ok) post('reset');
            return;
          }
          post(cmd);
        });
      });
      const renderRoles = () => {
        const roleList = document.getElementById('role-list');
        if (!roleList) return;
        const rolesRaw = window.__petCharacters;
        const current = window.__petCharacterCurrent || '';
        const roles = Array.isArray(rolesRaw) ? rolesRaw : [];
        roleList.innerHTML = '';
        roles.forEach(r => {
          const id = r && r.id ? String(r.id) : '';
          const name = r && r.name ? String(r.name) : id;
          if (!id) return;
          const item = document.createElement('div');
          item.className = 'item' + (id === current ? ' active' : '');
          item.dataset.cmd = `character:${id}`;

          const left = document.createElement('div');
          left.className = 'left';
          const icon = document.createElement('div');
          icon.className = 'icon';
          icon.textContent = '🖼';
          const title = document.createElement('div');
          title.textContent = name;
          left.appendChild(icon);
          left.appendChild(title);

          const hint = document.createElement('div');
          hint.className = 'hint2';
          hint.textContent = id === current ? '当前' : 'Role';

          item.appendChild(left);
          item.appendChild(hint);
          item.addEventListener('click', () => post(item.dataset.cmd));
          roleList.appendChild(item);
        });
      };
      window.__petRenderRoles = renderRoles;
      const renderHeader = () => {
        const rolesRaw = window.__petCharacters;
        const current = window.__petCharacterCurrent || '';
        const roles = Array.isArray(rolesRaw) ? rolesRaw : [];
        const found = roles.find(r => r && String(r.id) === String(current));
        const roleName = found && found.name ? String(found.name) : (current ? String(current) : '桌宠');
        const skin = window.__petSkinCurrent || 'default';
        const userRaw = window.__petUserLabel != null ? String(window.__petUserLabel) : '';
        const userLabel = userRaw.trim();
        const sub = document.getElementById('brand-sub');
        if (sub) {
          sub.textContent = userLabel
            ? `${userLabel} · ${roleName} · ${skin}`
            : `${roleName} · ${skin}`;
        }
        const badge = document.getElementById('badge-role');
        if (badge) {
          const source = userLabel || roleName;
          const t = source ? String(source) : '桌宠';
          badge.textContent = t.length <= 2 ? t : t.slice(0, 2);
        }
      };
      window.__petRenderHeader = renderHeader;
      const renderSkins = () => {
        const skinList = document.getElementById('skin-list');
        if (!skinList) return;
        const skinsRaw = window.__petSkins;
        const current = window.__petSkinCurrent || 'default';
        const skins = Array.isArray(skinsRaw) && skinsRaw.length ? skinsRaw : ['default'];
        skinList.innerHTML = '';
        skins.forEach(name => {
          const item = document.createElement('div');
          item.className = 'item' + (name === current ? ' active' : '');
          item.dataset.cmd = `skin:${name}`;

          const left = document.createElement('div');
          left.className = 'left';
          const icon = document.createElement('div');
          icon.className = 'icon';
          icon.textContent = '🎨';
          const title = document.createElement('div');
          title.textContent = name;
          left.appendChild(icon);
          left.appendChild(title);

          const hint = document.createElement('div');
          hint.className = 'hint2';
          hint.textContent = name === current ? '当前' : 'Skin';

          item.appendChild(left);
          item.appendChild(hint);
          item.addEventListener('click', () => post(item.dataset.cmd));
          skinList.appendChild(item);
        });
      };
      window.__petRenderSkins = renderSkins;
      const renderStats = () => {
        const s = window.__petStats;
        if (!s) return;
        const levelEl = document.getElementById('stat-level');
        const xpTextEl = document.getElementById('stat-xp-text');
        const xpFillEl = document.getElementById('stat-xp-fill');
        const hungerEl = document.getElementById('stat-hunger');
        const hungerHintEl = document.getElementById('stat-hunger-hint');
        const hungerFillEl = document.getElementById('stat-hunger-fill');
        const coinsEl = document.getElementById('stat-coins');
        if (levelEl) levelEl.textContent = `Lv.${s.level ?? 1}`;
        const xp = Math.max(0, Number(s.xp ?? 0));
        const xpToNext = Math.max(1, Number(s.xp_to_next ?? 1));
        if (xpTextEl) xpTextEl.textContent = `${xp}/${xpToNext}`;
        if (xpFillEl) xpFillEl.style.width = `${Math.max(0, Math.min(1, xp / xpToNext)) * 100}%`;
        const h = Math.max(0, Math.min(100, Number(s.hunger ?? 0)));
        if (hungerEl) hungerEl.textContent = `饥饿 ${h}/100`;
        if (coinsEl) coinsEl.textContent = `金币 ${Math.max(0, Number(s.coins ?? 0))}`;
        if (hungerHintEl) {
          hungerHintEl.textContent = h >= 70 ? '饱' : h >= 30 ? '一般' : '饿';
        }
        if (hungerFillEl) {
          hungerFillEl.style.width = `${h}%`;
          if (h < 30) {
            hungerFillEl.style.background =
              'linear-gradient(90deg, rgba(255, 107, 107, 0.95), rgba(240, 62, 62, 0.95))';
            if (hungerHintEl) hungerHintEl.style.color = 'rgba(255, 107, 107, 0.90)';
          } else if (h < 70) {
            hungerFillEl.style.background =
              'linear-gradient(90deg, rgba(255, 212, 59, 0.95), rgba(250, 176, 5, 0.95))';
            if (hungerHintEl) hungerHintEl.style.color = 'rgba(255, 212, 59, 0.92)';
          } else {
            hungerFillEl.style.background =
              'linear-gradient(90deg, rgba(64, 192, 87, 0.95), rgba(47, 158, 68, 0.95))';
            if (hungerHintEl) hungerHintEl.style.color = 'rgba(64, 192, 87, 0.92)';
          }
        }
      };
      window.__petRenderStats = renderStats;
      // drag window by shell
      let dragging = false;
      let startClientX = 0, startClientY = 0;
      document.querySelector('.shell').addEventListener('mousedown', (e) => {
        if (e.button !== 0) return;
        dragging = true;
        startClientX = e.clientX;
        startClientY = e.clientY;
      });
      window.addEventListener('mousemove', (e) => {
        if (!dragging) return;
        const x = Math.round(e.screenX - startClientX);
        const y = Math.round(e.screenY - startClientY);
        post(`menu:move:${x}:${y}`);
      });
      window.addEventListener('mouseup', () => { dragging = false; });
      let tries = 0;
      const t = setInterval(() => {
        renderHeader();
        renderRoles();
        renderSkins();
        renderStats();
        tries++;
        if (tries >= 10) clearInterval(t);
      }, 60);
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
        width: 100%;
        height: 100%;
        margin: 0;
        padding: 0;
        background: rgba(10, 10, 14, 0.62);
        font-family: system-ui, "Segoe UI", Arial, sans-serif;
        overflow: hidden;
      }
      .wrap {
        width: 100%;
        height: 100%;
        box-sizing: border-box;
        padding: 14px;
        display: flex;
        flex-direction: column;
        gap: 10px;
        border: 1px solid rgba(255,255,255,0.12);
        border-radius: 18px;
        background:
          radial-gradient(120% 120% at 10% 10%, rgba(95, 165, 255, 0.18), rgba(0,0,0,0) 55%),
          radial-gradient(120% 120% at 90% 0%, rgba(255, 110, 199, 0.12), rgba(0,0,0,0) 50%),
          rgba(18, 18, 22, 0.86);
        backdrop-filter: blur(18px);
        -webkit-backdrop-filter: blur(18px);
        box-shadow:
          0 18px 60px rgba(0,0,0,0.55),
          0 1px 0 rgba(255,255,255,0.06) inset;
      }
      .title {
        display: flex;
        align-items: center;
        justify-content: space-between;
        color: rgba(255,255,255,0.92);
        font-size: 13px;
        letter-spacing: 0.2px;
        padding: 2px 2px 0;
        cursor: move;
        user-select: none;
      }
      .title .left {
        display: flex;
        gap: 8px;
        align-items: center;
      }
      .dot {
        width: 9px;
        height: 9px;
        border-radius: 999px;
        background: rgba(120, 190, 255, 0.75);
        box-shadow: 0 0 0 3px rgba(120, 190, 255, 0.16);
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
        padding: 10px 10px;
        border-radius: 14px;
        border: 1px solid rgba(255,255,255,0.10);
        background: rgba(0,0,0,0.16);
        display: flex;
        flex-direction: column;
        gap: 10px;
      }
      .row { display: flex; }
      .row.user { justify-content: flex-end; }
      .row.ai { justify-content: flex-start; }
      .bubble {
        max-width: 360px;
        padding: 10px 12px;
        border-radius: 14px;
        font-size: 13px;
        line-height: 1.45;
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
        border-radius: 14px;
        border: 1px solid rgba(255,255,255,0.12);
        background: rgba(0,0,0,0.18);
        color: rgba(255,255,255,0.92);
        padding: 0 10px;
        font-size: 13px;
        outline: none;
      }
      input:focus {
        border-color: rgba(120, 190, 255, 0.42);
        box-shadow: 0 0 0 4px rgba(120, 190, 255, 0.12);
      }
      .btn {
        height: 34px;
        padding: 0 12px;
        border-radius: 14px;
        border: 1px solid rgba(255,255,255,0.12);
        background: rgba(255,255,255,0.08);
        color: rgba(255,255,255,0.92);
        display: flex;
        align-items: center;
        gap: 8px;
        cursor: default;
        user-select: none;
        font-size: 13px;
        white-space: nowrap;
      }
      .btn:hover { background: rgba(255,255,255,0.12); }
      .btn.primary {
        background: rgba(120, 190, 255, 0.16);
        border-color: rgba(120, 190, 255, 0.22);
      }
      .btn.primary:hover { background: rgba(120, 190, 255, 0.22); }
    </style>
  </head>
  <body>
    <div class="wrap">
      <div class="title">
        <div class="left"><span class="dot"></span><div>聊天室</div></div>
        <div class="close" id="close">✕</div>
      </div>
      <div class="msgs" id="msgs"></div>
      <div class="bar">
        <div class="btn" id="mic">🎙 语音</div>
        <input id="input" placeholder="输入内容，Enter 发送，Esc 关闭"/>
        <div class="btn primary" id="send">发送</div>
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

      let dragging = false;
      let startClientX = 0, startClientY = 0;
      const titleEl = document.querySelector('.title');
      if (titleEl) {
        titleEl.addEventListener('mousedown', (e) => {
          if (e.button !== 0) return;
          if (e.target && e.target.id === 'close') return;
          dragging = true;
          startClientX = e.clientX;
          startClientY = e.clientY;
        });
      }
      window.addEventListener('mousemove', (e) => {
        if (!dragging) return;
        const x = Math.round(e.screenX - startClientX);
        const y = Math.round(e.screenY - startClientY);
        post(`chat:move:${x}:${y}`);
      });
      window.addEventListener('mouseup', () => { dragging = false; });

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
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAYICON,
        hIcon: LoadIconW(None, IDI_APPLICATION).unwrap(),
        ..Default::default()
    };
    fill_wide(&mut nid.szTip, "DesktopPet");
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn tray_remove(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

fn spawn_backend_monitor() {
    std::thread::spawn(move || {
        let ipc_client = global_client();

        let mut last_summary = Instant::now() - std::time::Duration::from_secs(600);

        loop {
            let msg = IpcMessage::new_request("config", "get", serde_json::json!({}));
            let alive = ipc_client.send(&msg).is_ok();

            if !alive {
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }

            if last_summary.elapsed() >= std::time::Duration::from_secs(600) {
                let msg = IpcMessage::new_request("diary", "summarize", serde_json::json!({}));
                let _ = ipc_client.send(&msg);
                last_summary = Instant::now();
            }

            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });
}

fn spawn_backend_portal_opener(backend_base_url: String) {
    std::thread::spawn(move || {
        let ipc_client = global_client();
        let open_url = if backend_base_url.ends_with('/') {
            backend_base_url
        } else {
            format!("{backend_base_url}/")
        };

        loop {
            let msg = IpcMessage::new_request("config", "get", serde_json::json!({}));
            let alive = ipc_client.send(&msg).is_ok();
            if alive {
                interaction::open_url_in_browser(&open_url);
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_DROPFILES => {
                let hdrop = HDROP(lparam.0);
                let count = DragQueryFileW(hdrop, 0xFFFFFFFF, Some(&mut []));
                if count > 0 {
                    let delta = 20;
                    if let Ok(mut s) = PET_STATS.lock() {
                        s.add_hunger(delta);
                    }
                    if let Ok(mut g) = BUBBLE_TEXT.lock() {
                        let tpl = CHARACTER_TEXTS
                            .lock()
                            .ok()
                            .and_then(|t| t.snack_received.clone())
                            .unwrap_or_else(|| "收到零食啦，饥饿 +{delta}".to_string());
                        *g = Some(tpl.replace("{delta}", &delta.to_string()));
                    }
                }
                DragFinish(hdrop);
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_LBUTTONUP | WM_RBUTTONUP | WM_TRAYICON => {
                interaction::handle_wnd_message(hwnd, msg, wparam, lparam)
                    .unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                tray_remove(hwnd);
                let payload = PET_STATS.lock().ok().map(|s| BackendPetLevelUpdate {
                    user_id: fetch_store_user_id(),
                    level: s.level,
                    xp: s.xp,
                    hunger: s.hunger,
                    coins: Some(s.coins),
                });
                if let Some(payload) = payload {
                    let ipc_client = global_client();
                    let msg = IpcMessage::new_request(
                        "pet_level",
                        "post",
                        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
                    );
                    let _ = ipc_client.send(&msg);
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn single_frame_player(path: &str) -> AnimationPlayer {
    AnimationPlayer::new(Animation {
        frames: vec![Frame {
            path: path.to_string(),
        }],
        fps: 1,
        looped: true,
    })
}

fn find_first_png(dir: &PathBuf) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let is_png = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("png"))
                .unwrap_or(false);
            if is_png {
                return Some(path.to_string_lossy().to_string());
            }
        } else if path.is_dir() {
            if let Some(p) = find_first_png(&path) {
                return Some(p);
            }
        }
    }
    None
}

fn safe_load_animation_player(
    dir: &PathBuf,
    fps: u32,
    looped: bool,
    fallback_path: &str,
) -> AnimationPlayer {
    if std::fs::metadata(dir).is_ok() {
        let anim = load_animation(dir.to_string_lossy().as_ref(), fps, looped);
        if anim.frames.is_empty() {
            single_frame_player(fallback_path)
        } else {
            AnimationPlayer::new(anim)
        }
    } else {
        single_frame_player(fallback_path)
    }
}

fn max_animation_dimensions(players: &[&AnimationPlayer]) -> (i32, i32) {
    let mut max_w: u32 = 1;
    let mut max_h: u32 = 1;

    for player in players {
        for frame in &player.animation.frames {
            if let Ok(img) = image::open(&frame.path) {
                let (w, h) = img.dimensions();
                max_w = max_w.max(w);
                max_h = max_h.max(h);
            }
        }
    }

    (max_w as i32, max_h as i32)
}

fn build_actor_assets(animations_dir: &PathBuf) -> (ActorAssets, String) {
    let fallback_frame_path =
        find_first_png(animations_dir).expect("animations 目录中没有任何 png，无法启动");

    let has_walk_left = std::fs::metadata(animations_dir.join("walk_left")).is_ok();
    let has_walk_right = std::fs::metadata(animations_dir.join("walk_right")).is_ok();
    let has_walk = std::fs::metadata(animations_dir.join("walk")).is_ok();

    let mut flip_walk_left = false;
    let mut flip_walk_right = false;

    let (walk_left_dir, walk_right_dir) = if has_walk_left && has_walk_right {
        (
            animations_dir.join("walk_left"),
            animations_dir.join("walk_right"),
        )
    } else if has_walk_right {
        flip_walk_left = true;
        (
            animations_dir.join("walk_right"),
            animations_dir.join("walk_right"),
        )
    } else if has_walk_left {
        flip_walk_right = true;
        (
            animations_dir.join("walk_left"),
            animations_dir.join("walk_left"),
        )
    } else if has_walk {
        flip_walk_left = true;
        (animations_dir.join("walk"), animations_dir.join("walk"))
    } else {
        flip_walk_left = true;
        (animations_dir.clone(), animations_dir.clone())
    };
    let walk_left = safe_load_animation_player(&walk_left_dir, 12, true, &fallback_frame_path);
    let walk_right = safe_load_animation_player(&walk_right_dir, 12, true, &fallback_frame_path);

    let has_drag_left = std::fs::metadata(animations_dir.join("drag_left")).is_ok();
    let has_drag_right = std::fs::metadata(animations_dir.join("drag_right")).is_ok();
    let has_drag = std::fs::metadata(animations_dir.join("drag")).is_ok();

    let mut flip_drag_left = false;
    let mut flip_drag_right = false;

    let (drag_left_dir, drag_right_dir) = if has_drag_left && has_drag_right {
        (
            animations_dir.join("drag_left"),
            animations_dir.join("drag_right"),
        )
    } else if has_drag_right {
        flip_drag_left = true;
        (
            animations_dir.join("drag_right"),
            animations_dir.join("drag_right"),
        )
    } else if has_drag_left {
        flip_drag_right = true;
        (
            animations_dir.join("drag_left"),
            animations_dir.join("drag_left"),
        )
    } else if has_drag {
        flip_drag_left = true;
        (animations_dir.join("drag"), animations_dir.join("drag"))
    } else if has_walk_right {
        flip_drag_left = true;
        (
            animations_dir.join("walk_right"),
            animations_dir.join("walk_right"),
        )
    } else if has_walk_left {
        flip_drag_right = true;
        (
            animations_dir.join("walk_left"),
            animations_dir.join("walk_left"),
        )
    } else if has_walk {
        flip_drag_left = true;
        (animations_dir.join("walk"), animations_dir.join("walk"))
    } else {
        flip_drag_left = true;
        (animations_dir.clone(), animations_dir.clone())
    };

    let drag_left = safe_load_animation_player(&drag_left_dir, 12, false, &fallback_frame_path);
    let drag_right = safe_load_animation_player(&drag_right_dir, 12, false, &fallback_frame_path);

    let idle_dir = animations_dir.join("idle");
    let idle = safe_load_animation_player(&idle_dir, 6, true, &fallback_frame_path);
    let idle_frame_path = idle
        .animation
        .frames
        .first()
        .map(|f| f.path.clone())
        .unwrap_or_else(|| fallback_frame_path.clone());
    let idle_base_facing = if std::fs::metadata(&idle_dir).is_ok() {
        Facing::Right
    } else if !walk_right.animation.frames.is_empty() {
        if flip_walk_right {
            Facing::Left
        } else {
            Facing::Right
        }
    } else if !walk_left.animation.frames.is_empty() {
        if flip_walk_left {
            Facing::Right
        } else {
            Facing::Left
        }
    } else {
        Facing::Right
    };

    let relax =
        safe_load_animation_player(&animations_dir.join("relax"), 8, true, &idle_frame_path);
    let sleep =
        safe_load_animation_player(&animations_dir.join("sleep"), 6, true, &idle_frame_path);

    (
        ActorAssets {
            walk_left,
            walk_right,
            idle,
            relax,
            sleep,
            drag_left,
            drag_right,
            idle_base_facing,
            flip_walk_left,
            flip_walk_right,
            flip_drag_left,
            flip_drag_right,
        },
        fallback_frame_path,
    )
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let backend_url = pet_level_sync::backend_base_url();
    spawn_backend_portal_opener(backend_url.clone());
    let (auto_talk_tx, auto_talk_rx) = channel::<AutoTalkWsMsg>();
    let (ws_cmd_tx, ws_cmd_rx) = channel::<WsClientCmd>();
    spawn_auto_talk_ws_listener(backend_url.clone(), auto_talk_tx, ws_cmd_rx);

    pet_level_sync::init_from_backend();

    let (pet_save_tx, pet_save_rx) = channel::<BackendPetLevelUpdate>();
    pet_level_sync::spawn_saver(pet_save_rx);
    pet_level_sync::spawn_poller();

    let loaded_character = mod_loader::load_character_from_env();
    let mut char_mod = loaded_character.char_mod;
    let mut current_character_id = char_mod
        .base_dir
        .file_name()
        .and_then(|x| x.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "debug_pet".to_string());
    let _ = (&char_mod.name, &char_mod.animations_dir);
    if let Ok(mut g) = CHARACTER_TEXTS.lock() {
        *g = char_mod.personality.texts.clone().unwrap_or_default();
    }
    ai_persona::set_base_personality_from_character(&char_mod);
    {
        let stats = PET_STATS.lock().ok().map(|s| s.to_json());
        let base = std::fs::read_to_string(char_mod.base_dir.join("character.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .or_else(|| ai_persona::base_personality_value());
        let personality = if let Some(sv) = stats {
            if let Some(mut pv) = base {
                match &mut pv {
                    serde_json::Value::Object(obj) => {
                        obj.insert("pet_status".to_string(), sv);
                        Some(pv)
                    }
                    _ => Some(serde_json::json!({ "pet_status": sv, "base": pv })),
                }
            } else {
                Some(serde_json::json!({ "pet_status": sv }))
            }
        } else {
            base
        };
        let _ = ws_cmd_tx.send(WsClientCmd::PersonaUpdated(personality));
    }
    let mut skins = loaded_character.skins;
    let mut current_skin = loaded_character.current_skin;
    let mut animations_dir = loaded_character.animations_dir;
    let (assets, _fallback_frame_path) = build_actor_assets(&animations_dir);
    let mut pet_actor = Actor::new(assets);
    pet_actor.set_texts(char_mod.personality.texts.clone().unwrap_or_default());

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

    let (initial_w, initial_h) = max_animation_dimensions(&[
        &pet_actor.walk_left,
        &pet_actor.walk_right,
        &pet_actor.idle,
        &pet_actor.relax,
        &pet_actor.sleep,
        &pet_actor.drag_left,
        &pet_actor.drag_right,
    ]);
    let mut pet_w = initial_w;
    let mut pet_h = initial_h;

    let base_speed_dip = 24.0_f32;
    let mut pet_mover = Mover {
        pos: Position { x: 100.0, y: 100.0 },
        target: Target { x: 400.0, y: 100.0 },
        speed: base_speed_dip,
        state: MoverState::Moving,
        facing: Facing::Right,
        bounds_x: unsafe {
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN) as f32
        } - PET_RANGE_MARGIN as f32,
        bounds_y: unsafe {
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN) as f32
        } - PET_RANGE_MARGIN as f32,
        bounds_w: unsafe {
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN) as f32
        } + (PET_RANGE_MARGIN * 2) as f32,
        bounds_h: unsafe {
            GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN) as f32
        } + (PET_RANGE_MARGIN * 2) as f32,
        sprite_w: pet_w as f32,
        sprite_h: pet_h as f32,
    };
    let mut physics_body = PhysicsBody::new(pet_mover.pos.x, pet_mover.pos.y);
    let mut was_dragging = false;

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("DesktopPet"),
            WS_POPUP,
            pet_mover.pos.x as i32,
            pet_mover.pos.y as i32,
            pet_w,
            pet_h,
            None,
            None,
            hinstance,
            None,
        )
    };
    unsafe {
        DragAcceptFiles(hwnd, true);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    unsafe {
        tray_add(hwnd);
    }

    let mut web_menu = unsafe { interaction::WebMenu::new(hinstance) };
    unsafe {
        web_menu.init(MENU_HTML, backend_url.clone());
    }

    let (ai_req_tx, ai_req_rx) = channel::<AiRequest>();
    let (ai_resp_tx, ai_resp_rx) = channel::<AiResponse>();
    scripting::spawn_ai_worker(ai_req_rx, ai_resp_tx);
    spawn_backend_monitor();
    realworld::spawn_realworld_poller(backend_url.clone());

    interaction::set_bubble_sink(std::sync::Arc::new(|text| {
        if let Ok(mut g) = BUBBLE_TEXT.lock() {
            *g = Some(text);
        }
    }));

    let mut web_chat = unsafe { interaction::WebChat::new(hinstance) };
    unsafe {
        let personality_provider = std::sync::Arc::new(|| {
            let stats = PET_STATS.lock().ok().map(|s| s.to_json());
            ai_persona::compose_personality(stats)
        });
        web_chat.init(CHAT_HTML, ai_req_tx.clone(), personality_provider);
    }

    let mut scene = unsafe { Scene::new(hinstance, pet_w, pet_h, BUBBLE_W, BUBBLE_H) };
    let mut msg = MSG::default();
    let mut last_tick = Instant::now();
    let mut last_pet_save_tick = Instant::now();
    let mut work_area = interaction::virtual_work_area();
    let mut last_work_area_tick = Instant::now();
    let mut last_auto_talk_evt_at: Option<Instant> = None;
    let mut auto_talk_ok: u64 = 0;
    let mut auto_talk_err: u64 = 0;
    let mut last_timer_applied_at: Option<Instant> = None;
    let mut last_pet_clicked_applied_at: Option<Instant> = None;
    let mut clicked_at: VecDeque<Instant> = VecDeque::new();
    let mut suppress_default_phrase_until: Option<Instant> = None;
    let mut last_level = PET_STATS.lock().ok().map(|s| s.level).unwrap_or(1);
    let mut forced_sleep_active = false;

    'outer: loop {
        if last_work_area_tick.elapsed() >= std::time::Duration::from_millis(500) {
            work_area = interaction::virtual_work_area();
            last_work_area_tick = Instant::now();
        }
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

        if let Some((x, y)) = interaction::take_menu_request() {
            let chars =
                serde_json::to_string(&mod_loader::list_characters()).unwrap_or_else(|_| "[]".to_string());
            let cur_char = serde_json::to_string(&current_character_id)
                .unwrap_or_else(|_| "\"\"".to_string());
            let skins_json = serde_json::to_string(&skins).unwrap_or_else(|_| "[]".to_string());
            let cur = serde_json::to_string(&current_skin).unwrap_or_else(|_| "\"default\"".to_string());
            let stats = PET_STATS
                .lock()
                .ok()
                .map(|s| s.to_json().to_string())
                .unwrap_or_else(|| "null".to_string());
            web_menu.prepare_payload(&chars, &cur_char, &skins_json, &cur, &stats, true);
            unsafe {
                web_menu.show_at(x, y);
            }
        }

        if let Some((x, y)) = interaction::take_chat_request() {
            unsafe {
                web_chat.show_at(x, y);
            }
        }

        let menu_visible = unsafe { IsWindowVisible(web_menu.hwnd()).as_bool() };
        let chat_visible = unsafe { IsWindowVisible(web_chat.hwnd()).as_bool() };
        let ui_visible = menu_visible || chat_visible;
        // 聊天窗口显示时不停止宠物动作，只有主菜单显示时才停止
        let pet_stop_requested = menu_visible;

        if chat_visible {
            web_chat.on_visible_tick();
        }

        let now = Instant::now();
        let delta = now.saturating_duration_since(last_tick);
        last_tick = now;
        let should_save_pet = last_pet_save_tick.elapsed() >= Duration::from_secs(30);
        let (hunger, sleep_rolls, pet_save_payload, cur_level) = PET_STATS
            .lock()
            .ok()
            .map(|mut s| {
                let rolls = s.tick(delta.as_millis() as u64);
                let payload = if should_save_pet && s.dirty {
                    Some(BackendPetLevelUpdate {
                        user_id: fetch_store_user_id(),
                        level: s.level,
                        xp: s.xp,
                        hunger: s.hunger,
                        coins: None,
                    })
                } else {
                    None
                };
                (s.hunger, rolls, payload, s.level)
            })
            .unwrap_or((0, 0, None, last_level));
        if let Some(p) = pet_save_payload {
            let _ = pet_save_tx.send(p);
            last_pet_save_tick = Instant::now();
        }

        if cur_level > last_level {
            last_level = cur_level;
            let _ = ws_cmd_tx.send(WsClientCmd::LevelUp(cur_level));
            suppress_default_phrase_until = Some(Instant::now() + Duration::from_secs(2));
            if menu_visible {
                let stats = PET_STATS
                    .lock()
                    .ok()
                    .map(|s| s.to_json().to_string())
                    .unwrap_or_else(|| "null".to_string());
                web_menu.update_stats(&stats);
            }
        }

        if interaction::take_pet_clicked() {
            let _ = ws_cmd_tx.send(WsClientCmd::PetClicked);
            suppress_default_phrase_until = Some(Instant::now() + Duration::from_secs(2));
            clicked_at.push_back(Instant::now());
        }

        if interaction::take_feed_request() {
            let text = pick_event_phrase(char_mod.personality.texts.as_ref(), "feed")
                .unwrap_or_else(|| "博士，谢谢款待~".to_string());
            let _ = ws_cmd_tx.send(WsClientCmd::Feed { delta: 25, text });
            suppress_default_phrase_until = Some(Instant::now() + Duration::from_secs(2));
        }

        if interaction::take_coins_refresh_request() {
            std::thread::spawn(|| {
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
                        if let Ok(mut g) = BUBBLE_TEXT.lock() {
                            *g = Some("金币已刷新".to_string());
                        }
                    }
                }
            });
        }

        if interaction::take_play_request() {
            if let Ok(mut s) = PET_STATS.lock() {
                s.add_xp(6);
                s.add_hunger(-3);
            }
            if let Ok(mut g) = BUBBLE_TEXT.lock() {
                *g = Some("好玩！".to_string());
            }
        }

        if interaction::take_reset_request() {
            forced_sleep_active = false;
            if let Ok(mut s) = PET_STATS.lock() {
                s.level = 0;
                s.xp = 0;
                s.hunger = 100;
                s.coins = 0;
                s.hunger_acc_ms = 0;
                s.xp_acc_ms = 0;
                s.sleep_roll_acc_ms = 0;
                s.dirty = false;
                let _ = pet_save_tx.send(BackendPetLevelUpdate {
                    user_id: fetch_store_user_id(),
                    level: s.level,
                    xp: s.xp,
                    hunger: s.hunger,
                    coins: Some(s.coins),
                });
                last_pet_save_tick = Instant::now();
            }
            mod_loader::request_skin("default".to_string());
            physics_body.stop();
            pet_actor.request_state(ActorState::Idle);
            if let Ok(mut g) = BUBBLE_TEXT.lock() {
                *g = Some("已重置桌宠".to_string());
            }
            if menu_visible {
                let stats = PET_STATS
                    .lock()
                    .ok()
                    .map(|s| s.to_json().to_string())
                    .unwrap_or_else(|| "null".to_string());
                web_menu.update_stats(&stats);
            }
        }

        let mut talk_trigger = false;

        while let Ok(resp) = ai_resp_rx.try_recv() {
            let reply = resp.assistant_text;
            if let Ok(mut g) = BUBBLE_TEXT.lock() {
                *g = Some(reply.clone());
            }
            talk_trigger = true;
            web_chat.push_ai_reply(&reply);
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) } as f32;
        pet_mover.speed = base_speed_dip * (dpi / 96.0);

        let now2 = Instant::now();
        while let Some(front) = clicked_at.front().copied() {
            if now2.duration_since(front) > Duration::from_secs(30) {
                clicked_at.pop_front();
            } else {
                break;
            }
        }
        let (hour, bad_weather) = REALWORLD
            .lock()
            .ok()
            .map(|g| (g.hour, g.bad_weather))
            .unwrap_or((12, false));
        pet_actor.set_behavior_context(state_machine::BehaviorContext {
            hunger,
            hour,
            clicks_30s: clicked_at.len() as u32,
            bad_weather,
        });
        let (frame_path, need_flip) = pet_actor.current_frame_path_and_flip();
        let (body_l, body_t, body_r, body_b) =
            scene.opaque_bounds(frame_path, need_flip, pet_w, pet_h);
        let body_w = (body_r as i32 - body_l as i32).max(1) as f32;
        let body_h = (body_b as i32 - body_t as i32).max(1) as f32;

        pet_mover.bounds_x = work_area.left as f32 - body_l as f32;
        pet_mover.bounds_y = work_area.top as f32 - body_t as f32;
        pet_mover.bounds_w = (work_area.right - work_area.left).max(1) as f32;
        pet_mover.bounds_h = (work_area.bottom - work_area.top).max(1) as f32;
        pet_mover.sprite_w = body_w;
        pet_mover.sprite_h = body_h;

        if pet_stop_requested {
            physics_body.stop();
            physics_body.sync_pos(pet_mover.pos.x, pet_mover.pos.y);
            physics_body.end_drag();
        }

        let dragging = interaction::is_dragging();
        if dragging {
            let (mut x, mut y) = interaction::drag_position();

            let min_x = work_area.left - body_l as i32;
            let min_y = work_area.top - body_t as i32;
            let max_x = (work_area.right - body_r as i32).max(min_x);
            let max_y = (work_area.bottom - body_b as i32).max(min_y);

            x = x.clamp(min_x, max_x);
            y = y.clamp(min_y, max_y);

            interaction::set_drag_position(x, y);

            pet_mover.pos.x = x as f32;
            pet_mover.pos.y = y as f32;
            pet_mover.stop_at_current_pos();

            physics_body.on_drag(pet_mover.pos.x, pet_mover.pos.y, delta);
            was_dragging = true;
        } else if was_dragging {
            physics_body.end_drag();
            was_dragging = false;
        }

        if let Some(id) = mod_loader::take_requested_character() {
            let prev_character_id = current_character_id.clone();
            let prev_skin = current_skin.clone();
            let prev_pet_w = pet_w;
            let prev_pet_h = pet_h;

            let attempt = catch_unwind(AssertUnwindSafe(|| {
                let loaded = mod_loader::load_character_by_id(&id);
                let new_char_mod = loaded.char_mod;
                let new_character_id = new_char_mod
                    .base_dir
                    .file_name()
                    .and_then(|x| x.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "debug_pet".to_string());
                let new_skins = loaded.skins;
                let new_skin = loaded.current_skin;
                let new_animations_dir = loaded.animations_dir;
                let (assets, _fallback) = build_actor_assets(&new_animations_dir);
                let mut actor = Actor::new(assets);
                actor.set_texts(new_char_mod.personality.texts.clone().unwrap_or_default());
                (
                    new_char_mod,
                    new_character_id,
                    new_skins,
                    new_skin,
                    new_animations_dir,
                    actor,
                )
            }));

            match attempt {
                Ok((
                    new_char_mod,
                    new_character_id,
                    new_skins,
                    new_skin,
                    new_animations_dir,
                    new_actor,
                )) => {
                    char_mod = new_char_mod;
                    current_character_id = new_character_id;
                    skins = new_skins;
                    current_skin = new_skin;
                    animations_dir = new_animations_dir;
                    pet_actor = new_actor;

                    if let Ok(mut g) = CHARACTER_TEXTS.lock() {
                        *g = char_mod.personality.texts.clone().unwrap_or_default();
                    }
                    ai_persona::set_base_personality_from_character(&char_mod);
                    {
                        let stats = PET_STATS.lock().ok().map(|s| s.to_json());
                        let base = std::fs::read_to_string(char_mod.base_dir.join("character.json"))
                            .ok()
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                            .or_else(|| ai_persona::base_personality_value());
                        let personality = if let Some(sv) = stats {
                            if let Some(mut pv) = base {
                                match &mut pv {
                                    serde_json::Value::Object(obj) => {
                                        obj.insert("pet_status".to_string(), sv);
                                        Some(pv)
                                    }
                                    _ => Some(serde_json::json!({ "pet_status": sv, "base": pv })),
                                }
                            } else {
                                Some(serde_json::json!({ "pet_status": sv }))
                            }
                        } else {
                            base
                        };
                        let _ = ws_cmd_tx.send(WsClientCmd::PersonaUpdated(personality));
                    }

                    physics_body.stop();
                    pet_actor.request_state(ActorState::Idle);

                    let (w, h) = max_animation_dimensions(&[
                        &pet_actor.walk_left,
                        &pet_actor.walk_right,
                        &pet_actor.idle,
                        &pet_actor.relax,
                        &pet_actor.sleep,
                        &pet_actor.drag_left,
                        &pet_actor.drag_right,
                    ]);
                    let w = w.max(1);
                    let h = h.max(1);
                    if w != pet_w || h != pet_h {
                        pet_w = w;
                        pet_h = h;
                        unsafe { scene.resize_pet_surface(pet_w, pet_h); }
                        let _ = unsafe {
                            windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                                hwnd,
                                HWND(0),
                                pet_mover.pos.x as i32,
                                pet_mover.pos.y as i32,
                                pet_w,
                                pet_h,
                                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
                            )
                        };
                        scene.reset_composer();
                    }
                }
                Err(_) => {
                    let loaded = mod_loader::load_character_by_id(&prev_character_id);
                    char_mod = loaded.char_mod;
                    skins = loaded.skins;
                    current_skin = prev_skin;
                    current_character_id = prev_character_id;
                    animations_dir = loaded.animations_dir;
                    let (assets, _) = build_actor_assets(&animations_dir);
                    pet_actor = Actor::new(assets);
                    pet_actor.set_texts(char_mod.personality.texts.clone().unwrap_or_default());
                    pet_actor.enqueue_bubble_text("切换失败，已回退".to_string());
                    pet_w = prev_pet_w;
                    pet_h = prev_pet_h;
                }
            }

            if menu_visible {
                let chars =
                    serde_json::to_string(&mod_loader::list_characters()).unwrap_or_else(|_| "[]".to_string());
                let cur_char = serde_json::to_string(&current_character_id)
                    .unwrap_or_else(|_| "\"\"".to_string());
                let skins_json = serde_json::to_string(&skins).unwrap_or_else(|_| "[]".to_string());
                let cur_json =
                    serde_json::to_string(&current_skin).unwrap_or_else(|_| "\"default\"".to_string());
                let stats = PET_STATS
                    .lock()
                    .ok()
                    .map(|s| s.to_json().to_string())
                    .unwrap_or_else(|| "null".to_string());
                web_menu.prepare_payload(&chars, &cur_char, &skins_json, &cur_json, &stats, true);
            }
        }

        if let Some(skin) = mod_loader::take_requested_skin() {
            let prev_skin = current_skin.clone();
            let attempt = catch_unwind(AssertUnwindSafe(|| {
                let new_dir = char_mod.animations_dir_for_skin(&skin);
                let (assets, _fallback) = build_actor_assets(&new_dir);
                let mut actor = Actor::new(assets);
                actor.set_texts(char_mod.personality.texts.clone().unwrap_or_default());
                actor
            }));
            match attempt {
                Ok(actor) => {
                    current_skin = skin.clone();
                    pet_actor = actor;
                    let (w, h) = max_animation_dimensions(&[
                        &pet_actor.walk_left,
                        &pet_actor.walk_right,
                        &pet_actor.idle,
                        &pet_actor.relax,
                        &pet_actor.sleep,
                        &pet_actor.drag_left,
                        &pet_actor.drag_right,
                    ]);
                    let w = w.max(1);
                    let h = h.max(1);
                    if w != pet_w || h != pet_h {
                        pet_w = w;
                        pet_h = h;
                        unsafe { scene.resize_pet_surface(pet_w, pet_h); }
                        let _ = unsafe {
                            windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                                hwnd,
                                HWND(0),
                                pet_mover.pos.x as i32,
                                pet_mover.pos.y as i32,
                                pet_w,
                                pet_h,
                                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
                            )
                        };
                        scene.reset_composer();
                    }
                }
                Err(_) => {
                    current_skin = prev_skin;
                    pet_actor.enqueue_bubble_text("切换失败，已回退".to_string());
                }
            }

            if menu_visible {
                let chars =
                    serde_json::to_string(&mod_loader::list_characters()).unwrap_or_else(|_| "[]".to_string());
                let cur_char = serde_json::to_string(&current_character_id)
                    .unwrap_or_else(|_| "\"\"".to_string());
                let skins_json = serde_json::to_string(&skins).unwrap_or_else(|_| "[]".to_string());
                let cur_json =
                    serde_json::to_string(&current_skin).unwrap_or_else(|_| "\"default\"".to_string());
                let stats = PET_STATS
                    .lock()
                    .ok()
                    .map(|s| s.to_json().to_string())
                    .unwrap_or_else(|| "null".to_string());
                web_menu.prepare_payload(&chars, &cur_char, &skins_json, &cur_json, &stats, true);
            }
        }

        let (frame_path, need_flip) = pet_actor.current_frame_path_and_flip();
        let (body_l, body_t, body_r, body_b) =
            scene.opaque_bounds(frame_path, need_flip, pet_w, pet_h);
        let body_w = (body_r as i32 - body_l as i32).max(1) as f32;
        let body_h = (body_b as i32 - body_t as i32).max(1) as f32;
        pet_mover.bounds_x = work_area.left as f32 - body_l as f32;
        pet_mover.bounds_y = work_area.top as f32 - body_t as f32;
        pet_mover.sprite_w = body_w;
        pet_mover.sprite_h = body_h;

        match interaction::take_interact_action() {
            0 => {
                physics_body.stop();
                pet_actor.request_state(ActorState::Idle);
            }
            1 => {
                physics_body.stop();
                pet_actor.request_state(ActorState::Walk);
            }
            2 => {
                physics_body.stop();
                pet_actor.request_state(ActorState::Relax);
            }
            3 => {
                physics_body.stop();
                pet_actor.request_state(ActorState::Sleep);
            }
            4 => pet_actor.request_drag_pose_ms(2000),
            _ => {}
        }

        if !dragging {
            if hunger <= 0 {
                if !forced_sleep_active {
                    forced_sleep_active = true;
                    physics_body.stop();
                    if pet_actor.state != ActorState::Sleep {
                        pet_actor.request_state(ActorState::Sleep);
                    }
                }
            } else {
                if forced_sleep_active {
                    forced_sleep_active = false;
                    if pet_actor.state == ActorState::Sleep {
                        pet_actor.request_state(ActorState::Idle);
                    }
                }
                if !ui_visible && pet_actor.state != ActorState::Sleep && sleep_rolls > 0 {
                let chance = ((hunger as f32).clamp(0.0, 100.0) / 100.0) * 0.03;
                if chance > 0.0 {
                    let mut rng = rand::thread_rng();
                    for _ in 0..sleep_rolls {
                        if rng.gen_bool(chance.min(1.0) as f64) {
                            pet_actor.request_state(ActorState::Sleep);
                            break;
                        }
                    }
                }
                }
            }
        }

        if !dragging
            && !ui_visible
            && pet_actor.state != ActorState::Walk
            && physics_body.is_active()
        {
            let (frame_path, need_flip) = pet_actor.current_frame_path_and_flip();
            let (body_l, body_t, body_r, body_b) =
                scene.opaque_bounds(frame_path, need_flip, pet_w, pet_h);

            let min_x = work_area.left as f32 - body_l as f32;
            let min_y = work_area.top as f32 - body_t as f32;
            let max_x = (work_area.right as f32 - body_r as f32).max(min_x);
            let max_y = (work_area.bottom as f32 - body_b as f32).max(min_y);
            let bounds = PhysicsBounds {
                min_x,
                min_y,
                max_x,
                max_y,
            };
            physics_body.step(delta, bounds);

            pet_mover.pos.x = physics_body.pos.x;
            pet_mover.pos.y = physics_body.pos.y;
            pet_mover.stop_at_current_pos();

            if physics_body.vel.x < -1.0 {
                pet_mover.facing = Facing::Left;
            } else if physics_body.vel.x > 1.0 {
                pet_mover.facing = Facing::Right;
            }
        } else if !dragging && !physics_body.is_active() {
            physics_body.sync_pos(pet_mover.pos.x, pet_mover.pos.y);
            physics_body.end_drag();
        }

        let mut ws_timer: Option<AutoTalkWsMsg> = None;
        let mut ws_pet_clicked: Option<AutoTalkWsMsg> = None;
        let mut ws_feed: Option<AutoTalkWsMsg> = None;
        let mut ws_level_up: Option<AutoTalkWsMsg> = None;
        loop {
            match auto_talk_rx.try_recv() {
                Ok(m) => match m.source.as_str() {
                    "level_up" => ws_level_up = Some(m),
                    "feed" => ws_feed = Some(m),
                    "pet_clicked" => ws_pet_clicked = Some(m),
                    _ => ws_timer = Some(m),
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        let ws_msg = if ws_level_up.is_some() {
            ws_level_up
        } else if ws_feed.is_some() {
            ws_feed
        } else if ws_pet_clicked.is_some() {
            ws_pet_clicked
        } else {
            ws_timer
        };
        if let Some(m) = ws_msg {
            let now = Instant::now();
            let send_ms = now.duration_since(m.recv_at).as_millis();

            if let Some(h) = m.hunger {
                if let Ok(mut s) = PET_STATS.lock() {
                    let h = h.clamp(0, 100);
                    if s.hunger != h {
                        s.hunger = h;
                        s.dirty = true;
                    }
                }
                if menu_visible {
                    let stats = PET_STATS
                        .lock()
                        .ok()
                        .map(|s| s.to_json().to_string())
                        .unwrap_or_else(|| "null".to_string());
                    web_menu.update_stats(&stats);
                }
            }

            let allow = match m.source.as_str() {
                "level_up" => true,
                "feed" => true,
                "pet_clicked" => last_pet_clicked_applied_at
                    .map(|t| now.duration_since(t) >= Duration::from_millis(2000))
                    .unwrap_or(true),
                _ => {
                    if suppress_default_phrase_until
                        .map(|t| now < t)
                        .unwrap_or(false)
                    {
                        false
                    } else {
                        last_timer_applied_at
                            .map(|t| now.duration_since(t) >= Duration::from_millis(2500))
                            .unwrap_or(true)
                    }
                }
            };
            if !allow {
                println!(
                    "[auto_talk] source={} dropped_by_cooldown send_ms={} ok={} err={}",
                    m.source, send_ms, auto_talk_ok, auto_talk_err
                );
            } else {
                let interval_ms = last_auto_talk_evt_at
                    .map(|t| now.duration_since(t).as_millis())
                    .unwrap_or(0);
                last_auto_talk_evt_at = Some(now);
                match m.source.as_str() {
                    "level_up" => {}
                    "feed" => {}
                    "pet_clicked" => last_pet_clicked_applied_at = Some(now),
                    _ => last_timer_applied_at = Some(now),
                }

                let mut text = m.text.clone();
                if m.source == "level_up" && text.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                    let level = m.level.unwrap_or(0);
                    let picked = pick_event_phrase(char_mod.personality.texts.as_ref(), "level_up");
                    if let Some(p) = picked {
                        text = Some(p.replace("{level}", &level.to_string()));
                    } else {
                        let tpl = char_mod
                            .personality
                            .texts
                            .as_ref()
                            .and_then(|t| t.level_up_template.clone())
                            .unwrap_or_else(|| "升级啦！现在是 {level} 级".to_string());
                        text = Some(tpl.replace("{level}", &level.to_string()));
                    }
                }
                if m.source == "pet_clicked"
                    && (text.as_ref().map(|s| s.is_empty()).unwrap_or(true) || !m.ok)
                {
                    text = pick_event_phrase(char_mod.personality.texts.as_ref(), "pet_clicked")
                        .or(text);
                }
                if m.source == "feed"
                    && (text.as_ref().map(|s| s.is_empty()).unwrap_or(true) || !m.ok)
                {
                    text = pick_event_phrase(char_mod.personality.texts.as_ref(), "feed").or(text);
                }

                if m.ok {
                    auto_talk_ok = auto_talk_ok.saturating_add(1);
                } else {
                    auto_talk_err = auto_talk_err.saturating_add(1);
                }
                if let Some(t) = text {
                    if !t.is_empty() {
                        println!(
                            "[auto_talk] source={} interval_ms={} send_ms={} ws_ok={} ok={} err={} ws_err={} text_len={}",
                            m.source,
                            interval_ms,
                            send_ms,
                            m.ok,
                            auto_talk_ok,
                            auto_talk_err,
                            m.error.clone().unwrap_or_default(),
                            t.len(),
                        );
                        pet_actor.enqueue_bubble_text(t);
                    }
                } else {
                    println!(
                        "[auto_talk] source={} interval_ms={} send_ms={} ws_ok={} ok={} err={} ws_err={} text_len=0",
                        m.source,
                        interval_ms,
                        send_ms,
                        m.ok,
                        auto_talk_ok,
                        auto_talk_err,
                        m.error.clone().unwrap_or_default(),
                    );
                }
            }
        }

        let update_result =
            pet_actor.update(&mut pet_mover, dragging, pet_stop_requested, delta, talk_trigger);
        let _ = update_result.started_auto_talk;
        if let Some(t) = update_result.bubble_text.clone() {
            if let Ok(mut g) = BUBBLE_TEXT.lock() {
                *g = Some(t);
            }
        } else if update_result.started_talk {
            let suppressed = suppress_default_phrase_until
                .map(|t| Instant::now() < t)
                .unwrap_or(false);
            if !suppressed {
                if let Ok(mut g) = BUBBLE_TEXT.lock() {
                    if g.is_none() {
                        let speech_style = char_mod.personality.speech_style.as_ref();
                        let base_phrase = speech_style
                            .and_then(|s| s.default_phrases.as_ref())
                            .filter(|v| !v.is_empty())
                            .map(|v| {
                                let mut rng = rand::thread_rng();
                                let idx = rng.gen_range(0..v.len());
                                v[idx].clone()
                            })
                            .or_else(|| CHARACTER_TEXTS.lock().ok().and_then(|t| t.hello.clone()))
                            .unwrap_or_else(|| "你好".to_string());

                        let mood_key = match pet_actor.state {
                            ActorState::Idle => "平静",
                            ActorState::Walk => "好奇",
                            ActorState::Relax => "好奇",
                            ActorState::Sleep => "平静",
                            ActorState::Drag => "轻微担忧",
                        };
                        let prefix = speech_style
                            .and_then(|s| s.prefix_by_mood.as_ref())
                            .and_then(|m| m.get(mood_key))
                            .cloned();

                        let phrase = if let Some(p) = prefix {
                            format!("{}{}", p, base_phrase)
                        } else {
                            base_phrase
                        };
                        *g = Some(phrase);
                    }
                }
            }
        }

        let bubble_show = pet_actor.talk_timer_ms > 0;
        let bubble_text = if bubble_show {
            BUBBLE_TEXT
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .or_else(|| CHARACTER_TEXTS.lock().ok().and_then(|t| t.hello.clone()))
                .unwrap_or_else(|| "你好".to_string())
        } else {
            String::new()
        };
        unsafe {
            scene.present_bubble(
                bubble_show,
                pet_mover.pos.x as i32,
                pet_mover.pos.y as i32,
                pet_w,
                pet_h,
                &bubble_text,
            );
        }

        let (base_frame_path, need_flip) = pet_actor.current_frame_path_and_flip();
        unsafe {
            scene.present_pet(
                hwnd,
                pet_mover.pos.x as i32,
                pet_mover.pos.y as i32,
                pet_w,
                pet_h,
                base_frame_path,
                need_flip,
            );
        }

        let overlays_active = pet_actor.talk_timer_ms > 0;
        let delay = Scene::tick_delay(ui_visible, dragging, overlays_active, &pet_mover.state);

        std::thread::sleep(delay);
    }

    unsafe {
        scene.destroy();
    }
}
