//! Interaction（桌面交互）
//!
//! 负责桌宠窗口与用户输入交互：
//! - mouse_drag：鼠标拖拽移动桌宠
//! - click_event：点击/双击/右键菜单等交互事件
//! - window_behavior：窗口置顶、穿透、吸附边缘、焦点策略等

use crate::ipc::{global_client, IpcMessage};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use webview2::{Controller, Environment, WebView};
use webview2_sys::Color;
use winapi::shared::windef::{HWND as HWND_WINAPI, RECT as RECT_WINAPI};
use winapi::um::wingdi::CreateRoundRectRgn as CreateRoundRectRgnWinapi;
use winapi::um::winuser::SetWindowRgn as SetWindowRgnWinapi;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HMONITOR, MONITORINFO};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, ShellExecuteW, HDROP};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos, GetSystemMetrics, GetWindowRect,
    PostQuitMessage, RegisterClassW, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, SW_HIDE,
    SW_SHOW, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WA_INACTIVE, WM_ACTIVATE, WM_APP,
    WM_CLOSE, WM_DROPFILES, WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{w, PCWSTR};

pub const WM_TRAYICON: u32 = WM_APP + 1;

pub const MENU_W: i32 = 196;
pub const MENU_H: i32 = 360;
pub const CHAT_W: i32 = 640;
pub const CHAT_H: i32 = 420;
pub const CHAT_MIN_W: i32 = 520;
pub const CHAT_MIN_H: i32 = 360;

static DRAGGING: AtomicBool = AtomicBool::new(false);
static DRAG_OFFSET_X: AtomicI32 = AtomicI32::new(0);
static DRAG_OFFSET_Y: AtomicI32 = AtomicI32::new(0);
static DRAG_POS_X: AtomicI32 = AtomicI32::new(0);
static DRAG_POS_Y: AtomicI32 = AtomicI32::new(0);
static CLICK_DOWN_WIN_X: AtomicI32 = AtomicI32::new(0);
static CLICK_DOWN_WIN_Y: AtomicI32 = AtomicI32::new(0);
static PET_CLICKED: AtomicBool = AtomicBool::new(false);
static MENU_REQUESTED: AtomicBool = AtomicBool::new(false);
static MENU_POS_X: AtomicI32 = AtomicI32::new(0);
static MENU_POS_Y: AtomicI32 = AtomicI32::new(0);
static CHAT_REQUESTED: AtomicBool = AtomicBool::new(false);
static CHAT_POS_X: AtomicI32 = AtomicI32::new(0);
static CHAT_POS_Y: AtomicI32 = AtomicI32::new(0);
static INTERACT_ACTION: AtomicI32 = AtomicI32::new(-1);

static FEED_REQUESTED: AtomicBool = AtomicBool::new(false);
static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);
static PLAY_REQUESTED: AtomicBool = AtomicBool::new(false);
static COINS_REFRESH_REQUESTED: AtomicBool = AtomicBool::new(false);

static BUBBLE_SINK: OnceLock<Arc<dyn Fn(String) + Send + Sync>> = OnceLock::new();

pub fn set_bubble_sink(sink: Arc<dyn Fn(String) + Send + Sync>) {
    let _ = BUBBLE_SINK.set(sink);
}

fn emit_bubble(text: &str) {
    if let Some(f) = BUBBLE_SINK.get() {
        f(text.to_string());
    }
}

pub fn take_feed_request() -> bool {
    FEED_REQUESTED.swap(false, Ordering::Relaxed)
}

pub fn take_reset_request() -> bool {
    RESET_REQUESTED.swap(false, Ordering::Relaxed)
}

pub fn take_play_request() -> bool {
    PLAY_REQUESTED.swap(false, Ordering::Relaxed)
}

pub fn take_coins_refresh_request() -> bool {
    COINS_REFRESH_REQUESTED.swap(false, Ordering::Relaxed)
}

pub fn is_dragging() -> bool {
    DRAGGING.load(Ordering::Relaxed)
}

pub fn drag_position() -> (i32, i32) {
    (
        DRAG_POS_X.load(Ordering::Relaxed),
        DRAG_POS_Y.load(Ordering::Relaxed),
    )
}

pub fn set_drag_position(x: i32, y: i32) {
    DRAG_POS_X.store(x, Ordering::Relaxed);
    DRAG_POS_Y.store(y, Ordering::Relaxed);
}

