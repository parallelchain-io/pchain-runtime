/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines environment used for constructing the Wasm (specifically Wasmer) instance.
//!
//! The environment (Env) keeps track on the data changes happening inside a contract call.
//! Data changes include read-write operation on world state, gas consumed and
//! context related to cross-contract calls.

use pchain_world_state::{VersionProvider, DB};
use std::sync::{Arc, Mutex};
use wasmer::{Global, LazyInit, Memory, NativeFunc};

use crate::{
    execution::gas::WasmerGasGlobal, transition::TransitionContext, types::CallTx, BlockchainParams,
};

use super::memory::MemoryContext;

/// Env provides the functions in `exports` (which are in turn 'imported' by WASM smart contracts)
/// access to complex functionality that typically cannot cross the host-WASM barrier.
///
/// Wasmer handles everything for us.
#[derive(wasmer::WasmerEnv, Clone)]
pub(crate) struct Env<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Transition Context
    pub context: Arc<Mutex<TransitionContext<'a, S, V>>>,

    /// Thread safe Wasm gas global initialized by Wasmer for every new contract call
    pub wasmer_gas_global: Arc<Mutex<WasmerGasGlobal>>,

    /// Counter of calls, starting with zero and increases for every Internal Call
    pub call_counter: u32,

    /// Call Transaction consists of information such as target_address, gas limit, and data which is parameters provided to contract.
    /// In Internal Call, target address of the contract being called could be child contract.
    pub call_tx: CallTx,

    /// Blockchain data as an input to state transition
    pub params_from_blockchain: BlockchainParams,

    /// Indicator of whether this environment is created for a view call.
    pub is_view: bool,

    #[wasmer(export)]
    pub memory: LazyInit<Memory>,

    #[wasmer(export(name = "alloc"))]
    pub alloc: LazyInit<NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>>>,
}

impl<'a, S, V> Env<'a, S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    /// env is a helper function to create an Env, which is an object used in functions exported to smart
    /// contract modules.
    pub fn new(
        context: Arc<Mutex<TransitionContext<'a, S, V>>>,
        call_counter: u32,
        is_view: bool,
        call_tx: CallTx,
        params_from_blockchain: BlockchainParams,
    ) -> Env<'a, S, V> {
        Env {
            context,
            call_counter,
            wasmer_gas_global: Arc::new(Mutex::new(WasmerGasGlobal::new())),
            memory: LazyInit::default(),
            alloc: LazyInit::default(),
            call_tx,
            params_from_blockchain,
            is_view,
        }
    }

    /// initialize the variable with a global exposed by Wasmer
    pub fn init_wasmer_gas_global(&self, global: Global) {
        self.wasmer_gas_global.lock().unwrap().write(global);
    }

    /// drop the variable wasmer_remaining_points
    pub fn clear_wasmer_gas_global(&self) {
        self.wasmer_gas_global.lock().unwrap().clear();
    }
}

impl<'a, 'b, S, V> MemoryContext for Env<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn get_memory(&self) -> &Memory {
        self.memory_ref().unwrap()
    }

    fn get_alloc(&self) -> &NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>> {
        self.alloc_ref().unwrap()
    }
}
