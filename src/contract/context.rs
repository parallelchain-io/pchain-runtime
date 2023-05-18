/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct containing context for a Smart Contract Execution such as cache for storing 
//! smart contract module and VM memory limit for contract execution.

use crate::{Cache, wasmer::wasmer_store};

/// Smart Contract Context stores useful information for contract execution.
#[derive(Clone)]
pub(crate) struct SmartContractContext {
    /// smart contract cache for storing compiled wasmer module to save transition time
    pub cache: Option<Cache>,
    /// smart contract VM memory limit
    pub memory_limit: Option<usize>,
}

impl SmartContractContext {
    /// Instantiate [wasmer::Store] from this context.
    pub fn instantiate_store(&self) -> wasmer::Store {
        wasmer_store::instantiate_store(u64::MAX, self.memory_limit)
    }
}