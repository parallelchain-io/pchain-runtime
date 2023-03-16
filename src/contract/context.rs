/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

/// Context for Smart Contract Execution

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
    pub fn store(&self) -> wasmer::Store {
        wasmer_store::new(u64::MAX, self.memory_limit)
    }
}