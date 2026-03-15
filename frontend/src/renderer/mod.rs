//! Renderer（CPU 渲染）
//!
//! 目标：完全基于 CPU 的 RGBA 帧渲染，并在 Windows 上通过 Layered Window 展示透明桌面窗口动画。
//!
//! 规划职责：
//! - layered_renderer：封装 UpdateLayeredWindow 的提交逻辑（BGRA + 预乘 Alpha）
//! - frame_buffer：帧缓冲与像素格式转换（RGBA -> BGRA、预乘处理）
//! - animation_system：面向渲染侧的播放驱动（按 FPS/时间推进帧）

