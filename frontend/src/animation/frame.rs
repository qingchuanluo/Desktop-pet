//! Frame（单帧）
//!
//! 规划职责：
//! - 表示一帧动画数据（像素帧/精灵帧/引用资源路径等）
//! - 可选：热点（pivot）、碰撞框、事件点（event markers）

pub struct Frame {
    pub path: String,
}
