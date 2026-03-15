//! Extension System（可扩展模块系统）
//!
//! 目标：把“核心桌宠能力”和“可选扩展能力”解耦，核心只提供稳定的运行时与接口，
//! 其余功能（AI、语音、统计、物理等）以扩展模块形式接入。
//!
//! 规划职责：
//! - module_loader：发现与加载模块（内置模块/外部模块）
//! - module_registry：模块注册表（按 name/version 能力索引）
//! - lifecycle：生命周期（load/update/unload）与依赖顺序

