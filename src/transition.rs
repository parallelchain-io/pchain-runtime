/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of state transition function.
//!
//! The struct [Runtime] is an entry point to trigger state transition. It provides method to
//! intake a [Transaction] with [blockchain parameters](BlockchainParams) and then executes over
//! the [World State](WorldState). As a result, it commits a deterministic change of state to the
//! World State which can be inputted to the next state transition.
//!
//! The result of state transition includes
//! - State changes to [world state](pchain_world_state)
//! - [Receipt]
//! - [Transition Error](TransitionError)
//! - [ValidatorChanges] (for [NextEpoch](pchain_types::blockchain::Command::NextEpoch) command)
//!
//! [Runtime] also provides method to execute a [view call](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Contracts.md#view-calls).

use pchain_types::serialization::Serializable;
use pchain_types::{
    blockchain::{Command, CommandReceipt, ExitStatus, Receipt, Transaction},
    cryptography::PublicAddress,
};
use pchain_world_state::{states::WorldState, storage::WorldStateStorage};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use crate::execution::runtime_gas_meter::RuntimeGasMeter;
use crate::{
    contract::SmartContractContext,
    cost::CostChange,
    execution::{execute, state::ExecutionState},
    read_write_set::WorldStateCache,
    types::{BaseTx, DeferredCommand},
    wasmer::cache::Cache,
    BlockchainParams, TransitionError,
};

/// Version of Contract Binary Interface
#[inline]
pub const fn cbi_version() -> u32 {
    crate::contract::CBI_VERSION
}

/// A Runtime for state transition. Instants of runtime share the same execution logic, but
/// differ in configurations such as data cache for smart contract and memory limit to WASM execution.
pub struct Runtime {
    /// Smart Contract Cache
    sc_cache: Option<Cache>,
    /// Memory limit to wasm linear memory in contract execution
    sc_memory_limit: Option<usize>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            sc_cache: None,
            sc_memory_limit: None,
        }
    }

    /// specify smart contract cache to improve performance for contract code compilation.
    pub fn set_smart_contract_cache(mut self, sc_cache: Cache) -> Self {
        self.sc_cache = Some(sc_cache);
        self
    }

    /// specify the limit to wasm linear memory in contract execution.
    /// It is a tunable maximum guest memory limit that is made available to the VM
    pub fn set_smart_contract_memory_limit(mut self, memory_limit: usize) -> Self {
        self.sc_memory_limit = Some(memory_limit);
        self
    }

    /// state transition of world state (WS) from transaction (tx) and blockchain data (bd) as inputs.
    pub fn transition<S: WorldStateStorage + Send + Sync + Clone + 'static>(
        &self,
        ws: WorldState<S>,
        tx: Transaction,
        bd: BlockchainParams,
    ) -> TransitionResult<S> {
        // create transition context from world state
        let mut ctx = TransitionContext::new(ws, tx.gas_limit);
        ctx.sc_context.cache = self.sc_cache.clone();
        ctx.sc_context.memory_limit = self.sc_memory_limit;

        // transaction inputs
        let tx_size = tx.serialize().len();
        let base_tx = BaseTx::from(&tx);
        let commands = tx.commands;

        // initial state for transition
        let state = ExecutionState {
            tx: base_tx,
            tx_size,
            commands_len: commands.len(),
            ctx,
            bd,
        };

        // initiate command execution
        if commands.iter().any(|c| matches!(c, Command::NextEpoch)) {
            execute::execute_next_epoch_command(state, commands)
        } else {
            execute::execute_commands(state, commands)
        }
    }

    /// view performs view call to a target contract
    pub fn view<S: WorldStateStorage + Send + Sync + Clone + 'static>(
        &self,
        ws: WorldState<S>,
        gas_limit: u64,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>,
    ) -> (CommandReceipt, Option<TransitionError>) {
        // create transition context from world state
        let mut ctx = TransitionContext::new(ws, gas_limit);
        ctx.sc_context.cache = self.sc_cache.clone();
        ctx.sc_context.memory_limit = self.sc_memory_limit;

        // create a dummy transaction
        let dummy_tx = BaseTx {
            gas_limit,
            ..Default::default()
        };

        let dummy_bd = BlockchainParams::default();

        // initialize state for executing view call
        let state = ExecutionState {
            tx: dummy_tx,
            bd: dummy_bd,
            ctx,
            // the below fields are not cared in view call
            tx_size: 0,
            commands_len: 0,
        };

        // execute view
        execute::execute_view(state, target, method, arguments)
    }
}

