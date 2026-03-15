//! Animation（动画）
//!
//! 规划职责：
//! - 表示一个动画 clip（若干 Frame + fps + loop 策略）
//! - 可选：不同状态下的变体、随机权重、过渡规则

use super::frame::Frame;

pub struct Animation {
    pub frames: Vec<Frame>,
    pub fps: u32,
    pub looped: bool,
}
