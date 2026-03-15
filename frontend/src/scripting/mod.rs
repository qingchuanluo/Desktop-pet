//! Scripting（角色行为脚本系统）
//!
//! 目标：把角色行为逻辑从代码中抽离，使用脚本定义“何时播放什么动作/如何响应事件”，便于 MOD 作者定制。
//!
//! 规划职责：
//! - script_runtime：脚本运行时（Lua 等）与调用边界
//! - api_surface：暴露给脚本的安全 API（play_animation、emit_event、get_state 等）
//! - hot_reload：脚本热重载与错误隔离（脚本崩溃不影响主程序）