pub fn take_pet_clicked() -> bool {
    PET_CLICKED.swap(false, Ordering::Relaxed)
}

pub fn request_menu_at(x: i32, y: i32) {
    MENU_POS_X.store(x, Ordering::Relaxed);
    MENU_POS_Y.store(y, Ordering::Relaxed);
    MENU_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn take_menu_request() -> Option<(i32, i32)> {
    if MENU_REQUESTED.swap(false, Ordering::Relaxed) {
        Some((
            MENU_POS_X.load(Ordering::Relaxed),
            MENU_POS_Y.load(Ordering::Relaxed),
        ))
    } else {
        None
    }
}

pub fn request_chat_at(x: i32, y: i32) {
    CHAT_POS_X.store(x, Ordering::Relaxed);
    CHAT_POS_Y.store(y, Ordering::Relaxed);
    CHAT_REQUESTED.store(true, Ordering::Relaxed);
}

pub fn take_chat_request() -> Option<(i32, i32)> {
    if CHAT_REQUESTED.swap(false, Ordering::Relaxed) {
        Some((
            CHAT_POS_X.load(Ordering::Relaxed),
            CHAT_POS_Y.load(Ordering::Relaxed),
        ))
    } else {
        None
    }
}

pub fn set_interact_action(code: i32) {
    INTERACT_ACTION.store(code, Ordering::Relaxed);
}

pub fn take_interact_action() -> i32 {
    INTERACT_ACTION.swap(-1, Ordering::Relaxed)
}

pub fn open_url_in_browser(url: &str) {
    let mut wide: Vec<u16> = url.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let _ = ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOW);
    }
}

fn fetch_store_user_label() -> String {
    let msg = IpcMessage::new_request("store_user", "get", serde_json::json!({}));
    match global_client().send(&msg) {
        Ok(r) => {
            let dn = r
                .payload
                .get("display_name")
                .and_then(|x| x.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let uid = r
                .payload
                .get("user_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            dn.map(|s| s.to_string())
                .or_else(|| (!uid.is_empty()).then(|| uid.to_string()))
                .unwrap_or_default()
        }
        Err(_) => String::new(),
    }
}

fn pet_menu_script_inject(
    user_label: &str,
    chars: &str,
    cid: &str,
    skins: &str,
    cur: &str,
    stats: &str,
    invoke_render: bool,
) -> String {
    let uj = serde_json::to_string(user_label).unwrap_or_else(|_| "\"\"".to_string());
    let tail = if invoke_render {
        " window.__petRenderHeader && window.__petRenderHeader(); window.__petRenderRoles && window.__petRenderRoles(); window.__petRenderSkins && window.__petRenderSkins(); window.__petRenderStats && window.__petRenderStats();"
    } else {
        ""
    };
    format!(
        "window.__petUserLabel = {uj}; window.__petCharacters = {chars}; window.__petCharacterCurrent = {cid}; window.__petSkins = {skins}; window.__petSkinCurrent = {cur}; window.__petStats = {stats};{tail}",
        uj = uj,
        chars = chars,
        cid = cid,
        skins = skins,
        cur = cur,
        stats = stats,
        tail = tail,
    )
}

unsafe fn get_window_wh(hwnd: HWND) -> (i32, i32) {
    let mut rect = windows::Win32::Foundation::RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    (
        (rect.right - rect.left).max(1),
        (rect.bottom - rect.top).max(1),
    )
}

pub fn virtual_work_area() -> windows::Win32::Foundation::RECT {
    unsafe extern "system" fn enum_proc(
        hmon: HMONITOR,
        _hdc: windows::Win32::Graphics::Gdi::HDC,
        _rc: *mut windows::Win32::Foundation::RECT,
        lparam: LPARAM,
    ) -> windows::Win32::Foundation::BOOL {
        unsafe {
            let vec_ptr = lparam.0 as *mut Vec<windows::Win32::Foundation::RECT>;
            if vec_ptr.is_null() {
                return windows::Win32::Foundation::BOOL(0);
            }
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut info).as_bool() {
                (*vec_ptr).push(info.rcWork);
            }
            windows::Win32::Foundation::BOOL(1)
        }
    }

    let mut rects: Vec<windows::Win32::Foundation::RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            windows::Win32::Graphics::Gdi::HDC(0),
            None,
            Some(enum_proc),
            LPARAM(&mut rects as *mut _ as isize),
        );
    }

    if rects.is_empty() {
        unsafe {
            let x = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN);
            let h = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN);
            return windows::Win32::Foundation::RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
        }
    }

    let mut left = i32::MAX;
    let mut right = i32::MIN;
    let mut top = i32::MIN;
    let mut bottom = i32::MIN;
    for r in rects {
        left = left.min(r.left);
        right = right.max(r.right);
        top = top.max(r.top);
        bottom = bottom.max(r.bottom);
    }
    windows::Win32::Foundation::RECT {
        left,
        top,
        right,
        bottom,
    }
}

