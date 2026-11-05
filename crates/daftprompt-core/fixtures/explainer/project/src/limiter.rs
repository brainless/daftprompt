//! Request rate limiting for the public API.

/// A token bucket refills at a fixed rate and allows bursts up to its capacity.
/// Each request removes one token; a request that finds the bucket empty is
/// rejected with HTTP 429.
pub struct TokenBucket {
    /// Largest number of tokens the bucket can hold (the burst size).
    pub capacity: u32,
    /// Tokens added back every second.
    pub refill_per_sec: u32,
    tokens: u32,
}

/// Default limits: bursts of 20 requests, refilling 5 tokens per second.
pub const DEFAULT_CAPACITY: u32 = 20;
pub const DEFAULT_REFILL_PER_SEC: u32 = 5;

impl TokenBucket {
    pub fn new(capacity: u32, refill_per_sec: u32) -> Self {
        Self { capacity, refill_per_sec, tokens: capacity }
    }

    pub fn try_take(&mut self) -> bool {
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}
