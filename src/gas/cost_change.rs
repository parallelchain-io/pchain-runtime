/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! A struct which stores intermediate gas cost changes.
//!
//! The struct is used by the respective Gas Meters, and stores both the deducted and rewarded gas.
//!
//! In most chargeable operations, gas is deducted.
//! However, in some cases, e.g. the removal of stored data, gas can be refunded as a reward.
//! The net gas cost is computed by offsetting these values.

use std::ops::{Add, AddAssign, Sub, SubAssign};

/// ### Example:
/// ```no_run
/// let mut change = CostChange::default(); // = 0
/// change += CostChange::reward(1); // = 1
/// change += CostChange::deduct(2); // = -1
/// assert_eq!(change.net_cost().0, 1);
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CostChange {
    deduct: u64,
    reward: u64,
}

impl CostChange {
    pub const fn deduct(value: u64) -> Self {
        Self {
            deduct: value,
            reward: 0,
        }
    }
    pub const fn reward(value: u64) -> Self {
        Self {
            deduct: 0,
            reward: value,
        }
    }

    pub fn net_cost(&self) -> (u64, u64) {
        (
            self.deduct.saturating_sub(self.reward),
            self.reward.saturating_sub(self.deduct),
        )
    }
}

impl AddAssign for CostChange {
    fn add_assign(&mut self, rhs: Self) {
        self.deduct = self.deduct.saturating_add(rhs.deduct);
        self.reward = self.reward.saturating_add(rhs.reward);
    }
}

impl Add for CostChange {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            deduct: self.deduct.saturating_add(other.deduct),
            reward: self.reward.saturating_add(other.reward),
        }
    }
}

impl SubAssign for CostChange {
    fn sub_assign(&mut self, rhs: Self) {
        let v = self.sub(rhs);
        *self = v;
    }
}

impl Sub for CostChange {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        let net_deduct = other.deduct.saturating_sub(self.deduct);
        let net_reward = other.reward.saturating_sub(self.reward);
        Self {
            deduct: self.deduct.saturating_sub(other.deduct) + net_reward,
            reward: self.reward.saturating_sub(other.reward) + net_deduct,
        }
    }
}
#[test]
fn test_cost_change() {
    let mut change = CostChange::default(); // = 0
    change += CostChange::reward(1); // = 1
    change += CostChange::deduct(2); // = -1
    assert_eq!(change.net_cost(), (1, 0));
    change -= CostChange::deduct(3); // = 2
    change -= CostChange::reward(0); // = 2
    assert_eq!(change.net_cost(), (0, 2));
}
