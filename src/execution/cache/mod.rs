/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Modules for data caching and temporary storage during execution.
//!
//! Includes:
//! - `world_state_cache`: A cache that enhances read and write efficiency to the World State.
//! - `output_buffer`: Temporary storage for command outputs.
//! - `receipt_buffer`: Temporary storage for accumulating command receipts.

pub mod world_state_cache;
pub(crate) use world_state_cache::*;

pub mod output_buffer;
pub(crate) use output_buffer::*;

pub mod receipt_buffer;
pub(crate) use receipt_buffer::*;
