//! Animation Editor（动作编辑器）
//!
//! 目标：为 MOD 作者提供可视化工具链（导入帧、调 FPS、导出角色包），降低制作门槛。
//!
//! 规划职责：
//! - import_pipeline：导入 PNG 帧与预览（排序、裁剪、对齐）
//! - timeline：时间轴与帧率编辑、循环区间
//! - export：导出 animations/ 与角色包清单（可选压缩为单文件格式）

