use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use serenity::all::UserId;

#[derive(Debug, Clone)]
pub struct RateLimit {
    users: HashMap<UserId, Vec<SystemTime>>,
    uses_per_interval: usize,
    interval: u64,
}

impl RateLimit {
    pub fn new(uses_per_interval: usize, interval: u64) -> Self {
        Self {
            users: HashMap::new(),
            uses_per_interval,
            interval,
        }
    }

    pub fn is_at_limit(&mut self, user_id: UserId) -> bool {
        let now = SystemTime::now();
        let usage_window = now - Duration::from_secs(self.interval);

        let uses = self.users.entry(user_id).or_insert_with(Vec::new);

        uses.retain(|&t| t > usage_window);

        if uses.len() >= self.uses_per_interval {
            return true;
        }

        uses.push(now);
        return false;
    }
}
