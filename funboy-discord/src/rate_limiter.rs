use std::{
    collections::HashMap,
    thread::sleep,
    time::{Duration, SystemTime},
};

use serenity::all::UserId;

#[derive(Debug, Clone)]
pub struct Uses {
    times_limited: u16,
    time_stamps: Vec<SystemTime>,
}

impl Uses {
    fn new() -> Self {
        Self {
            times_limited: 0,
            time_stamps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimit {
    users: HashMap<UserId, Uses>,
    uses_per_interval: usize,
    interval: u64,
    max_times_limited: u16,
}

pub enum RateLimitResult {
    MaxLimitsReached,
    UsesPerIntervalreached,
    Ok,
}

impl RateLimit {
    pub fn new(uses_per_interval: usize, interval: u64, max_times_limited: u16) -> Self {
        Self {
            users: HashMap::new(),
            uses_per_interval,
            interval,
            max_times_limited,
        }
    }

    pub fn check(&mut self, user_id: UserId) -> RateLimitResult {
        let now = SystemTime::now();
        let usage_window = now - Duration::from_secs(self.interval);

        let uses = self.users.entry(user_id).or_insert_with(Uses::new);

        uses.time_stamps.retain(|&t| t > usage_window);

        if uses.time_stamps.len() == 0 {
            uses.times_limited = 0;
        }

        if uses.time_stamps.len() >= self.uses_per_interval {
            uses.times_limited = uses.times_limited.saturating_add(1);
            if uses.times_limited >= self.max_times_limited {
                return RateLimitResult::MaxLimitsReached;
            } else {
                return RateLimitResult::UsesPerIntervalreached;
            }
        }

        uses.time_stamps.push(now);
        return RateLimitResult::Ok;
    }

    pub fn reset(&mut self, user_id: UserId) {
        self.users.remove_entry(&user_id);
    }
}
