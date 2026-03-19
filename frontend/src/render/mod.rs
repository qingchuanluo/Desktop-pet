use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use image::RgbaImage;

const MAX_FRAME_CACHE: usize = 96;

pub struct FrameComposer {
    cache: HashMap<String, Arc<RgbaImage>>,
    cache_order: VecDeque<String>,
    opaque_bounds_base_cache: HashMap<String, (u32, u32, u32, u32)>,
    opaque_bounds_cache: HashMap<String, (u32, u32, u32, u32)>,
    last_key: String,
}

impl FrameComposer {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            opaque_bounds_base_cache: HashMap::new(),
            opaque_bounds_cache: HashMap::new(),
            last_key: String::new(),
        }
    }

    pub fn reset(&mut self) {
        self.cache.clear();
        self.cache_order.clear();
        self.opaque_bounds_base_cache.clear();
        self.opaque_bounds_cache.clear();
        self.last_key.clear();
    }

    pub fn opaque_bounds(
        &mut self,
        base_frame_path: &str,
        need_flip: bool,
        target_w: i32,
        target_h: i32,
    ) -> (u32, u32, u32, u32) {
        let w = target_w.max(1) as u32;
        let h = target_h.max(1) as u32;
        let key = format!(
            "{}|{}|{}x{}|ob",
            base_frame_path,
            if need_flip { "F" } else { "" },
            w,
            h
        );
        if let Some(v) = self.opaque_bounds_cache.get(&key) {
            return *v;
        }

        let base_img = load_cached_rgba(&mut self.cache, &mut self.cache_order, base_frame_path);
        let base_w = base_img.width();
        let base_h = base_img.height();
        let base_bounds = if let Some(v) = self.opaque_bounds_base_cache.get(base_frame_path) {
            Some(*v)
        } else {
            let v = find_opaque_bounds(&base_img, 8);
            if let Some(b) = v {
                self.opaque_bounds_base_cache
                    .insert(base_frame_path.to_string(), b);
            }
            v
        };

        let Some((mut l, mut t, mut r, mut b)) = base_bounds else {
            let v = (0, 0, w, h);
            self.opaque_bounds_cache.insert(key, v);
            return v;
        };

        if need_flip && base_w > 0 {
            let nl = base_w.saturating_sub(r);
            let nr = base_w.saturating_sub(l);
            l = nl;
            r = nr;
        }

        let min_w = base_w.min(w);
        let min_h = base_h.min(h);
        l = l.min(min_w);
        r = r.min(min_w);
        t = t.min(min_h);
        b = b.min(min_h);

        let v = if l < r && t < b {
            (l, t, r, b)
        } else {
            (0, 0, w, h)
        };
        self.opaque_bounds_cache.insert(key, v);
        v
    }

    pub fn compose_bgra(
        &mut self,
        base_frame_path: &str,
        need_flip: bool,
        target_w: i32,
        target_h: i32,
    ) -> Option<Vec<u8>> {
        let key = format!(
            "{}|{}|{}x{}",
            base_frame_path,
            if need_flip { "F" } else { "" },
            target_w,
            target_h
        );
        if key == self.last_key {
            return None;
        }
        self.last_key = key;

        let w = target_w.max(1) as u32;
        let h = target_h.max(1) as u32;
        let base_img = load_cached_rgba(&mut self.cache, &mut self.cache_order, base_frame_path);
        if !need_flip && base_img.width() == w && base_img.height() == h {
            return Some(to_premultiplied_bgra(&base_img));
        }

        let base_img = if need_flip {
            flip_rgba_horz(&base_img)
        } else {
            (*base_img).clone()
        };

        let composed = if base_img.width() == w && base_img.height() == h {
            base_img
        } else {
            let mut canvas = RgbaImage::new(w, h);
            let min_w = canvas.width().min(base_img.width());
            let min_h = canvas.height().min(base_img.height());
            for y in 0..min_h {
                for x in 0..min_w {
                    canvas.get_pixel_mut(x, y).0 = base_img.get_pixel(x, y).0;
                }
            }
            canvas
        };

        Some(to_premultiplied_bgra(&composed))
    }
}

pub fn to_premultiplied_bgra(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((img.width() * img.height() * 4) as usize);
    for p in img.pixels() {
        let a = p[3] as u32;
        let r = (p[0] as u32 * a + 127) / 255;
        let g = (p[1] as u32 * a + 127) / 255;
        let b = (p[2] as u32 * a + 127) / 255;
        out.push(b as u8);
        out.push(g as u8);
        out.push(r as u8);
        out.push(a as u8);
    }
    out
}

