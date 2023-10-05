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

use pchain_types::{
    blockchain::{Command, CommandReceipt, ExitStatus, Receipt, Transaction},
    cryptography::PublicAddress,
};
use pchain_world_state::{states::WorldState, storage::WorldStateStorage};

use crate::{
    contract::SmartContractContext,
    execution::{cache::WorldStateCache, execute_commands, state::ExecutionState},
    execution::{execute_next_epoch_command, execute_view, gas::GasMeter},
    types::{BaseTx, DeferredCommand},
    BlockchainParams, Cache, TransitionError,
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
        let base_tx = BaseTx::from(&tx);
        let commands = tx.commands;

        // initial state for transition
        let state = ExecutionState {
            tx: base_tx,
            ctx,
            bd,
            receipt: Default::default(),
        };

        // initiate command execution
        if commands.iter().any(|c| matches!(c, Command::NextEpoch)) {
            execute_next_epoch_command(state, commands)
        } else {
            execute_commands::execute_commands(state, commands)
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
            receipt: Default::default(),
        };

        // execute view
        execute_view(state, target, method, arguments)
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

    /// Commands that deferred from a Call Command via host function specified in CBI.
    pub deferred_commands: Vec<DeferredCommand>,

    /// GasMeter holding state for entire txn
    pub gas_meter: GasMeter<S>,
}

impl<S> TransitionContext<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    pub fn new(ws: WorldState<S>, gas_limit: u64) -> Self {
        let host_gm = GasMeter::new(WorldStateCache::new(ws), gas_limit);

        Self {
            sc_context: SmartContractContext {
                cache: None,
                memory_limit: None,
            },
            deferred_commands: Vec::new(),
            gas_meter: host_gm,
        }
    }

    /// Get the World State Cache which allows read-write without gas metering.
    pub fn inner_ws_cache(&self) -> &WorldStateCache<S> {
        &self.gas_meter.ws_cache
    }

    /// Get the mutable World State Cache which allows read-write without gas metering.
    pub fn inner_ws_cache_mut(&mut self) -> &mut WorldStateCache<S> {
        &mut self.gas_meter.ws_cache
    }

    /// Consume itself to get the World State Cache. It can be used when the transition context is
    /// no longer needed (e.g. at the end of transition).
    pub fn into_ws_cache(self) -> WorldStateCache<S> {
        self.gas_meter.ws_cache
    }

    /// Discard the changes to world state
    pub fn revert_changes(&mut self) {
        self.gas_meter.ws_cache.revert();
    }

    // - TODO 8 - Potentially part of command lifecycle refactor
    //
    // IMPORTANT: This function must be called after each command execution, whether success or fail
    // as all the tallying and state changes happen here.
    //
    /// Output the CommandReceipt and clear the intermediate context for next command execution.
    pub fn extract(
        &mut self,
        exit_status: ExitStatus,
    ) -> (CommandReceipt, Option<Vec<DeferredCommand>>) {
        // 1. Take the fields from output cache and update to gas meter at this checkpoint
        let (gas_used, logs, return_values) = self.gas_meter.take_current_command_result();

        // 2. Clear data for next command execution
        let deferred_commands = (!self.deferred_commands.is_empty())
            .then_some(std::mem::take(&mut self.deferred_commands));

        (
            CommandReceipt {
                exit_status,
                gas_used,
                return_values,
                logs,
            },
            deferred_commands,
        )
    }
}
