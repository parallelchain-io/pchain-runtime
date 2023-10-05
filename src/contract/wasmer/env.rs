/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines environment for constructing instance of wasmer execution.
//!
//! The environment (Env) keeps track on the data changes happens inside a contract call. Data
//! changes include the read-write operation on world state, the incurring gas consumption and
//! context related to cross-contract calls.

use pchain_world_state::storage::WorldStateStorage;
use std::sync::{Arc, Mutex, MutexGuard};
use wasmer::{Global, LazyInit, Memory, NativeFunc};

use crate::{
    contract::SmartContractContext,
    execution::gas::{WasmerGasMeter, WasmerRemainingGas},
    transition::TransitionContext,
    types::{CallTx, DeferredCommand},
    BlockchainParams,
};

use super::memory::MemoryContext;

/// Env provides the functions in `exports` (which are in turn 'imported' by WASM smart contracts)
/// access to complex functionality that typically cannot cross the host-WASM barrier.
///
/// Wasmer handles everything for us.
#[derive(wasmer::WasmerEnv, Clone)]
pub(crate) struct Env<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Transition Context
    pub context: Arc<Mutex<TransitionContext<S>>>,
    /// gas meter for wasm execution.
    gas_meter: Arc<Mutex<WasmerRemainingGas>>,

    /// counter of calls. It starts with zero and increases for every Internal Calls
    pub call_counter: u32,

    /// Call Transaction consists of information such as target_address, gas limit, and data which is parameters provided to contract.
    /// In Internal Call, target address of the contract being called could be child contract.
    pub call_tx: CallTx,

    /// Blockchain data as an input to state transition
    pub params_from_blockchain: BlockchainParams,

    /// Indicator of whether this environment is used in view calls.
    pub is_view: bool,

    #[wasmer(export)]
    pub memory: LazyInit<Memory>,

    #[wasmer(export(name = "alloc"))]
    pub alloc: LazyInit<NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>>>,
}

impl<S> Env<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// env is a helper function to create an Env, which is an object used in functions exported to smart
    /// contract modules.
    pub fn new(
        context: Arc<Mutex<TransitionContext<S>>>,
        call_counter: u32,
        is_view: bool,
        call_tx: CallTx,
        params_from_blockchain: BlockchainParams,
    ) -> Env<S> {
        Env {
            context,
            call_counter,
            gas_meter: Arc::new(Mutex::new(WasmerRemainingGas::new())),
            memory: LazyInit::default(),
            alloc: LazyInit::default(),
            call_tx,
            params_from_blockchain,
            is_view,
        }
    }

    /// initialize the variable wasmer_remaining_points
    pub fn init_wasmer_remaining_points(&self, global: Global) {
        self.gas_meter.lock().unwrap().write(global);
    }

    /// drop the variable wasmer_remaining_points
    pub fn drop_wasmer_remaining_points(&self) {
        self.gas_meter.lock().unwrap().clear();
    }

    pub fn lock(&self) -> LockWasmerTransitionContext<'_, S> {
        LockWasmerTransitionContext {
            env: self,
            context: self.context.lock().unwrap(),
            wasmer_remaining_gas: self.gas_meter.lock().unwrap(),
        }
    }
}

impl<S> MemoryContext for Env<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    fn get_memory(&self) -> &Memory {
        self.memory_ref().unwrap()
    }

    fn get_alloc(&self) -> &NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>> {
        self.alloc_ref().unwrap()
    }
}

pub(crate) struct LockWasmerTransitionContext<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    env: &'a Env<S>,
    context: MutexGuard<'a, TransitionContext<S>>,
    wasmer_remaining_gas: MutexGuard<'a, WasmerRemainingGas>,
}

impl<'a, S> LockWasmerTransitionContext<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub fn gas_meter(&mut self) -> WasmerGasMeter<'_, S, Env<S>> {
        WasmerGasMeter::new(
            self.env,
            &self.wasmer_remaining_gas,
            &mut self.context.gas_meter,
        )
    }

    pub fn smart_contract_context(&self) -> SmartContractContext {
        self.context.sc_context.clone()
    }

    pub fn append_deferred_command(&mut self, deferred_command: DeferredCommand) {
        self.context.deferred_commands.push(deferred_command);
    }
}
