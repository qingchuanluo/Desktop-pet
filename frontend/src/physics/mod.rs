use std::time::Duration;

#[derive(Clone, Copy, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy)]
pub struct Bounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

pub struct PhysicsBody {
    pub pos: Vec2,
    pub vel: Vec2,
    pub friction_per_sec: f32,
    pub restitution: f32,
    pub max_speed: f32,
    last_drag_pos: Option<Vec2>,
}

impl PhysicsBody {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            pos: Vec2 { x, y },
            vel: Vec2::default(),
            friction_per_sec: 4.0,
            restitution: 0.65,
            max_speed: 1800.0,
            last_drag_pos: None,
        }
    }

    pub fn sync_pos(&mut self, x: f32, y: f32) {
        self.pos = Vec2 { x, y };
        self.last_drag_pos = Some(self.pos);
    }

    pub fn stop(&mut self) {
        self.vel = Vec2::default();
        self.last_drag_pos = None;
    }

    pub fn is_active(&self) -> bool {
        self.vel.x.abs() + self.vel.y.abs() > 0.5
    }

    pub fn on_drag(&mut self, x: f32, y: f32, delta: Duration) {
        let dt = delta.as_secs_f32().max(1.0 / 240.0);
        let cur = Vec2 { x, y };
        if let Some(prev) = self.last_drag_pos {
            let vx = (cur.x - prev.x) / dt;
            let vy = (cur.y - prev.y) / dt;
            self.vel.x = self.vel.x * 0.35 + vx * 0.65;
            self.vel.y = self.vel.y * 0.35 + vy * 0.65;
            self.clamp_speed();
        }
        self.pos = cur;
        self.last_drag_pos = Some(cur);
    }

    pub fn end_drag(&mut self) {
        self.last_drag_pos = None;
    }

    pub fn step(&mut self, delta: Duration, bounds: Bounds) {
        let dt = delta.as_secs_f32().clamp(1.0 / 240.0, 0.05);

        self.clamp_speed();
        self.pos.x += self.vel.x * dt;
        self.pos.y += self.vel.y * dt;

        let damp = (-self.friction_per_sec.max(0.0) * dt).exp();
        self.vel.x *= damp;
        self.vel.y *= damp;
        if self.vel.x.abs() < 0.2 {
            self.vel.x = 0.0;
        }
        if self.vel.y.abs() < 0.2 {
            self.vel.y = 0.0;
        }

        let (min_x, max_x) = if bounds.min_x <= bounds.max_x {
            (bounds.min_x, bounds.max_x)
        } else {
            (bounds.max_x, bounds.min_x)
        };
        let (min_y, max_y) = if bounds.min_y <= bounds.max_y {
            (bounds.min_y, bounds.max_y)
        } else {
            (bounds.max_y, bounds.min_y)
        };

        if self.pos.x < min_x {
            self.pos.x = min_x;
            self.vel.x = self.vel.x.abs() * self.restitution;
        } else if self.pos.x > max_x {
            self.pos.x = max_x;
            self.vel.x = self.vel.x.abs() * -self.restitution;
        }

        if self.pos.y < min_y {
            self.pos.y = min_y;
            self.vel.y = self.vel.y.abs() * self.restitution;
        } else if self.pos.y > max_y {
            self.pos.y = max_y;
            self.vel.y = self.vel.y.abs() * -self.restitution;
        }
    }

    fn clamp_speed(&mut self) {
        let s2 = self.vel.x * self.vel.x + self.vel.y * self.vel.y;
        if s2 <= 0.0 {
            return;
        }
        let max = self.max_speed.max(0.0);
        let max2 = max * max;
        if s2 > max2 {
            let s = s2.sqrt();
            if s > 0.0 {
                let k = max / s;
                self.vel.x *= k;
                self.vel.y *= k;
            }
        }
    }
}
