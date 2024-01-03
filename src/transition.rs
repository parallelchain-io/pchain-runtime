/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Entry points for state transition functions.
//!
//! The [Runtime] primarily facilitates the execution of state transitions.
//! These transitions process either a [TransactionV1] or [TransactionV2], along with [blockchain parameters](BlockchainParams),
//! resulting in either a [TransitionV1Result] or [TransitionV2Result].
//! This process involves committing deterministic state changes to the [World State](WorldState),
//! forming the foundation for future transitions.
//!
//! State transition outcomes include:
//! - State modifications in the [world state](pchain_world_state)
//! - [ReceiptV1] or [ReceiptV2]
//! - [TransitionError]
//! - [ValidatorChanges] (specific to the [NextEpoch](pchain_types::blockchain::Command::NextEpoch) command)
//!
//! Additionally, the [Runtime] offers methods to execute [view calls](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Contracts.md#view-calls)
//! and to transition between [World State V1](pchain_world_state::V1) and [V2](pchain_world_state::V2).

use pchain_types::{
    blockchain::{
        Command, CommandReceiptV1, CommandReceiptV2, ReceiptV1, ReceiptV2, TransactionV1,
        TransactionV2,
    },
    cryptography::PublicAddress,
};
use pchain_world_state::{VersionProvider, WorldState, DB, V1, V2};

use crate::{
    context::TransitionContext,
    contract::SmartContractContext,
    execution::{
        execute_commands::{execute_commands_v1, execute_commands_v2},
        // execute_commands::{execute_commands_v1, execute_commands_v2},
        execute_next_epoch::{execute_next_epoch_v1, execute_next_epoch_v2},
        execute_view::{execute_view_v1, execute_view_v2},
        state::ExecutionState,
    },
    types::{TxnMetadata, TxnVersion},
    BlockchainParams, Cache, TransitionError,
};

/// A Runtime for state transition.
/// Instances share the same execution logic,
/// but offer tunable configurations such as data cache for smart contract
/// and memory limit allowed for Wasm contract code execution.
#[derive(Default)]
pub struct Runtime {
    sc_context: SmartContractContext,
}

impl Runtime {
    pub fn new() -> Self {
        Default::default()
    }

    /// Specify a cache for contracts already compiled down to machine code. When asked to execute a contract, the Runtime will
    /// first look at its smart contract cache for the contract's machine code. If it is not there yet, it will get the contract's
    /// Wasm bytecode from the world state, compile it down to machine code, and then put it into the smart contract cache. The
    /// next time the Runtime is asked to execute the same contract, it will get its machine code from the cache and skip the
    /// compilation step.
    pub fn set_smart_contract_cache(mut self, sc_cache: Cache) -> Self {
        self.sc_context.cache = Some(sc_cache);
        self
    }

    /// Specify how big Wasm linear memory is allowed to grow in a single contract execution.
    pub fn set_smart_contract_memory_limit(mut self, memory_limit: usize) -> Self {
        self.sc_context.memory_limit = Some(memory_limit);
        self
    }

