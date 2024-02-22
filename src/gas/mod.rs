/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines all components related to execution gas cost and metering.
//!
//! In the ParallelChain Mainnet Protocol, gas is the base measurement unit for transaction execution cost.

pub(crate) mod cost_change;
pub(crate) use cost_change::*;

pub mod constants;
pub use constants::*;

pub(crate) mod operations;

pub(crate) mod wasmer_gas;
pub(crate) use wasmer_gas::*;

pub(crate) mod gas_meter;
pub(crate) use gas_meter::*;