/// Result of state transition. It is the return type of `pchain_runtime::Runtime::transition`.
#[derive(Clone)]
pub struct TransitionResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// New world state (ws') after state transition
    pub new_state: WorldState<S>,
    /// Transaction receipt. None if the transition receipt is not includable in the block
    pub receipt: Option<Receipt>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validate set.
    /// It is specific to [Next Epoch](pchain_types::blockchain::Command::NextEpoch) Command. None for other commands.
    pub validator_changes: Option<ValidatorChanges>,
}

pub(crate) struct StateChangesResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// resulting state in transit
    pub state: ExecutionState<S>,
    /// transition error
    pub error: Option<TransitionError>,
}

impl<S> StateChangesResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub(crate) fn new(
        state: ExecutionState<S>,
        transition_error: Option<TransitionError>,
    ) -> StateChangesResult<S> {
        Self {
            state,
            error: transition_error,
        }
    }

    /// finalize generates TransitionResult
    pub(crate) fn finalize(self, command_receipts: Vec<CommandReceipt>) -> TransitionResult<S> {
        let error = self.error;
        let new_state = self.state.ctx.ws_cache().clone().commit_to_world_state();

        TransitionResult {
            new_state,
            receipt: Some(command_receipts),
            error,
            validator_changes: None,
        }
    }
}

/// Defines changes to validator set. It is the transition result from
/// executing Command [NextEpoch](pchain_types::blockchain::Command::NextEpoch).
#[derive(Clone)]
pub struct ValidatorChanges {
    /// the new validator set in list of tuple of operator address and power
    pub new_validator_set: Vec<(PublicAddress, u64)>,
    /// the list of address of operator who is removed from state
    pub remove_validator_set: Vec<PublicAddress>,
}

/// TransitionContext defines transiting data required for state transition.
#[derive(Clone)]
pub(crate) struct TransitionContext<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Smart contract context for execution
    pub sc_context: SmartContractContext,

    /// Commands that deferred from a Call Comamnd via host function specified in CBI.
    pub commands: Vec<DeferredCommand>,

    /// GasMeter holding state for entire txn
    pub gas_meter: RuntimeGasMeter<S>,
}

impl<S> TransitionContext<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    pub fn new(ws: WorldState<S>, gas_limit: u64) -> Self {
        let host_gm = RuntimeGasMeter::new(WorldStateCache::new(ws), gas_limit);

        Self {
            sc_context: SmartContractContext {
                cache: None,
                memory_limit: None,
            },
            commands: Vec::new(),
            gas_meter: host_gm,
        }
    }

    pub fn ws_cache(&self) -> &WorldStateCache<S> {
        self.gas_meter.ws_cache()
    }

    pub fn ws_cache_mut(&mut self) -> &mut WorldStateCache<S> {
        self.gas_meter.ws_cache_mut()
    }

    pub fn into_ws_cache(self) -> WorldStateCache<S> {
        self.gas_meter.into_ws_cache()
    }

    /// Discard the changes to world state
    pub fn revert_changes(&mut self) {
        self.gas_meter.ws_cache_mut().revert();
    }

    // - TODO 8 - Potentially part of command lifecycle refactor
    //
    // IMPORTANT: This function must be called after each command execution, whether success or fail
    // as all the tallying and state changes happen here.
    //
    /// Output the CommandReceipt and clear the intermediate context for next command execution.
    pub fn extract(&mut self, exit_status: ExitStatus) -> CommandReceipt {
        // 1. Create Command Receipt
        let ret = CommandReceipt {
            exit_status,
            gas_used: self.gas_meter.get_gas_used_for_current_command(),
            return_values: self
                .gas_meter
                .command_return_value
                .clone()
                .map_or(Vec::new(), std::convert::identity),
            logs: self.gas_meter.command_logs.clone(),
        };

        // 2. Write command gas to the total
        self.gas_meter.finalize_command_gas();

        // 3. Clear data for next command execution
        self.commands.clear();
        ret
    }

    /// Pop commands from context. None if there is nothing to pop
    pub fn pop_commands(&mut self) -> Option<Vec<DeferredCommand>> {
        if self.commands.is_empty() {
            return None;
        }
        let mut ret = Vec::new();
        ret.append(&mut self.commands);
        Some(ret)
    }
}