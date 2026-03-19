use std::ptr::null_mut;

use winapi::shared::windef::{HDC as HDC_WINAPI, RECT as RECT_WINAPI};
use winapi::um::winuser::{
    DrawTextW as DrawTextWWinapi, DT_LEFT, DT_NOPREFIX, DT_TOP, DT_WORDBREAK,
};
use windows::Win32::{
    Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM},
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetStockObject, SelectObject,
        SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        BLENDFUNCTION, DEFAULT_GUI_FONT, DIB_RGB_COLORS, HBITMAP, HDC, TRANSPARENT,
    },
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, ShowWindow,
        UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, SW_HIDE, SW_SHOW, ULW_ALPHA, WM_CLOSE,
        WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    },
};

pub struct LayeredSurface {
    mem_dc: HDC,
    dib: HBITMAP,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl LayeredSurface {
    pub unsafe fn new(screen_dc: HDC, w: i32, h: i32) -> Self {
        let mem_dc = CreateCompatibleDC(screen_dc);
        let (dib, bits) = create_dib(mem_dc, w, h);
        let _ = SelectObject(mem_dc, dib);
        Self {
            mem_dc,
            dib,
            bits: bits as *mut u8,
            w,
            h,
        }
    }

    pub unsafe fn resize(&mut self, w: i32, h: i32) {
        if self.w == w && self.h == h {
            return;
        }
        let (new_dib, new_bits) = create_dib(self.mem_dc, w, h);
        let _ = SelectObject(self.mem_dc, new_dib);
        let _ = DeleteObject(self.dib);
        self.dib = new_dib;
        self.bits = new_bits as *mut u8;
        self.w = w;
        self.h = h;
    }

    pub unsafe fn present(
        &mut self,
        hwnd: HWND,
        screen_dc: HDC,
        dst_x: i32,
        dst_y: i32,
        bgra: Option<&[u8]>,
    ) {
        if let Some(bgra) = bgra {
            let len = (self.w * self.h * 4).max(0) as usize;
            if bgra.len() == len {
                std::ptr::copy_nonoverlapping(bgra.as_ptr(), self.bits, len);
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
            hwnd,
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

    pub unsafe fn destroy(&mut self) {
        let _ = DeleteObject(self.dib);
        let _ = DeleteDC(self.mem_dc);
        self.bits = null_mut();
    }
}

pub struct ChatBubble {
    hwnd: HWND,
    w: i32,
    h: i32,
    mem_dc: HDC,
    dib: HBITMAP,
    bits: *mut u8,
    visible: bool,
}

impl ChatBubble {
    pub unsafe fn new(hinstance: HINSTANCE, screen_dc: HDC, w: i32, h: i32) -> Self {
        let class_name = windows::core::w!("DesktopPetBubbleClass");
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
            windows::core::w!("DesktopPetBubble"),
            WS_POPUP,
            0,
            0,
            w,
            h,
            None,
            None,
            hinstance,
            None,
        );

        let mem_dc = CreateCompatibleDC(screen_dc);
        let (dib, bits) = create_dib(mem_dc, w, h);
        let _ = SelectObject(mem_dc, dib);

        Self {
            hwnd,
            w,
            h,
            mem_dc,
            dib,
            bits: bits as *mut u8,
            visible: false,
        }
    }

    pub unsafe fn set_visible(&mut self, show: bool) {
        if show && !self.visible {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                self.hwnd,
                windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST,
                0,
                0,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            self.visible = true;
        } else if !show && self.visible {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            self.visible = false;
        }
    }

    pub unsafe fn render_and_present(
        &mut self,
        screen_dc: HDC,
        dst_x: i32,
        dst_y: i32,
        tail_x: i32,
        tail_up: bool,
        text: &str,
    ) {
        let len = (self.w * self.h * 4) as usize;
        let buf = std::slice::from_raw_parts_mut(self.bits, len);
        draw_chat_bubble_bg(buf, self.w, self.h, tail_x, tail_up);

        let mut before = vec![0_u8; len];
        before.copy_from_slice(buf);

        let old_font = SelectObject(self.mem_dc, GetStockObject(DEFAULT_GUI_FONT));
        let _ = SetBkMode(self.mem_dc, TRANSPARENT);
        let _ = SetTextColor(self.mem_dc, COLORREF(0x00FFFFFF));

        let tail_h = 10;
        let mut text_rect = RECT_WINAPI {
            left: 12,
            top: if tail_up { 8 + tail_h } else { 8 },
            right: self.w - 12,
            bottom: if tail_up { self.h - 14 } else { self.h - 14 - tail_h },
        };

        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        let _ = DrawTextWWinapi(
            self.mem_dc.0 as HDC_WINAPI,
            wide.as_ptr(),
            -1,
            &mut text_rect,
            DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX,
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
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            self.hwnd,
            windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST,
            dst_x,
            dst_y,
            0,
            0,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                | windows::Win32::UI::WindowsAndMessaging::SWP_SHOWWINDOW,
        );
    }

    pub unsafe fn destroy(&mut self) {
        let _ = ShowWindow(self.hwnd, SW_HIDE);
        let _ = DestroyWindow(self.hwnd);
        let _ = DeleteObject(self.dib);
        let _ = DeleteDC(self.mem_dc);
        self.bits = null_mut();
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

fn draw_chat_bubble_bg(bgra: &mut [u8], w: i32, h: i32, tail_x: i32, tail_up: bool) {
    bgra.fill(0);

    let tail_h = 10;
    let (body_top, body_bottom) = if tail_up { (tail_h, h) } else { (0, h - tail_h) };
    let body_h = body_bottom - body_top;
    let radius = 12;
    let pad = 1;

    let bg = (30_u8, 30_u8, 30_u8, 220_u8);
    let border = (255_u8, 255_u8, 255_u8, 40_u8);

    for y in body_top..body_bottom {
        let y_body = y - body_top;
        for x in 0..w {
            let inside_outer = inside_round_rect(x, y_body, w, body_h, radius);
            if !inside_outer {
                continue;
            }

            let inside_inner = inside_round_rect(
                x - pad,
                y_body - pad,
                w - pad * 2,
                body_h - pad * 2,
                radius - pad,
            );
            let (r, g, b, a) = if inside_inner { bg } else { border };

            let idx = ((y * w + x) * 4) as usize;
            bgra[idx] = premul_u8(b, a);
            bgra[idx + 1] = premul_u8(g, a);
            bgra[idx + 2] = premul_u8(r, a);
            bgra[idx + 3] = a;
        }
    }

    let tail_center_x = tail_x.clamp(16, w - 16);
    if tail_up {
        let tail_top = 0;
        let tail_bottom = tail_h - 1;
        for y in tail_top..=tail_bottom {
            let t = y as f32 / tail_h as f32;
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
    } else {
        let tail_top = body_bottom - 1;
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
}

extern "system" fn bubble_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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

fn create_dib(mem_dc: HDC, w: i32, h: i32) -> (HBITMAP, *mut core::ffi::c_void) {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = null_mut();
    let dib = unsafe { CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }
        .expect("CreateDIBSection failed");
    (dib, bits)
}
