/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines the environment provided when instantiating a Wasm module (using Wasmer).
//!
//! This environment forms part of the Wasm import object provided to Wasmer,
//! and is created during the instantiation of [ContractModule](crate::contract::module::ContractModule).
//!
//! The environment tracks state changes happening inside a contract call.
//! These changes include read-write operations on World State (by encapsulating [TransitionContext]),
//! gas consumed and context related to cross-contract calls.

use pchain_world_state::{VersionProvider, DB};
use std::sync::{Arc, Mutex};
use wasmer::{Global, LazyInit, Memory, NativeFunc};

use super::memory::MemoryContext;
use crate::{context::TransitionContext, gas::WasmerGasGlobal, types::CallTx, BlockchainParams};

/// The Environment is implemented as an Env struct tracking relevant state variables.
/// From wasmer, we derive the necessary WasmerEnv trait for the Env struct to be used to
/// create a Wasm import object.
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

    /// Link to the linear memory instance boostrapped by the relevant Wasmer instance
    #[wasmer(export)]
    pub memory: LazyInit<Memory>,

    /// Link to the Wasmer function to allocate linear memory from the relevant Wasmer instance
    #[wasmer(export(name = "alloc"))]
    pub alloc: LazyInit<NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>>>,
}

impl<'a, S, V> Env<'a, S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    /// bootstraps a new instance of Env
    /// including various uninitialized variables (wasmer_gas_global, memory, alloc)
    /// that would be initialized only after the Wasm module is instantiated by Wasmer
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

    /// initializes wasmer_gas_global with provided ref from the Wasmer instance
    pub fn init_wasmer_gas_global(&self, global: Global) {
        self.wasmer_gas_global.lock().unwrap().init(global);
    }

    /// drops wasmer_gas_global
    pub fn drop_wasmer_gas_global(&self) {
        self.wasmer_gas_global.lock().unwrap().deinit();
    }
}

/// Impl MemoryContext for Env to expose methods for linear memory access.
/// These methods will be used by host functions.
impl<'a, 'b, S, V> MemoryContext for Env<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// # Panics
    /// Will panic if the Wasm instance fails to initialize linear memory correctly.
    /// This will render the Wasm instance unusable.
    fn memory(&self) -> &Memory {
        self.memory_ref().unwrap()
    }

    /// # Panics
    /// Will panic if the native function to allocate linear memory is not found.
    fn alloc(&self) -> &NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>> {
        self.alloc_ref().unwrap()
    }
}
