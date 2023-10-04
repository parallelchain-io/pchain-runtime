/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of state transition functions.
//!
//! The transition function basically [executes](execute) sequence of commands across [phases](phase):
//! Pre-Charge -> Command(s) -> Charge. The Commands to execute includes [Account](account) Command,
//! [Staking](staking) Command and [Protocol](protocol) Command.

pub mod account;

pub mod contract;

pub mod execute;

pub mod staking;

pub mod phase;

pub mod protocol;

pub mod state;

pub mod gas_meter;
