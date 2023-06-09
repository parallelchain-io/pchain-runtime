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
    blockchain::{Command, CommandReceipt, ExitStatus, Log, Receipt, Transaction},
    cryptography::PublicAddress,
};
use pchain_world_state::{states::WorldState, storage::WorldStateStorage};
use std::ops::{Deref, DerefMut};

use crate::{
    contract::SmartContractContext,
    cost::CostChange,
    execution::{execute, state::ExecutionState},
    read_write_set::ReadWriteSet,
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
        let mut ctx = TransitionContext::new(ws);
        if let Some(cache) = &self.sc_cache {
            ctx.sc_context.cache = Some(cache.clone());
        }
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
        let mut ctx = TransitionContext::new(ws);
        if let Some(cache) = &self.sc_cache {
            ctx.sc_context.cache = Some(cache.clone());
        }
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
        let rw_set = self.state.ctx.rw_set;

        let new_state = rw_set.commit_to_world_state();

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
    S: WorldStateStorage + Send + Sync + Clone,
{
    /// Running data cache for Read-Write operations during state transition.
    pub rw_set: ReadWriteSet<S>,

    /// Smart contract context for execution
    pub sc_context: SmartContractContext,

    /// Commands that deferred from a Call Comamnd via host function specified in CBI.
    pub commands: Vec<DeferredCommand>,

    /// Gas consumed in transaction, no matter whether the transaction succeeds or fails.
    gas_used: u64,

    /// the gas charged for adding logs and setting return value in receipt.
    pub receipt_write_gas: CostChange,

    /// logs stores the list of events emitted by an execution ordered in the order of emission.
    pub logs: Vec<Log>,

    /// return_value is the value returned by a call transaction using the `return_value` SDK function. It is None if the
    /// execution has not/did not return anything.
    pub return_value: Option<Vec<u8>>,
}

impl<S> TransitionContext<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    pub fn new(ws: WorldState<S>) -> Self {
        Self {
            rw_set: ReadWriteSet::new(ws),
            sc_context: SmartContractContext {
                cache: None,
                memory_limit: None,
            },
            receipt_write_gas: CostChange::default(),
            logs: Vec::new(),
            gas_used: 0,
            return_value: None,
            commands: Vec::new(),
        }
    }

    pub fn gas_consumed(&self) -> u64 {
        self.gas_used
    }

    pub fn set_gas_consumed(&mut self, gas_used: u64) {
        self.gas_used = gas_used
    }

    /// It is equivalent to gas_consumed + chareable_gas. The chareable_gas consists of
    /// - write cost to storage
    /// - read cost to storage
    /// - write cost to receipt (blockchain data)
    pub fn total_gas_to_be_consumed(&self) -> u64 {
        // Gas incurred to be charged
        let chargeable_gas =
            (self.rw_set.write_gas + self.receipt_write_gas + *self.rw_set.read_gas.borrow())
                .values()
                .0;
        self.gas_consumed().saturating_add(chargeable_gas)
    }

    /// Discard the changes to world state
    pub fn revert_changes(&mut self) {
        self.rw_set.reads.borrow_mut().clear();
        self.rw_set.writes.clear();
    }

    /// Output the CommandReceipt and clear the intermediate context for next command execution.
    /// `prev_gas_used` will be needed for getting the intermediate gas consumption.
    pub fn extract(&mut self, prev_gas_used: u64, exit_status: ExitStatus) -> CommandReceipt {
        // 1. Create Command Receipt
        let ret = CommandReceipt {
            exit_status,
            gas_used: self.gas_used.saturating_sub(prev_gas_used),
            // Intentionally retain return_values and logs even if exit_status is failed
            return_values: self
                .return_value
                .clone()
                .map_or(Vec::new(), std::convert::identity),
            logs: self.logs.clone(),
        };
        // 2. Clear data for next command execution
        *self.rw_set.read_gas.borrow_mut() = CostChange::default();
        self.rw_set.write_gas = CostChange::default();
        self.receipt_write_gas = CostChange::default();
        self.logs.clear();
        self.return_value = None;
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

impl<S> Deref for TransitionContext<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    type Target = ReadWriteSet<S>;

    fn deref(&self) -> &Self::Target {
        &self.rw_set
    }
}

impl<S> DerefMut for TransitionContext<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rw_set
    }
}
