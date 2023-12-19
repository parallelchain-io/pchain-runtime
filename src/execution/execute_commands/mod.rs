/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Module for managing the lifecycle and execution strategy of Account and Staking commands.
pub mod executor;
pub(crate) use executor::*;

pub mod phases;