fn place_popup_in_work_area(
    anchor_x: i32,
    anchor_y: i32,
    popup_w: i32,
    popup_h: i32,
    work_area: windows::Win32::Foundation::RECT,
) -> (i32, i32) {
    let mut x = anchor_x;
    let mut y = anchor_y;

    if x + popup_w > work_area.right {
        x = anchor_x - popup_w;
    }
    if y + popup_h > work_area.bottom {
        y = anchor_y - popup_h;
    }

    let max_x = work_area.right - popup_w;
    let max_y = work_area.bottom - popup_h;

    if max_x < work_area.left {
        x = work_area.left;
    } else {
        x = x.clamp(work_area.left, max_x);
    }
    if max_y < work_area.top {
        y = work_area.top;
    } else {
        y = y.clamp(work_area.top, max_y);
    }

    (x, y)
}

fn place_popup_centered_in_work_area(
    popup_w: i32,
    popup_h: i32,
    work_area: windows::Win32::Foundation::RECT,
) -> (i32, i32) {
    let work_w = (work_area.right - work_area.left).max(1);
    let work_h = (work_area.bottom - work_area.top).max(1);

    let mut x = work_area.left + (work_w - popup_w) / 2;
    let mut y = work_area.top + (work_h - popup_h) / 2;

    let max_x = work_area.right - popup_w;
    let max_y = work_area.bottom - popup_h;

    if max_x < work_area.left {
        x = work_area.left;
    } else {
        x = x.clamp(work_area.left, max_x);
    }

    if max_y < work_area.top {
        y = work_area.top;
    } else {
        y = y.clamp(work_area.top, max_y);
    }

    (x, y)
}

pub struct WebMenu {
    hwnd: HWND,
    controller: Rc<RefCell<Option<Controller>>>,
    webview: Rc<RefCell<Option<WebView>>>,
    pending_show: Rc<RefCell<Option<(i32, i32)>>>,
    pending_script: Rc<RefCell<Option<String>>>,
}

pub struct WebChat {
    hwnd: HWND,
    controller: Rc<RefCell<Option<Controller>>>,
    webview: Rc<RefCell<Option<WebView>>>,
    pending_show: Rc<RefCell<Option<(i32, i32)>>>,
}

