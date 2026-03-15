//! 动画系统（Animation）
//!
//! 负责“按时间推进帧序列”：
//! - clip：帧序列 + fps + loop 策略
//! - animator：当前动作、当前帧、切动作的过渡策略（立即/播完再切）
//! - 时间源：由 app 层提供 delta time / tick

pub mod animation;
pub mod animation_player;
pub mod frame;
pub mod loader;

pub use animation_player::AnimationPlayer;
