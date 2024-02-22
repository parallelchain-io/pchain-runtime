/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Helper context for smart contract execution within the [Runtime](crate::transition::Runtime).
//!
//! The [SmartContractContext] is initialized in the Runtime and passed to [TransitionContext](crate::context::TransitionContext).
//! It holds settings specific to contract execution and uses a cache to optimize loading times for smart contracts.
use super::wasmer::cache::Cache;

/// Smart Contract Context responsibilities include:
/// - Holding a cache instance for compiled Wasm modules
/// - Setting a memory limit for the smart contract virtual machine (VM), ensuring efficient and secure execution.
#[derive(Clone, Default)]
pub(crate) struct SmartContractContext {
    /// smart contract cache for storing compiled Wasmer module to reduce loading time
    pub cache: Option<Cache>,
    /// smart contract VM memory limit
    pub memory_limit: Option<usize>,
}