impl WebMenu {
    pub unsafe fn new(hinstance: windows::Win32::Foundation::HINSTANCE) -> Self {
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
            pending_script: Rc::new(RefCell::new(None)),
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub unsafe fn init(&mut self, menu_html: &'static str, backend_url: String) {
        let menu_hwnd_windows = self.hwnd;
        let menu_hwnd: HWND_WINAPI = menu_hwnd_windows.0 as HWND_WINAPI;
        let controller_cell_env = self.controller.clone();
        let webview_cell_env = self.webview.clone();
        let pending_show_env = self.pending_show.clone();
        let pending_script_env = self.pending_script.clone();

        let _ = Environment::builder().build(move |env| {
            let env = env?;
            let controller_cell_controller = controller_cell_env.clone();
            let webview_cell_controller = webview_cell_env.clone();
            let pending_show_controller = pending_show_env.clone();
            let pending_script_controller = pending_script_env.clone();
            let menu_hwnd2 = menu_hwnd_windows;
            let backend_url2 = backend_url.clone();

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
                webview.navigate_to_string(menu_html)?;

                webview.add_web_message_received(move |_w, msg| {
                    let msg = msg.try_get_web_message_as_string()?;
                    if let Some(act) = msg.strip_prefix("act:") {
                        let code = match act {
                            "idle" => 0,
                            "walk" => 1,
                            "relax" => 2,
                            "sleep" => 3,
                            "drag" => 4,
                            _ => -1,
                        };
                        if code >= 0 {
                            set_interact_action(code);
                        }
                    } else if let Some(skin) = msg.strip_prefix("skin:") {
                        crate::mod_loader::request_skin(skin.to_string());
                    } else if let Some(id) = msg.strip_prefix("character:") {
                        if id == "add" {
                            let open_url = if backend_url2.ends_with('/') {
                                format!("{backend_url2}?page=characters")
                            } else {
                                format!("{backend_url2}/?page=characters")
                            };
                            open_url_in_browser(&open_url);
                        } else {
                            crate::mod_loader::request_character(id.to_string());
                        }
                    } else if let Some(coords) = msg.strip_prefix("menu:move:") {
                        let mut parts = coords.split(':');
                        if let (Some(xs), Some(ys)) = (parts.next(), parts.next()) {
                            if let (Ok(x), Ok(y)) = (xs.parse::<i32>(), ys.parse::<i32>()) {
                                let _ = SetWindowPos(menu_hwnd2, HWND(0), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
                            }
                        }
                    } else {
                        match msg.as_str() {
                            "talk" => {
                                let mut pt = POINT::default();
                                let _ = GetCursorPos(&mut pt);
                                request_chat_at(pt.x, pt.y);
                            }
                            "feed" => {
                                FEED_REQUESTED.store(true, Ordering::Relaxed);
                            }
                            "play" => {
                                PLAY_REQUESTED.store(true, Ordering::Relaxed);
                            }
                            "coins_refresh" => {
                                COINS_REFRESH_REQUESTED.store(true, Ordering::Relaxed);
                            }
                            "reset" => {
                                RESET_REQUESTED.store(true, Ordering::Relaxed);
                            }
                            "open_backend" => {
                                open_url_in_browser(&backend_url2);
                            }
                            "exit" => PostQuitMessage(0),
                            "settings" => {}
                            "close" => {}
                            _ => {}
                        }
                    }
                    let _ = ShowWindow(menu_hwnd2, SW_HIDE);
                    Ok(())
                })?;

                *controller_cell_controller.borrow_mut() = Some(controller.clone());
                *webview_cell_controller.borrow_mut() = Some(webview.clone());

                if let Some(s) = pending_script_controller.borrow_mut().take() {
                    let _ = webview.execute_script(&s, |_| Ok(()));
                }

                if let Some((x, y)) = pending_show_controller.borrow_mut().take() {
                    let work_area = virtual_work_area();
                    let (pos_x, pos_y) = place_popup_in_work_area(x, y, MENU_W, MENU_H, work_area);
                    let _ = SetWindowPos(
                        menu_hwnd2,
                        HWND(0),
                        pos_x,
                        pos_y,
                        MENU_W,
                        MENU_H,
                        SWP_NOZORDER | SWP_SHOWWINDOW,
                    );
                    let _ = ShowWindow(menu_hwnd2, SW_SHOW);
                }

                Ok(())
            })
        });
    }

    pub fn prepare_payload(
        &self,
        characters_json: &str,
        current_character_json: &str,
        skins_json: &str,
        current_skin_json: &str,
        stats_json: &str,
        invoke_render: bool,
    ) {
        let user_label = fetch_store_user_label();
        let script = pet_menu_script_inject(
            &user_label,
            characters_json,
            current_character_json,
            skins_json,
            current_skin_json,
            stats_json,
            invoke_render,
        );

        if let Some(wv) = self.webview.borrow().as_ref() {
            let _ = wv.execute_script(&script, |_| Ok(()));
        } else {
            *self.pending_script.borrow_mut() = Some(script);
        }
    }

    pub fn update_stats(&self, stats_json: &str) {
        if let Some(wv) = self.webview.borrow().as_ref() {
            let _ = wv.execute_script(
                &format!(
                    "window.__petStats = {stats}; window.__petRenderStats && window.__petRenderStats();",
                    stats = stats_json
                ),
                |_| Ok(()),
            );
        }
    }

