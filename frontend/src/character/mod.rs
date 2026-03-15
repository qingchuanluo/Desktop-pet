//! Character（角色资产系统）
//!
//! 目标：把桌宠角色从“写死在代码里”升级为“可导入的角色包”，支持用户直接替换/安装角色 MOD。
//!
//! 规划职责：
//! - package_format：角色包结构与清单（character.json、animations/、ai/、voice/、scripts/ 等）
//! - loader：从目录或压缩包读取、校验、版本兼容检查
//! - runtime_binding：把角色资源绑定到动画/状态机/AI/语音等运行时系统

