use serde_json::json;
use std::sync::Mutex;

#[derive(Clone)]
pub struct PetStats {
    pub level: u32,
    pub xp: u32,
    pub hunger: i32,
    pub coins: u32,
    pub hunger_acc_ms: u64,
    pub xp_acc_ms: u64,
    pub sleep_roll_acc_ms: u64,
    pub dirty: bool,
}

impl PetStats {
    pub fn xp_to_next(&self) -> u32 {
        50
    }

    pub fn add_xp(&mut self, amount: u32) {
        if amount > 0 {
            self.dirty = true;
        }
        self.xp = self.xp.saturating_add(amount);
        loop {
            let need = self.xp_to_next();
            if self.xp < need {
                break;
            }
            self.xp -= need;
            self.level = self.level.saturating_add(1);
            self.coins = self.coins.saturating_add(10);
        }
    }

    pub fn add_hunger(&mut self, amount: i32) {
        if amount != 0 {
            self.dirty = true;
        }
        self.hunger = (self.hunger + amount).clamp(0, 100);
    }

    pub fn tick(&mut self, delta_ms: u64) -> u32 {
        self.hunger_acc_ms = self.hunger_acc_ms.saturating_add(delta_ms);
        while self.hunger_acc_ms >= 360_000 {
            self.hunger_acc_ms -= 360_000;
            self.dirty = true;
            self.hunger = (self.hunger - 10).clamp(0, 100);
        }

        if self.hunger > 0 {
            self.xp_acc_ms = self.xp_acc_ms.saturating_add(delta_ms);
            while self.xp_acc_ms >= 60_000 {
                self.xp_acc_ms -= 60_000;
                self.add_xp(1);
            }
        }

        let mut sleep_rolls = 0_u32;
        self.sleep_roll_acc_ms = self.sleep_roll_acc_ms.saturating_add(delta_ms);
        while self.sleep_roll_acc_ms >= 1000 {
            self.sleep_roll_acc_ms -= 1000;
            sleep_rolls = sleep_rolls.saturating_add(1);
        }
        sleep_rolls
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "level": self.level,
            "xp": self.xp,
            "xp_to_next": self.xp_to_next(),
            "hunger": self.hunger,
            "hunger_max": 100,
            "coins": self.coins
        })
    }
}

pub static PET_STATS: Mutex<PetStats> = Mutex::new(PetStats {
    level: 0,
    xp: 0,
    hunger: 100,
    coins: 0,
    hunger_acc_ms: 0,
    xp_acc_ms: 0,
    sleep_roll_acc_ms: 0,
    dirty: false,
});
