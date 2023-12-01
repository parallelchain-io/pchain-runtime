/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct containing context for smart contract execution such as
//! the backing cache for storing compiled Wasmer modules and
//! the and VM memory limit for contract execution.

use super::wasmer::cache::Cache;

/// Smart Contract Context stores useful information for contract execution.
#[derive(Clone, Default)]
pub(crate) struct SmartContractContext {
    /// smart contract cache for storing compiled Wasmer module to reduce loading time
    pub cache: Option<Cache>,
    /// smart contract VM memory limit
    pub memory_limit: Option<usize>,
}
