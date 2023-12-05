/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines the environment provided when instantiating a Wasm module (specifically using Wasmer).
//! The environment tracks state changes happening inside a contract call.
//! These changes include read-write operations on World State, gas consumed and
//! context related to cross-contract calls.

use pchain_world_state::{VersionProvider, DB};
use std::sync::{Arc, Mutex};
use wasmer::{Global, LazyInit, Memory, NativeFunc};

use crate::{
    execution::gas::WasmerGasGlobal, transition::TransitionContext, types::CallTx, BlockchainParams,
};

use super::memory::MemoryContext;

/// The Environment is implemented as an Env struct tracking relevant state variables.
/// WasmerEnv implements the necessary trait for the Env struct to create a Wasm import object.
#[derive(wasmer::WasmerEnv, Clone)]
pub(crate) struct Env<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Singleton Transition Context
    pub context: Arc<Mutex<TransitionContext<'a, S, V>>>,

    /// Thread safe Wasm gas global. With every new contract call, we initialize a new Wasmer instance and gas global.
    pub wasmer_gas_global: Arc<Mutex<WasmerGasGlobal>>,

    /// Counter of calls, starting with zero and increasing with every Internal Call
    pub call_counter: u32,

    /// Call Transaction consists of information such as target_address, gas limit, and data which are parameters provided to contract.
    /// In an Internal Call, target address of the contract will be the child contract.
    pub call_tx: CallTx,

    /// Blockchain data as an input to state transition
    pub params_from_blockchain: BlockchainParams,

    /// Indicator of whether this environment is created for a view call.
    pub is_view: bool,

    /// Link to the linear memory instance boostrapped by Wasmer
    #[wasmer(export)]
    pub memory: LazyInit<Memory>,

    /// Link to the Wasmer function to allocate linear memory
    #[wasmer(export(name = "alloc"))]
    pub alloc: LazyInit<NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>>>,
}

impl<'a, S, V> Env<'a, S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    /// bootstrap a new instance of Env
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

/// Impl MemoryContext for Env to expose linear memory access to the host functions
impl<'a, 'b, S, V> MemoryContext for Env<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// ### Panics
    /// panics if the linear memory instance is not initialized
    fn get_memory(&self) -> &Memory {
        self.memory_ref().unwrap()
    }

    /// ### Panics
    /// panics if unable if the native function to allocate linear memory is not found
    fn get_alloc(&self) -> &NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>> {
        self.alloc_ref().unwrap()
    }
}