    pub unsafe fn show_at(&self, x: i32, y: i32) {
        if self.controller.borrow().is_none() {
            *self.pending_show.borrow_mut() = Some((x, y));
            return;
        }
        let work_area = virtual_work_area();
        let (pos_x, pos_y) = place_popup_in_work_area(x, y, MENU_W, MENU_H, work_area);
        let _ = SetWindowPos(
            self.hwnd,
            HWND(0),
            pos_x,
            pos_y,
            MENU_W,
            MENU_H,
            SWP_NOZORDER | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(self.hwnd, SW_SHOW);
    }
}

impl WebChat {
    pub unsafe fn new(hinstance: windows::Win32::Foundation::HINSTANCE) -> Self {
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
            WS_POPUP | windows::Win32::UI::WindowsAndMessaging::WS_THICKFRAME,
            0,
            0,
            CHAT_W,
            CHAT_H,
            None,
            None,
            hinstance,
            None,
        );

        let rgn = CreateRoundRectRgnWinapi(0, 0, CHAT_W, CHAT_H, 34, 34);
        let _ = SetWindowRgnWinapi(hwnd.0 as HWND_WINAPI, rgn, 1);
        DragAcceptFiles(hwnd, true);

        Self {
            hwnd,
            controller: Rc::new(RefCell::new(None)),
            webview: Rc::new(RefCell::new(None)),
            pending_show: Rc::new(RefCell::new(None)),
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub unsafe fn init(
        &mut self,
        chat_html: &'static str,
        ai_tx: Sender<crate::scripting::AiRequest>,
        personality_provider: Arc<dyn Fn() -> Option<serde_json::Value> + Send + Sync>,
    ) {
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
            let ai_tx2 = ai_tx.clone();
            let personality_provider2 = personality_provider.clone();

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
                webview.navigate_to_string(chat_html)?;

                webview.add_web_message_received(move |_w, msg| {
                    let msg = msg.try_get_web_message_as_string()?;
                    if msg == "close" {
                        let _ = ShowWindow(chat_hwnd2, SW_HIDE);
                        return Ok(());
                    }

                    if let Some(coords) = msg.strip_prefix("chat:move:") {
                        let mut parts = coords.split(':');
                        if let (Some(xs), Some(ys)) = (parts.next(), parts.next()) {
                            if let (Ok(x), Ok(y)) = (xs.parse::<i32>(), ys.parse::<i32>()) {
                                let _ = SetWindowPos(chat_hwnd2, HWND(0), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
                            }
                        }
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
                                let personality = personality_provider2();
                                let _ = ai_tx2.send(crate::scripting::AiRequest {
                                    user_text: text,
                                    personality,
                                });
                            }
                        }
                    }
                    Ok(())
                })?;

                *controller_cell_controller.borrow_mut() = Some(controller.clone());
                *webview_cell_controller.borrow_mut() = Some(webview.clone());

                if let Some((_x, _y)) = pending_show_controller.borrow_mut().take() {
                    let (cur_w, cur_h) = get_window_wh(chat_hwnd2);
                    let work_area = virtual_work_area();
                    let (pos_x, pos_y) = place_popup_centered_in_work_area(cur_w, cur_h, work_area);
                    let _ = SetWindowPos(
                        chat_hwnd2,
                        HWND(0),
                        pos_x,
                        pos_y,
                        cur_w,
                        cur_h,
                        SWP_NOZORDER | SWP_SHOWWINDOW,
                    );
                    let _ = ShowWindow(chat_hwnd2, SW_SHOW);
                }

                Ok(())
            })
        });
    }

    pub unsafe fn show_at(&self, x: i32, y: i32) {
        let _ = (x, y);
        if self.controller.borrow().is_none() {
            *self.pending_show.borrow_mut() = Some((x, y));
            return;
        }

        let (cur_w, cur_h) = get_window_wh(self.hwnd);
        let work_area = virtual_work_area();
        let (pos_x, pos_y) = place_popup_centered_in_work_area(cur_w, cur_h, work_area);
        let _ = SetWindowPos(
            self.hwnd,
            HWND(0),
            pos_x,
            pos_y,
            cur_w,
            cur_h,
            SWP_NOZORDER | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(self.hwnd, SW_SHOW);
    }

    pub fn push_ai_reply(&self, reply: &str) {
        if let Some(wv) = self.webview.borrow().as_ref() {
            let payload = serde_json::json!({ "type": "ai", "text": reply });
            let json_str = payload.to_string();
            let _ = wv.post_web_message_as_json(&json_str);
        }
    }

    pub fn on_visible_tick(&self) {
        unsafe {
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = GetClientRect(self.hwnd, &mut rc);
            let w = (rc.right - rc.left).max(1);
            let h = (rc.bottom - rc.top).max(1);
            if let Some(c) = self.controller.borrow().as_ref() {
                let rect = RECT_WINAPI {
                    left: 0,
                    top: 0,
                    right: w,
                    bottom: h,
                };
                let _ = c.put_bounds(rect);
            }
            let rgn = CreateRoundRectRgnWinapi(0, 0, w, h, 34, 34);
            let _ = SetWindowRgnWinapi(self.hwnd.0 as HWND_WINAPI, rgn, 1);
        }
    }
}

