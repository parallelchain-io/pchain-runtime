/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Facilitates the configuration of a Wasm store for smart contract execution.
//!
//! This module provides essential functions to configure the Wasm store,
//! which represents the collective state of all Wasm runtime objects, including functions, memory, and global variables.
//!
//! Utilized by [ContractModule](crate::contract::wasmer::module), this module includes functions for:
//! - Instantiating the store with custom middleware, such as [NonDeterminismFilter](super::non_determinism_filter::NonDeterminismFilter), and configuring gas and memory limits.
//! - Setting a cap on the Wasm linear memory.
//!
//! These functions play a pivotal role in tailoring the Wasmer-provided store to the exact needs of the contract execution environment.

use std::convert::TryFrom;
use std::sync::Arc;
use wasmer::{BaseTunables, CompilerConfig, Pages, Store, Target, WASM_PAGE_SIZE};
use wasmer_compiler_singlepass::Singlepass;
use wasmer_engine_universal::Universal;
use wasmer_middlewares::Metering;

use crate::gas::wasm_opcode_gas_schedule;

use super::custom_tunables::CustomTunables;
use super::non_determinism_filter::NonDeterminismFilter;

/// Instantiate a Store which includes customised middleware e.g. [filter](super::non_determinism_filter::NonDeterminismFilter).
pub fn instantiate_store(gas_limit: u64, memory_limit: Option<usize>) -> Store {
    // call non_determinism_filter.rs to disallow non-deterministic types
    let nd_filter = Arc::new(NonDeterminismFilter::default());

    // define the metering middleware
    let metering = Arc::new(Metering::new(gas_limit, wasm_opcode_gas_schedule));

    // use the Singlepass compiler which is optimised for fast compilation
    let mut compiler_config = Singlepass::new();
    compiler_config.push_middleware(nd_filter);
    compiler_config.push_middleware(metering);
    let engine = Universal::new(compiler_config).engine();

    // creates a Wasmer store with an optional guest memory limit
    // If no memory limit is set, the method falls back to creating the store without custom memory adjustment
    match memory_limit {
        Some(limit) => {
            let base_tunables = BaseTunables::for_target(&Target::default());
            let custom_tunables = CustomTunables::new(base_tunables, limit_pages(limit));
            Store::new_with_tunables(&engine, custom_tunables)
        }
        None => Store::new(&engine),
    }
}

/// `limit_pages` caps the total Wasm linear memory (measured in page size) for runtime.
/// Linear memory in Wasm can have at most 65536 pages, each page being equal to 2^16 or 65536 bytes.
/// The limit supplied has to be less than the maximum value from WebAssembly v1.0 spec.
/// In case of an error, a total of 2^32 bytes (4 gigabytes) will be allocated.
/// See <https://github.com/WebAssembly/memory64/blob/master/proposals/memory64/Overview.md>
fn limit_pages(limit: usize) -> Pages {
    const MAX_PAGES_AVAILABLE: u32 = 65536;

    let capped_size = u32::try_from(limit / WASM_PAGE_SIZE)
        .unwrap_or(MAX_PAGES_AVAILABLE)
        .min(MAX_PAGES_AVAILABLE);

    Pages(capped_size)
}
