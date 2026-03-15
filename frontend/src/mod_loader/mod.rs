//! Mod Loader（MOD 系统）
//!
//! 目标：支持加载“角色 MOD”和“功能 MOD”，形成可扩展生态。
//!
//! 规划职责：
//! - discovery：扫描 mods/ 与 characters/（本地目录或安装目录）
//! - manifest：读取 mod.json/character.json（版本、依赖、权限声明）
//! - install_uninstall：安装、卸载、启用/禁用、冲突检测与回滚

