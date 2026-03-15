//! State Machine（行为状态机）
//!
//! 用状态机驱动桌宠行为与动画选择：
//! - idle：待机（低 FPS、低打扰）
//! - blink：眨眼（短动画循环/定时触发）
//! - walk：行走/移动（可选）
//! - interaction：交互态（拖拽、点击反馈等）

use super::animation::AnimationPlayer;

pub struct Position {
    pub x: f32,
    pub y: f32,
}

pub struct Target {
    pub x: f32,
    pub y: f32,
}

pub enum MoverState {
    Moving,
    Resting { timer_ms: u32 },
}

pub struct Mover {
    pub pos: Position,
    pub target: Target,
    pub speed: f32,
    pub state: MoverState,
    pub bounds_x: f32,
    pub bounds_y: f32,
    pub bounds_w: f32,
    pub bounds_h: f32,
    pub sprite_w: f32,
    pub sprite_h: f32,
}

impl Mover {
    pub fn update(&mut self, delta: std::time::Duration) {
        let dt = delta.as_secs_f32();
        let mut next_state: Option<MoverState> = None;

        match &mut self.state {
            MoverState::Moving => {
                let dx = self.target.x - self.pos.x;
                let dy = self.target.y - self.pos.y;
                let distance = (dx * dx + dy * dy).sqrt();
                let step = self.speed * dt;

                if distance <= step || distance == 0.0 {
                    self.pos = Position {
                        x: self.target.x,
                        y: self.target.y,
                    };
                    next_state = Some(MoverState::Resting { timer_ms: 0 });
                } else {
                    self.pos.x += dx / distance * step;
                    self.pos.y += dy / distance * step;
                }
            }
            MoverState::Resting { timer_ms } => {
                *timer_ms =
                    timer_ms.saturating_add(delta.as_millis().min(u128::from(u32::MAX)) as u32);
                if *timer_ms > 2000 {
                    let max_x = (self.bounds_w - self.sprite_w).max(0.0);
                    let max_y = (self.bounds_h - self.sprite_h).max(0.0);
                    self.target = Target {
                        x: self.bounds_x + rand::random::<f32>() * max_x,
                        y: self.bounds_y + rand::random::<f32>() * max_y,
                    };
                    next_state = Some(MoverState::Moving);
                }
            }
        }

        if let Some(state) = next_state {
            self.state = state;
        }
    }
}

pub struct Actor {
    pub walk: AnimationPlayer,
    pub idle: AnimationPlayer,
    pub blink: AnimationPlayer,
    pub talk: AnimationPlayer,
    pub blink_timer_ms: u32,
    pub talk_timer_ms: u32,
    pub talk_cooldown_ms: u32,
}

impl Actor {
    pub fn update(
        &mut self,
        mover_state: &MoverState,
        delta: std::time::Duration,
        talk_trigger: bool,
    ) {
        let dt_ms = delta.as_millis().min(u128::from(u32::MAX)) as u32;

        match mover_state {
            MoverState::Moving => self.walk.tick(delta),
            MoverState::Resting { .. } => self.idle.tick(delta),
        }

        if self.blink_timer_ms > 0 {
            self.blink.tick(delta);
            self.blink_timer_ms = self.blink_timer_ms.saturating_sub(dt_ms);
        } else if rand::random::<f32>() < 0.01 {
            self.blink_timer_ms = 250;
            self.blink.reset();
        }

        self.talk_cooldown_ms = self.talk_cooldown_ms.saturating_add(dt_ms);
        if talk_trigger || self.talk_cooldown_ms >= 10_000 {
            self.talk_cooldown_ms = 0;
            self.talk_timer_ms = 2000;
            self.talk.reset();
        }

        if self.talk_timer_ms > 0 {
            self.talk.tick(delta);
            self.talk_timer_ms = self.talk_timer_ms.saturating_sub(dt_ms);
        }
    }
}
