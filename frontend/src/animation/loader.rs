//! Loader（动画加载器）
//!
//! 规划职责：
//! - 从角色包/资源目录加载动画帧列表（Frame.path）
//! - 读取动画配置（fps、looped）
//! - 按文件名排序帧（0.png/1.png/... 或自定义规则）

use super::animation::Animation;
use super::frame::Frame;
use std::fs;

pub fn load_animation(dir: &str, fps: u32, looped: bool) -> Animation {
    let mut frames = Vec::new();

    let entries = fs::read_dir(dir).unwrap();

    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();

        if let Some(ext) = path.extension() {
            if ext == "png" {
                let frame = Frame {
                    path: path.to_str().unwrap().to_string(),
                };

                frames.push(frame);
            }
        }
    }

    frames.sort_by(|a, b| a.path.cmp(&b.path));

    Animation {
        frames,
        fps,
        looped,
    }
}
