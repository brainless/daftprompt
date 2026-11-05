//! Usage accounting for paid plans.

/// A quota here is the monthly allowance of API calls included in an
/// account's plan. When an account has used its quota, further calls are
/// billed as overage at the plan's overage rate.
pub struct Quota {
    pub calls_per_month: u64,
    pub used: u64,
}

impl Quota {
    pub fn remaining(&self) -> u64 {
        self.calls_per_month.saturating_sub(self.used)
    }
}
