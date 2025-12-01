use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use serenity::all::UserId;

#[derive(Debug, Clone)]
pub struct Uses {
    times_limited: u16,
    time_stamps: Vec<SystemTime>,
    timeout_start: SystemTime,
}

impl Uses {
    fn new() -> Self {
        Self {
            times_limited: 0,
            time_stamps: Vec::new(),
            timeout_start: SystemTime::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimit {
    users: HashMap<UserId, Uses>,
    uses_per_interval: usize,
    interval: u64,
    limits_before_timeout: u16,
    timeout: u64,
}

pub enum RateLimitResult {
    MaxLimitsReached,
    UsesPerIntervalreached,
    Ok,
}

impl RateLimit {
    pub fn new(uses_per_interval: usize, interval: u64) -> Self {
        Self {
            users: HashMap::new(),
            uses_per_interval,
            interval,
            limits_before_timeout: 0,
            timeout: 0,
        }
    }

    pub fn with_timeout(mut self, timeout: u64, limits_before_timeout: u16) -> Self {
        self.timeout = timeout;
        self.limits_before_timeout = limits_before_timeout;
        self
    }

    pub fn check(&mut self, user_id: UserId) -> RateLimitResult {
        let now = SystemTime::now();
        let usage_window = now - Duration::from_secs(self.interval);

        let uses = self.users.entry(user_id).or_insert_with(Uses::new);

        uses.time_stamps.retain(|&t| t > usage_window);

        if self.limits_before_timeout != 0 && uses.times_limited >= self.limits_before_timeout {
            let dur_since = now.duration_since(uses.timeout_start);
            if dur_since.is_ok_and(|t| t >= Duration::from_secs(self.timeout)) {
                uses.times_limited = 0;
            } else {
                return RateLimitResult::MaxLimitsReached;
            }
        }

        if uses.time_stamps.len() >= self.uses_per_interval {
            uses.times_limited = uses.times_limited.saturating_add(1);
            if self.limits_before_timeout != 0 && uses.times_limited >= self.limits_before_timeout {
                uses.timeout_start = SystemTime::now();
                return RateLimitResult::MaxLimitsReached;
            } else {
                return RateLimitResult::UsesPerIntervalreached;
            }
        }

        uses.time_stamps.push(now);
        return RateLimitResult::Ok;
    }
}
