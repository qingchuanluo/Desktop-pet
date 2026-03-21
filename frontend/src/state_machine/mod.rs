//! State Machine（行为状态机）
//!
//! 用状态机驱动桌宠行为与动画选择：
//! - Idle：待机
//! - Walk：行走
//! - Relax：闲置小动作
//! - Sleep：睡觉
//! - Drag：拖拽（按住持续，松开回 Idle）

use super::animation::AnimationPlayer;
use crate::character::CharacterTexts;
use rand::Rng;

#[derive(Clone, Copy, Default)]
pub struct BehaviorContext {
    pub hunger: i32,
    pub hour: u8,
    pub clicks_30s: u32,
    pub bad_weather: bool,
}

pub struct Position {
    pub x: f32,
    pub y: f32,
}

pub struct Target {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Left,
    Right,
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
    pub facing: Facing,
    pub bounds_x: f32,
    pub bounds_y: f32,
    pub bounds_w: f32,
    pub bounds_h: f32,
    pub sprite_w: f32,
    pub sprite_h: f32,
}

impl Mover {
    pub fn stop_at_current_pos(&mut self) {
        self.target = Target {
            x: self.pos.x,
            y: self.pos.y,
        };
        self.state = MoverState::Resting { timer_ms: 0 };
    }

    pub fn update(&mut self, delta: std::time::Duration) {
        let dt = delta.as_secs_f32();
        let mut next_state: Option<MoverState> = None;

        match &mut self.state {
            MoverState::Moving => {
                let dx = self.target.x - self.pos.x;
                let dy = self.target.y - self.pos.y;
                let distance = (dx * dx + dy * dy).sqrt();
                let step = self.speed * dt;

                if dx < 0.0 {
                    self.facing = Facing::Left;
                } else if dx > 0.0 {
                    self.facing = Facing::Right;
                }

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
            }
        }

        if let Some(state) = next_state {
            self.state = state;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActorState {
    Idle,
    Walk,
    Relax,
    Sleep,
    Drag,
}

pub struct Actor {
    pub walk_left: AnimationPlayer,
    pub walk_right: AnimationPlayer,
    pub idle: AnimationPlayer,
    pub relax: AnimationPlayer,
    pub sleep: AnimationPlayer,
    pub drag_left: AnimationPlayer,
    pub drag_right: AnimationPlayer,
    pub facing: Facing,
    pub state: ActorState,
    pub state_left_ms: u32,
    pub drag_release_hold_ms: u32,
    pub idle_base_facing: Facing,
    pub flip_walk_left: bool,
    pub flip_walk_right: bool,
    pub flip_drag_left: bool,
    pub flip_drag_right: bool,
    pub talk_timer_ms: u32,
    pub talk_cooldown_ms: u32,
    texts: CharacterTexts,
    pending_bubble_text: Option<String>,
    pending_state: Option<ActorState>,
    last_drag_x: Option<f32>,
    behavior: BehaviorContext,
    rng: rand::rngs::ThreadRng,
}

pub struct ActorAssets {
    pub walk_left: AnimationPlayer,
    pub walk_right: AnimationPlayer,
    pub idle: AnimationPlayer,
    pub relax: AnimationPlayer,
    pub sleep: AnimationPlayer,
    pub drag_left: AnimationPlayer,
    pub drag_right: AnimationPlayer,
    pub idle_base_facing: Facing,
    pub flip_walk_left: bool,
    pub flip_walk_right: bool,
    pub flip_drag_left: bool,
    pub flip_drag_right: bool,
}

pub struct ActorUpdateResult {
    pub started_talk: bool,
    pub started_auto_talk: bool,
    pub bubble_text: Option<String>,
}

impl Actor {
    fn set_walk_target(&mut self, mover: &mut Mover) {
        let max_x = (mover.bounds_w - mover.sprite_w).max(0.0);
        let max_y = (mover.bounds_h - mover.sprite_h).max(0.0);
        let min_y = mover.bounds_y;
        let max_y = mover.bounds_y + max_y;

        let hunger = self.behavior.hunger.clamp(0, 100);
        let night = self.behavior.hour <= 6 || self.behavior.hour >= 23;
        let clingy = self.behavior.clicks_30s >= 5;
        let bad_weather = self.behavior.bad_weather;

        let mut range_factor: f32 = 1.0;
        if night {
            range_factor *= 0.5;
        }
        if bad_weather {
            range_factor *= 0.75;
        }
        if hunger < 20 {
            range_factor *= 0.6;
        }
        if clingy {
            range_factor *= 0.45;
        }
        range_factor = range_factor.clamp(0.15, 1.0);

        let range_x = max_x * range_factor;
        let center_x = (mover.pos.x - mover.bounds_x).clamp(0.0, max_x);
        let dx = (self.rng.gen::<f32>() * 2.0 - 1.0) * range_x;
        let next_x = (center_x + dx).clamp(0.0, max_x);

        mover.target = Target {
            x: mover.bounds_x + next_x,
            y: mover.pos.y.clamp(min_y, max_y),
        };
        mover.state = MoverState::Moving;
    }

    pub fn new(assets: ActorAssets) -> Self {
        let ActorAssets {
            walk_left,
            walk_right,
            idle,
            relax,
            sleep,
            drag_left,
            drag_right,
            idle_base_facing,
            flip_walk_left,
            flip_walk_right,
            flip_drag_left,
            flip_drag_right,
        } = assets;
        let mut actor = Self {
            walk_left,
            walk_right,
            idle,
            relax,
            sleep,
            drag_left,
            drag_right,
            facing: Facing::Right,
            state: ActorState::Idle,
            state_left_ms: 0,
            drag_release_hold_ms: 0,
            idle_base_facing,
            flip_walk_left,
            flip_walk_right,
            flip_drag_left,
            flip_drag_right,
            talk_timer_ms: 0,
            talk_cooldown_ms: 0,
            texts: CharacterTexts::default(),
            pending_bubble_text: None,
            pending_state: None,
            last_drag_x: None,
            behavior: BehaviorContext::default(),
            rng: rand::thread_rng(),
        };
        actor.reset_state(ActorState::Idle, None);
        actor
    }

    pub fn set_texts(&mut self, texts: CharacterTexts) {
        self.texts = texts;
    }

    pub fn set_behavior_context(&mut self, ctx: BehaviorContext) {
        self.behavior = ctx;
    }

    pub fn enqueue_bubble_text(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        self.pending_bubble_text = Some(text);
    }

    pub fn request_state(&mut self, state: ActorState) {
        self.pending_state = Some(state);
    }

    pub fn request_drag_pose_ms(&mut self, ms: u32) {
        self.drag_left.reset();
        self.drag_right.reset();
        self.drag_release_hold_ms = self.drag_release_hold_ms.max(ms);
    }

    pub fn current_frame_path_and_flip(&self) -> (&str, bool) {
        if self.drag_release_hold_ms > 0 {
            let flip = match self.facing {
                Facing::Left => self.flip_drag_left,
                Facing::Right => self.flip_drag_right,
            };
            return (self.drag_frame_path(), flip);
        }

        match self.state {
            ActorState::Drag => {
                let flip = match self.facing {
                    Facing::Left => self.flip_drag_left,
                    Facing::Right => self.flip_drag_right,
                };
                (self.drag_frame_path(), flip)
            }
            ActorState::Walk => {
                let (path, flip) = match self.facing {
                    Facing::Left => (
                        self.walk_left.get_current_frame().path.as_str(),
                        self.flip_walk_left,
                    ),
                    Facing::Right => (
                        self.walk_right.get_current_frame().path.as_str(),
                        self.flip_walk_right,
                    ),
                };
                (path, flip)
            }
            ActorState::Relax => {
                let flip = self.facing != self.idle_base_facing;
                (self.relax.get_current_frame().path.as_str(), flip)
            }
            ActorState::Sleep => {
                let flip = self.facing != self.idle_base_facing;
                (self.sleep.get_current_frame().path.as_str(), flip)
            }
            ActorState::Idle => {
                let flip = self.facing != self.idle_base_facing;
                (self.idle.get_current_frame().path.as_str(), flip)
            }
        }
    }

    fn drag_frame_path(&self) -> &str {
        match self.facing {
            Facing::Left => self.drag_left.get_current_frame().path.as_str(),
            Facing::Right => self.drag_right.get_current_frame().path.as_str(),
        }
    }

    fn roll_weighted(&mut self, options: &[(ActorState, u32)]) -> ActorState {
        let total: u32 = options.iter().map(|(_, w)| *w).sum();
        let mut r = self.rng.gen_range(0..total);
        for (state, weight) in options {
            if r < *weight {
                return *state;
            }
            r -= *weight;
        }
        options.last().map(|(s, _)| *s).unwrap_or(ActorState::Idle)
    }

    fn choose_next_state(&mut self, from: ActorState) -> ActorState {
        let mut walk_delta: i32 = 0;
        let mut relax_delta: i32 = 0;
        let mut sleep_delta: i32 = 0;
        let mut idle_delta: i32 = 0;

        let hunger = self.behavior.hunger.clamp(0, 100);
        let night = self.behavior.hour <= 6 || self.behavior.hour >= 23;
        if night {
            sleep_delta += 20;
            walk_delta -= 20;
        }
        if self.behavior.bad_weather {
            relax_delta += 10;
            walk_delta -= 10;
        }
        if hunger < 20 {
            sleep_delta += 15;
            walk_delta -= 15;
        }
        if self.behavior.clicks_30s >= 5 {
            idle_delta += 20;
            walk_delta -= 25;
            relax_delta += 5;
        }

        fn clamp_w(base: u32, delta: i32) -> u32 {
            let v = base as i32 + delta;
            v.clamp(1, 10_000) as u32
        }

        match from {
            ActorState::Idle => self.roll_weighted(&[
                (ActorState::Walk, clamp_w(75, walk_delta)),
                (ActorState::Relax, clamp_w(10, relax_delta)),
                (ActorState::Sleep, clamp_w(5, sleep_delta)),
                (ActorState::Idle, clamp_w(10, idle_delta)),
            ]),
            ActorState::Walk => self.roll_weighted(&[
                (ActorState::Idle, clamp_w(90, idle_delta)),
                (ActorState::Relax, clamp_w(5, relax_delta)),
                (ActorState::Sleep, clamp_w(5, sleep_delta)),
            ]),
            ActorState::Relax => self.roll_weighted(&[
                (ActorState::Idle, clamp_w(80, idle_delta)),
                (ActorState::Walk, clamp_w(10, walk_delta)),
                (ActorState::Sleep, clamp_w(10, sleep_delta)),
            ]),
            ActorState::Sleep => ActorState::Idle,
            ActorState::Drag => ActorState::Idle,
        }
    }

    fn random_duration_ms(&mut self, state: ActorState) -> u32 {
        let base = match state {
            ActorState::Idle => self.rng.gen_range(2000..=5000),
            ActorState::Walk => 8000,
            ActorState::Relax => self.rng.gen_range(1000..=2000),
            ActorState::Sleep => self.rng.gen_range(5000..=10_000),
            ActorState::Drag => u32::MAX,
        };
        let hunger = self.behavior.hunger.clamp(0, 100);
        let night = self.behavior.hour <= 6 || self.behavior.hour >= 23;
        let clingy = self.behavior.clicks_30s >= 5;

        let mut scale: u32 = 2;
        if night && matches!(state, ActorState::Sleep) {
            scale = scale.saturating_add(1);
        }
        if hunger < 20 && matches!(state, ActorState::Sleep) {
            scale = scale.saturating_add(1);
        }
        if clingy && matches!(state, ActorState::Walk) {
            scale = scale.saturating_sub(1).max(1);
        }
        base.saturating_mul(scale)
    }

    fn reset_state(&mut self, state: ActorState, mover: Option<&mut Mover>) -> Option<String> {
        self.state = state;
        self.state_left_ms = self.random_duration_ms(state);

        let bubble_text = match state {
            ActorState::Walk => self.pick_event_phrase("walk"),
            ActorState::Relax => self.pick_event_phrase("relax"),
            ActorState::Sleep => self.pick_event_phrase("sleep"),
            _ => None,
        };
        if bubble_text.is_some() {
            self.talk_timer_ms = self.talk_timer_ms.max(3500);
        }

        match state {
            ActorState::Idle => {
                self.idle.reset();
                if let Some(m) = mover {
                    m.stop_at_current_pos();
                }
            }
            ActorState::Walk => {
                self.walk_left.reset();
                self.walk_right.reset();
                if let Some(m) = mover {
                    self.set_walk_target(m);
                }
            }
            ActorState::Relax => {
                self.relax.reset();
                if let Some(m) = mover {
                    m.stop_at_current_pos();
                }
            }
            ActorState::Sleep => {
                self.sleep.reset();
                if let Some(m) = mover {
                    m.stop_at_current_pos();
                }
            }
            ActorState::Drag => {
                self.drag_left.reset();
                self.drag_right.reset();
                if let Some(m) = mover {
                    m.stop_at_current_pos();
                }
            }
        }
        bubble_text
    }


    fn pick_event_phrase(&mut self, key: &str) -> Option<String> {
        let phrases = self
            .texts
            .event_phrases
            .as_ref()
            .and_then(|m| m.get(key))
            .filter(|v| !v.is_empty())?;
        let idx = self.rng.gen_range(0..phrases.len());
        Some(phrases[idx].clone())
    }

    pub fn update(
        &mut self,
        mover: &mut Mover,
        dragging: bool,
        stop_pet: bool,
        delta: std::time::Duration,
        talk_trigger: bool,
    ) -> ActorUpdateResult {
        let dt_ms = delta.as_millis().min(u128::from(u32::MAX)) as u32;
        let dt_ms = dt_ms.max(1);

        const TALK_MS: u32 = 2000;
        let was_talking = self.talk_timer_ms > 0;
        let mut bubble_text: Option<String> = self.pending_bubble_text.take();
        if bubble_text.is_some() {
            self.talk_timer_ms = TALK_MS;
            self.talk_cooldown_ms = 0;
        }

        let next_cooldown = self.talk_cooldown_ms.saturating_add(dt_ms);
        let auto_due = false;
        let talk_requested = talk_trigger || auto_due;

        let mut started_auto_talk = false;
        if talk_requested {
            self.talk_cooldown_ms = 0;
            started_auto_talk = !talk_trigger && auto_due;
            self.talk_timer_ms = TALK_MS;
        } else {
            self.talk_cooldown_ms = next_cooldown;
        }

        if self.talk_timer_ms > 0 {
            self.talk_timer_ms = self.talk_timer_ms.saturating_sub(dt_ms);
        }

        if self.drag_release_hold_ms > 0 {
            self.drag_release_hold_ms = self.drag_release_hold_ms.saturating_sub(dt_ms);
        }

        if dragging {
            if let Some(prev_x) = self.last_drag_x {
                let dx = mover.pos.x - prev_x;
                if dx < 0.0 {
                    self.facing = Facing::Left;
                } else if dx > 0.0 {
                    self.facing = Facing::Right;
                }
            }
            self.last_drag_x = Some(mover.pos.x);

            if self.state != ActorState::Drag {
                let _ = self.reset_state(ActorState::Drag, Some(mover));
            }
            match self.facing {
                Facing::Left => self.drag_left.tick(delta),
                Facing::Right => self.drag_right.tick(delta),
            }

            mover.stop_at_current_pos();

            return ActorUpdateResult {
                started_talk: !was_talking && self.talk_timer_ms > 0,
                started_auto_talk: started_auto_talk && !was_talking && self.talk_timer_ms > 0,
                bubble_text: None,
            };
        }

        self.last_drag_x = None;
        self.facing = mover.facing;

        if !stop_pet && self.drag_release_hold_ms > 0 {
            match self.facing {
                Facing::Left => self.drag_left.tick(delta),
                Facing::Right => self.drag_right.tick(delta),
            }
            mover.stop_at_current_pos();
            return ActorUpdateResult {
                started_talk: !was_talking && self.talk_timer_ms > 0,
                started_auto_talk: started_auto_talk && !was_talking && self.talk_timer_ms > 0,
                bubble_text: None,
            };
        }

        if let Some(req) = self.pending_state.take() {
            bubble_text = self.reset_state(req, Some(mover)).or(bubble_text);
        }

        if self.state == ActorState::Drag {
            self.drag_left.reset();
            self.drag_right.reset();
            self.drag_release_hold_ms = 200;
            bubble_text = self
                .reset_state(ActorState::Idle, Some(mover))
                .or(bubble_text);
        }

        if stop_pet {
            mover.stop_at_current_pos();
            if self.state == ActorState::Sleep {
                self.sleep.tick(delta);
            } else {
                self.idle.tick(delta);
            }
            return ActorUpdateResult {
                started_talk: !was_talking && self.talk_timer_ms > 0,
                started_auto_talk: started_auto_talk && !was_talking && self.talk_timer_ms > 0,
                bubble_text: None,
            };
        }

        if self.state_left_ms > 0 {
            self.state_left_ms = self.state_left_ms.saturating_sub(dt_ms);
        }
        if self.state_left_ms == 0 {
            let next = self.choose_next_state(self.state);
            bubble_text = self.reset_state(next, Some(mover)).or(bubble_text);
        }

        match self.state {
            ActorState::Idle => {
                mover.stop_at_current_pos();
                self.idle.tick(delta);
            }
            ActorState::Walk => {
                if matches!(mover.state, MoverState::Resting { .. }) {
                    self.set_walk_target(mover);
                }
                mover.update(delta);
                self.facing = mover.facing;
                match self.facing {
                    Facing::Left => self.walk_left.tick(delta),
                    Facing::Right => self.walk_right.tick(delta),
                }
            }
            ActorState::Relax => {
                mover.stop_at_current_pos();
                self.relax.tick(delta);
            }
            ActorState::Sleep => {
                mover.stop_at_current_pos();
                self.sleep.tick(delta);
            }
            ActorState::Drag => {
                mover.stop_at_current_pos();
            }
        }

        ActorUpdateResult {
            started_talk: !was_talking && self.talk_timer_ms > 0,
            started_auto_talk: started_auto_talk && !was_talking && self.talk_timer_ms > 0,
            bubble_text,
        }
    }
}
