//! 桌宠领域模型（Pet）
//!
//! 负责“桌宠是什么、有哪些状态、如何切换”：
//! - 状态机：Idle / Hover / Dragging / ClickReact / Sleep ...
//! - 状态到动作的映射：state -> animation clip
//! - 与 IPC 协作：后台触发动作/情绪/台词，前台回传行为事件