pub fn load_cached_rgba(
    cache: &mut HashMap<String, Arc<RgbaImage>>,
    order: &mut VecDeque<String>,
    path: &str,
) -> Arc<RgbaImage> {
    if let Some(img) = cache.get(path) {
        touch_key(order, path);
        return Arc::clone(img);
    }
    let mut img = image::open(path).expect("无法打开 PNG").to_rgba8();
    auto_colorkey_if_no_alpha(&mut img);
    let key = path.to_string();
    let arc = Arc::new(img);
    cache.insert(key.clone(), Arc::clone(&arc));
    order.push_back(key);
    trim_cache(cache, order);
    arc
}

fn touch_key(order: &mut VecDeque<String>, key: &str) {
    if let Some(pos) = order.iter().position(|k| k == key) {
        order.remove(pos);
    }
    order.push_back(key.to_string());
}

fn trim_cache(cache: &mut HashMap<String, Arc<RgbaImage>>, order: &mut VecDeque<String>) {
    while order.len() > MAX_FRAME_CACHE {
        if let Some(k) = order.pop_front() {
            cache.remove(&k);
        }
    }
}

pub fn auto_colorkey_if_no_alpha(img: &mut RgbaImage) {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 {
        return;
    }

    let mut has_any_transparency = false;
    for p in img.pixels() {
        if p[3] != 255 {
            has_any_transparency = true;
            break;
        }
    }
    if has_any_transparency {
        return;
    }

    let key = img.get_pixel(0, 0).0;
    let key_rgb = (key[0], key[1], key[2]);

    let corners = [
        img.get_pixel(0, 0).0,
        img.get_pixel(w - 1, 0).0,
        img.get_pixel(0, h - 1).0,
        img.get_pixel(w - 1, h - 1).0,
    ];
    if corners
        .iter()
        .any(|c| (c[0], c[1], c[2]) != key_rgb || c[3] != 255)
    {
        return;
    }

    let img_view = img.clone();
    let mut visited = vec![false; (w as usize) * (h as usize)];
    let mut stack: Vec<(u32, u32)> = Vec::new();

    let push_if_key = |x: u32, y: u32, stack: &mut Vec<(u32, u32)>, visited: &mut [bool]| {
        let idx = (y as usize) * (w as usize) + (x as usize);
        if visited[idx] {
            return;
        }
        let p = img_view.get_pixel(x, y).0;
        if p[3] == 255 && (p[0], p[1], p[2]) == key_rgb {
            visited[idx] = true;
            stack.push((x, y));
        }
    };

    for x in 0..w {
        push_if_key(x, 0, &mut stack, &mut visited);
        push_if_key(x, h - 1, &mut stack, &mut visited);
    }
    for y in 0..h {
        push_if_key(0, y, &mut stack, &mut visited);
        push_if_key(w - 1, y, &mut stack, &mut visited);
    }

    while let Some((x, y)) = stack.pop() {
        img.get_pixel_mut(x, y).0[3] = 0;

        if x > 0 {
            push_if_key(x - 1, y, &mut stack, &mut visited);
        }
        if x + 1 < w {
            push_if_key(x + 1, y, &mut stack, &mut visited);
        }
        if y > 0 {
            push_if_key(x, y - 1, &mut stack, &mut visited);
        }
        if y + 1 < h {
            push_if_key(x, y + 1, &mut stack, &mut visited);
        }
    }
}

pub fn flip_rgba_horz(img: &RgbaImage) -> RgbaImage {
    let w = img.width();
    let h = img.height();
    let mut out = RgbaImage::new(w, h);
    if w == 0 || h == 0 {
        return out;
    }
    for y in 0..h {
        for x in 0..w {
            let sx = w - 1 - x;
            out.get_pixel_mut(x, y).0 = img.get_pixel(sx, y).0;
        }
    }
    out
}

fn find_opaque_bounds(img: &RgbaImage, alpha_threshold: u8) -> Option<(u32, u32, u32, u32)> {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 {
        return None;
    }

    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0_u32;
    let mut max_y = 0_u32;
    let mut any = false;

    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y).0[3] >= alpha_threshold {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }

    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}
