use super::animation::Animation;

pub struct AnimationPlayer {
    pub animation: Animation,
    pub current_frame_index: usize,
    pub current_frame: usize,
    accumulator_ms: u32,
}

impl AnimationPlayer {
    pub fn new(animation: Animation) -> Self {
        Self {
            animation,
            current_frame_index: 0,
            current_frame: 0,
            accumulator_ms: 0,
        }
    }

    pub fn reset(&mut self) {
        self.current_frame_index = 0;
        self.current_frame = 0;
        self.accumulator_ms = 0;
    }

    pub fn next_frame(&mut self) {
        if self.animation.frames.is_empty() {
            self.current_frame_index = 0;
            self.current_frame = 0;
            self.accumulator_ms = 0;
            return;
        }

        self.current_frame_index += 1;

        if self.current_frame_index >= self.animation.frames.len() {
            if self.animation.looped {
                self.current_frame_index = 0;
            } else {
                self.current_frame_index = self.animation.frames.len() - 1;
            }
        }

        self.current_frame = self.current_frame_index;
    }

    pub fn tick(&mut self, delta: std::time::Duration) {
        let fps = self.animation.fps.max(1);
        let frame_ms = (1000 / fps).max(1);
        self.accumulator_ms = self
            .accumulator_ms
            .saturating_add(delta.as_millis().min(u128::from(u32::MAX)) as u32);

        while self.accumulator_ms >= frame_ms {
            self.next_frame();
            self.accumulator_ms -= frame_ms;
        }
    }

    pub fn get_current_frame(&self) -> &super::frame::Frame {
        self.animation
            .frames
            .get(self.current_frame_index)
            .expect("动画没有任何帧")
    }
}
