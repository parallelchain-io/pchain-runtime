/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Handles execution of [state](state) transition functions and view calls.
//!
//! This module provides the core functionality for [executing commands](execute_commands) that facilitate state transitions, which are triggered by submitting a transaction.
//! It also manages [view calls](execute_view), which are used for read-only access to the blockchain state.
//! While view calls follow similar execution logic as state transitions, they are distinct in that they do not result in any state modification.
//! This design allows for a consistent approach to both state-altering transactions and non-modifying view calls.

pub mod abort;

pub mod cache;

pub mod state;

pub mod execute_commands;

pub mod execute;

pub mod execute_view;

pub mod execute_next_epoch;

#[cfg(test)]
mod tests {
    mod basic;
    mod next_epoch;
    mod pool;
    mod staking;
    mod test_utils;
}