extern "system" fn menu_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            x if x == WM_ACTIVATE => {
                let state = (wparam.0 as u32) & 0xFFFF;
                if state == WA_INACTIVE {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
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
            WM_DROPFILES => {
                let hdrop = HDROP(lparam.0);
                let count = DragQueryFileW(hdrop, 0xFFFFFFFF, Some(&mut []));
                if count > 0 {
                    let ipc_client = global_client();
                    for i in 0..count {
                        let len = DragQueryFileW(hdrop, i, Some(&mut [])) as usize;
                        if len == 0 {
                            continue;
                        }
                        let mut buf = vec![0u16; len + 1];
                        let got = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
                        if got > 0 {
                            let path = String::from_utf16_lossy(&buf[..got]);
                            let msg = IpcMessage::new_request(
                                "file",
                                "summarize",
                                serde_json::json!({ "path": path }),
                            );
                            let _ = ipc_client.send(&msg);
                        }
                    }
                    emit_bubble("已提交文件总结");
                }
                DragFinish(hdrop);
                LRESULT(0)
            }
            WM_NCHITTEST => {
                let x = (lparam.0 & 0xFFFF) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let mut rect = windows::Win32::Foundation::RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                let border = 10;
                let title_h = 52;
                if x >= rect.left + border
                    && x <= rect.right - border
                    && y >= rect.top + border
                    && y <= rect.top + title_h
                {
                    return LRESULT(windows::Win32::UI::WindowsAndMessaging::HTCAPTION as isize);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_GETMINMAXINFO => {
                let info = lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::MINMAXINFO;
                if !info.is_null() {
                    (*info).ptMinTrackSize.x = CHAT_MIN_W;
                    (*info).ptMinTrackSize.y = CHAT_MIN_H;
                }
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

pub unsafe fn handle_wnd_message(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match msg {
        WM_LBUTTONDOWN => {
            let _ = SetCapture(hwnd);
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mut rect = windows::Win32::Foundation::RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);

            CLICK_DOWN_WIN_X.store(rect.left, Ordering::Relaxed);
            CLICK_DOWN_WIN_Y.store(rect.top, Ordering::Relaxed);
            DRAG_OFFSET_X.store(pt.x - rect.left, Ordering::Relaxed);
            DRAG_OFFSET_Y.store(pt.y - rect.top, Ordering::Relaxed);
            DRAG_POS_X.store(rect.left, Ordering::Relaxed);
            DRAG_POS_Y.store(rect.top, Ordering::Relaxed);
            DRAGGING.store(true, Ordering::Relaxed);
            Some(LRESULT(0))
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
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            let mut rect = windows::Win32::Foundation::RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            let sx = CLICK_DOWN_WIN_X.load(Ordering::Relaxed);
            let sy = CLICK_DOWN_WIN_Y.load(Ordering::Relaxed);
            let dx = (rect.left - sx).abs();
            let dy = (rect.top - sy).abs();
            if dx <= 4 && dy <= 4 {
                PET_CLICKED.store(true, Ordering::Relaxed);
            }
            DRAGGING.store(false, Ordering::Relaxed);
            let _ = ReleaseCapture();
            Some(LRESULT(0))
        }
        WM_RBUTTONUP => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            request_menu_at(pt.x, pt.y);
            Some(LRESULT(0))
        }
        WM_TRAYICON => {
            let mouse_msg = lparam.0 as u32;
            if mouse_msg == WM_RBUTTONUP {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                request_menu_at(pt.x, pt.y);
                Some(LRESULT(0))
            } else {
                Some(DefWindowProcW(hwnd, msg, wparam, lparam))
            }
        }
        _ => None,
    }
}
