/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

// TODO 1
//! Implementation of state transition functions.
//!
//! Transition functions [executes](execute) one or more commands across phases,
//! and they can be triggered through submitting a [transaction](transactions) or a [view call](execute_view),
//! the latter of which does not involve actual state transition.

pub mod cache;

pub mod state;

pub mod transactions;
pub use transactions::*;

pub mod execute;

pub mod execute_view;

pub mod execute_next_epoch;
