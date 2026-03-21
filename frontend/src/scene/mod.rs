//! 场景层（Scene）
//!
//! 用来组织“现在画什么/处于什么状态/如何响应输入”：
//! - 桌宠贴图/动画的绘制入口
//! - 命中测试（透明像素穿透需要）
//! - 行为状态与渲染状态的桥接（和 pet/animation 配合）

use crate::render::FrameComposer;
use crate::renderer::{ChatBubble, LayeredSurface};
use crate::state_machine::MoverState;
use std::time::Duration;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC, HDC};
use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;

pub struct Scene {
    screen_dc: HDC,
    surface: LayeredSurface,
    bubble: ChatBubble,
    composer: FrameComposer,
    bubble_w: i32,
    bubble_h: i32,
}

impl Scene {
    pub unsafe fn new(hinstance: HINSTANCE, pet_w: i32, pet_h: i32, bubble_w: i32, bubble_h: i32) -> Self {
        let screen_dc = GetDC(HWND(0));
        let surface = LayeredSurface::new(screen_dc, pet_w, pet_h);
        let bubble = ChatBubble::new(hinstance, screen_dc, bubble_w, bubble_h);
        Self {
            screen_dc,
            surface,
            bubble,
            composer: FrameComposer::new(),
            bubble_w,
            bubble_h,
        }
    }

    pub fn reset_composer(&mut self) {
        self.composer.reset();
    }

    pub fn opaque_bounds(
        &mut self,
        base_frame_path: &str,
        need_flip: bool,
        target_w: i32,
        target_h: i32,
    ) -> (u32, u32, u32, u32) {
        self.composer
            .opaque_bounds(base_frame_path, need_flip, target_w, target_h)
    }

    pub unsafe fn resize_pet_surface(&mut self, pet_w: i32, pet_h: i32) {
        self.surface.resize(pet_w, pet_h);
    }

    pub unsafe fn present_pet(
        &mut self,
        hwnd: HWND,
        pet_x: i32,
        pet_y: i32,
        pet_w: i32,
        pet_h: i32,
        base_frame_path: &str,
        need_flip: bool,
    ) {
        let bgra = self
            .composer
            .compose_bgra(base_frame_path, need_flip, pet_w, pet_h);
        self.surface
            .present(hwnd, self.screen_dc, pet_x, pet_y, bgra.as_deref());
    }

    pub unsafe fn present_bubble(
        &mut self,
        show: bool,
        pet_x: i32,
        pet_y: i32,
        pet_w: i32,
        pet_h: i32,
        text: &str,
    ) {
        if !show {
            self.bubble.set_visible(false);
            return;
        }

        let screen_x = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_XVIRTUALSCREEN);
        let screen_y = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_YVIRTUALSCREEN);
        let screen_w = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXVIRTUALSCREEN);
        let screen_h = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYVIRTUALSCREEN);

        let pet_center_x = pet_x + pet_w / 2;
        let max_x = screen_x + (screen_w - self.bubble_w).max(0);
        let max_y = screen_y + (screen_h - self.bubble_h).max(0);

        let preferred_x = pet_center_x - self.bubble_w / 2;
        let bubble_x = if preferred_x < screen_x {
            (pet_x + pet_w + 8).clamp(screen_x, max_x)
        } else if preferred_x > max_x {
            (pet_x - self.bubble_w - 8).clamp(screen_x, max_x)
        } else {
            preferred_x
        };

        let preferred_y = pet_y - self.bubble_h - 8;
        let bubble_below = preferred_y < screen_y;
        let bubble_y = if bubble_below {
            (pet_y + pet_h + 8).clamp(screen_y, max_y)
        } else {
            preferred_y.clamp(screen_y, max_y)
        };

        let tail_x = (pet_center_x - bubble_x).clamp(16, self.bubble_w - 16);

        self.bubble.render_and_present(
            self.screen_dc,
            bubble_x,
            bubble_y,
            tail_x,
            bubble_below,
            text,
        );
        self.bubble.set_visible(true);
    }

    pub fn tick_delay(
        ui_visible: bool,
        dragging: bool,
        overlays_active: bool,
        mover_state: &MoverState,
    ) -> Duration {
        if ui_visible || dragging {
            Duration::from_millis(1000 / 60)
        } else if overlays_active {
            Duration::from_millis(1000 / 15)
        } else {
            match mover_state {
                MoverState::Moving => Duration::from_millis(1000 / 12),
                MoverState::Resting { .. } => Duration::from_millis(1000 / 6),
            }
        }
    }

    pub unsafe fn destroy(&mut self) {
        self.bubble.destroy();
        self.surface.destroy();
        let _ = ReleaseDC(HWND(0), self.screen_dc);
    }
}
