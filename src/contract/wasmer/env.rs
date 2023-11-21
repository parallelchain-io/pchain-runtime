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
pub(crate) struct Env<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Transition Context
    pub context: Arc<Mutex<TransitionContext<'a, S, V>>>,

    /// gas meter for wasm execution.
    gas_meter: Arc<Mutex<WasmerRemainingGas>>,

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

    /* TODO 96, I still find this a little confusing
    why is there a gas_meter() and .gas_meter, which points to wasmer_remaining_gas
    .gas_meter() returns a new instance of WasmGasMeter,
    which wraps Env itself (through self, WTF mind blown)

    how can something (WamserGasMeter), which we concieve to be a hierachcal child of Env,
    itself wrap a ref to Env?

    the black magic used to achieve this
    lock() returns a subset struct of Env, which is a LockWasmerTransitionContext
    LockWasmerTransitionContext has a gas_meter() method returning WasmGasMeter, conceptually something like "into"
    that turns arounds and wraps the parent Env in a WasmGasMeter

    i smell a cyclic reference
    */

    pub fn lock(&self) -> LockedWasmerTransitionContext<'a, '_, S, V> {
        LockedWasmerTransitionContext {
            env: self,
            context: self.context.lock().unwrap(),
            wasmer_remaining_gas: self.gas_meter.lock().unwrap(),
        }
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

pub(crate) struct LockedWasmerTransitionContext<'a, 'b, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    env: &'b Env<'a, S, V>,
    context: MutexGuard<'b, TransitionContext<'a, S, V>>,
    wasmer_remaining_gas: MutexGuard<'b, WasmerRemainingGas>,
}

// 'a is the lifetime of the DB ref within TransitionContext
// 'b is the lifetime of the Env ref inside LockedWasmerTransitionContext
// 'c is the lifetime of the LockedWasmerTransitionContext ref inside WasmGasMeter, passed to the gas_meter

impl<'a, 'b, S, V> LockedWasmerTransitionContext<'a, 'b, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    pub fn gas_meter(&mut self) -> WasmerGasMeter<'a, '_, S, Env<'a, S, V>, V> {
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