    /// state transition of world state (WS) from transaction (tx) and blockchain data (bd) as inputs.
    pub fn transition_v1<'a, S, V>(
        &self,
        ws: WorldState<'a, S, V>,
        tx: TransactionV1,
        bd: BlockchainParams,
    ) -> TransitionV1Result<'a, S, V>
    where
        S: DB + Send + Sync + Clone + 'static,
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        // transaction inputs
        let txn_meta = TxnMetadata::from(&tx);
        let commands = tx.commands;

        // create transition context from world state
        let mut ctx = TransitionContext::new(txn_meta.version, ws, tx.gas_limit);
        ctx.sc_context = self.sc_context.clone();

        // initial state for transition
        let state = ExecutionState::new(txn_meta, bd, ctx);

        // initiate command execution
        if commands.iter().any(|c| matches!(c, Command::NextEpoch)) {
            execute_next_epoch_v1(state, commands)
        } else {
            execute_commands_v1(state, commands)
        }
    }

    /// state transition of world state (WS) from transaction (tx) and blockchain data (bd) as inputs.
    pub fn transition_v2<'a, S, V>(
        &self,
        ws: WorldState<'a, S, V>,
        tx: TransactionV2,
        bd: BlockchainParams,
    ) -> TransitionV2Result<'a, S, V>
    where
        S: DB + Send + Sync + Clone + 'static,
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        // transaction inputs
        let txn_meta = TxnMetadata::from(&tx);
        let commands = tx.commands;

        // create transition context from world state
        let mut ctx = TransitionContext::new(txn_meta.version, ws, tx.gas_limit);
        ctx.sc_context = self.sc_context.clone();

        // initial state for transition
        let state = ExecutionState::new(txn_meta, bd, ctx);

        // initiate command execution
        if commands.iter().any(|c| matches!(c, Command::NextEpoch)) {
            execute_next_epoch_v2(state, commands)
        } else {
            execute_commands_v2(state, commands)
        }
    }

    /// view performs view call to a target contract
    pub fn view_v1<'a, S, V>(
        &self,
        ws: WorldState<'a, S, V>,
        gas_limit: u64,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>,
    ) -> (CommandReceiptV1, Option<TransitionError>)
    where
        S: DB + Send + Sync + Clone + 'static,
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        // create transition context from world state
        let mut ctx = TransitionContext::new(TxnVersion::V1, ws, gas_limit);
        ctx.sc_context = self.sc_context.clone();

        // create a dummy transaction
        let dummy_txn_meta = TxnMetadata {
            gas_limit,
            ..Default::default()
        };

        let dummy_bd = BlockchainParams::default();

        // initialize state for executing view call
        let state = ExecutionState::new(dummy_txn_meta, dummy_bd, ctx);

        // execute view
        execute_view_v1(state, target, method, arguments)
    }

    /// view performs view call to a target contract
    pub fn view_v2<'a, S, V>(
        &self,
        ws: WorldState<'a, S, V>,
        gas_limit: u64,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>,
    ) -> (CommandReceiptV2, Option<TransitionError>)
    where
        S: DB + Send + Sync + Clone + 'static,
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        // create transition context from world state
        let mut ctx = TransitionContext::new(TxnVersion::V1, ws, gas_limit);
        ctx.sc_context = self.sc_context.clone();

        // create a dummy transaction
        let dummy_txn_meta = TxnMetadata {
            gas_limit,
            ..Default::default()
        };

        let dummy_bd = BlockchainParams::default();

        // initialize state for executing view call
        let state = ExecutionState::new(dummy_txn_meta, dummy_bd, ctx);

        // execute view
        execute_view_v2(state, target, method, arguments)
    }

    /// upgrades world state from v1 to v2, expects a valid next epoch command
    pub fn transition_v1_to_v2<'a, S: DB + Send + Sync + Clone + 'static>(
        &self,
        ws: WorldState<'a, S, V1>,
        tx: TransactionV1,
        bd: BlockchainParams,
    ) -> TransitionV1ToV2Result<'a, S> {
        let txn_meta = TxnMetadata::from(&tx);
        let commands = tx.commands;

        let mut ctx = TransitionContext::new(txn_meta.version, ws, tx.gas_limit);
        ctx.sc_context = self.sc_context.clone();
        let state = ExecutionState::new(txn_meta, bd, ctx);

        // first execute next epoch
        let TransitionV1Result {
            new_state,
            error,
            receipt,
            validator_changes,
        } = execute_next_epoch_v1(state, commands);

        // rollback if the command is invalid
        if error.is_some() {
            return TransitionV1ToV2Result {
                new_state: None,
                receipt: None,
                error,
                validator_changes: None,
            };
        }

        // on success, transform and return a World State V2
        match WorldState::<S, V1>::upgrade(new_state) {
            Ok(ws) => TransitionV1ToV2Result {
                new_state: Some(ws),
                receipt,
                error: None,
                validator_changes,
            },
            Err(_) => TransitionV1ToV2Result {
                new_state: None,
                receipt: None,
                error: Some(TransitionError::FailedWorldStateUpgrade),
                validator_changes: None,
            },
        }
    }
}

/// Result of a world state upgrade from V1 to V2.
/// Return type of `pchain_runtime::Runtime::upgrade_ws_v1_to_v2`.
#[derive(Clone)]
pub struct TransitionV1ToV2Result<'a, S>
where
    S: DB + Send + Sync + Clone + 'static,
{
    /// Next world state (ws') after upgrading to V2
    pub new_state: Option<WorldState<'a, S, V2>>,
    /// Transaction receipt, always None.
    pub receipt: Option<ReceiptV1>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validator set, always None.
    pub validator_changes: Option<ValidatorChanges>,
}

/// Return type of `pchain_runtime::Runtime::transition_v1`.
#[derive(Clone)]
pub struct TransitionV1Result<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Next world state (ws') after state transition
    pub new_state: WorldState<'a, S, V>,
    /// Transaction receipt. None if no commands were executed,
    /// e.g. due to failing checks in the pre-charge phase
    pub receipt: Option<ReceiptV1>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validator set.
    /// Only from executing the [Next Epoch](pchain_types::blockchain::Command::NextEpoch) Command. None for other commands.
    pub validator_changes: Option<ValidatorChanges>,
}

/// Return type of `pchain_runtime::Runtime::transition_v2`.
///
/// [V1](TransitionV1Result) -> V2: contains ReceiptV2 instead of ReceiptV1
#[derive(Clone)]
pub struct TransitionV2Result<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Next world state (ws') after state transition
    pub new_state: WorldState<'a, S, V>,
    /// Transaction receipt. None if no commands were executed,
    /// e.g. due to failing checks in the pre-charge phase
    pub receipt: Option<ReceiptV2>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validator set.
    /// Only from executing the [Next Epoch](pchain_types::blockchain::Command::NextEpoch) Command. None for other commands.
    pub validator_changes: Option<ValidatorChanges>,
}

/// Defines changes to validator set. It is the transition result from
/// executing Command [NextEpoch](pchain_types::blockchain::Command::NextEpoch).
#[derive(Clone, Debug)]
pub struct ValidatorChanges {
    /// the next validator set in list of tuple of operator address and power
    pub new_validator_set: Vec<(PublicAddress, u64)>,
    /// the list of address of operator who is removed from state
    pub remove_validator_set: Vec<PublicAddress>,
}
