/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of state transition function.
//!
//! The struct [Runtime] is an entry point to trigger state transition. It provides methods to
//! take in a [Transaction] with [blockchain parameters](BlockchainParams). It then outputs a [TransitionResult]
//! committing a set of deterministic state changes to the [World State](WorldState),
//! which we be included in subsequent state changes
//!
//! The result of state transition includes
//! - State changes to [world state](pchain_world_state)
//! - [Receipt]
//! - [Transition Error](TransitionError)
//! - [ValidatorChanges] (for [NextEpoch](pchain_types::blockchain::Command::NextEpoch) command)
//!
//! [Runtime] also exposes a method to execute a [view call]
//! (https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Contracts.md#view-calls).

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
        execute_next_epoch::{execute_next_epoch_v1, execute_next_epoch_v2},
        execute_view::{execute_view_v1, execute_view_v2},
        state::ExecutionState,
    },
    types::{BaseTx, TxnVersion},
    BlockchainParams, Cache, TransitionError,
};

/// Version of Contract Binary Interface
#[inline]
pub const fn cbi_version() -> u32 {
    crate::contract::CBI_VERSION
}

/// A Runtime for state transition.
/// Instances of runtime share the same execution logic,
/// but offer tunable configurations such as data cache for smart contract and memory limit allowed for Wasm contract code execution.
#[derive(Default)]
pub struct Runtime {
    sc_context: SmartContractContext,
}

impl Runtime {
    pub fn new() -> Self {
        Default::default()
    }

    /// specify smart contract cache to improve performance for contract code compilation.
    pub fn set_smart_contract_cache(mut self, sc_cache: Cache) -> Self {
        self.sc_context.cache = Some(sc_cache);
        self
    }

    /// specify the limit to wasm linear memory in contract execution.
    /// It is a tunable maximum guest memory limit that is made available to the VM
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
        let base_tx = BaseTx::from(&tx);
        let commands = tx.commands;

        // create transition context from world state
        let mut ctx = TransitionContext::new(base_tx.version, ws, tx.gas_limit);
        ctx.sc_context = self.sc_context.clone();

        // initial state for transition
        let state = ExecutionState::new(base_tx, bd, ctx);

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
        let base_tx = BaseTx::from(&tx);
        let commands = tx.commands;

        // create transition context from world state
        let mut ctx = TransitionContext::new(base_tx.version, ws, tx.gas_limit);
        ctx.sc_context = self.sc_context.clone();

        // initial state for transition
        let state = ExecutionState::new(base_tx, bd, ctx);

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
        let dummy_tx = BaseTx {
            gas_limit,
            ..Default::default()
        };

        let dummy_bd = BlockchainParams::default();

        // initialize state for executing view call
        let state = ExecutionState::new(dummy_tx, dummy_bd, ctx);

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
        let dummy_tx = BaseTx {
            gas_limit,
            ..Default::default()
        };

        let dummy_bd = BlockchainParams::default();

        // initialize state for executing view call
        let state = ExecutionState::new(dummy_tx, dummy_bd, ctx);

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
        let base_tx = BaseTx::from(&tx);
        let commands = tx.commands;

        let mut ctx = TransitionContext::new(base_tx.version, ws, tx.gas_limit);
        ctx.sc_context = self.sc_context.clone();
        let state = ExecutionState::new(base_tx, bd, ctx);

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

/// Result of a world state upgrade from V1 to V2. It is the return type of `pchain_runtime::Runtime::upgrade_ws_v1_to_v2`.
#[derive(Clone)]
pub struct TransitionV1ToV2Result<'a, S>
where
    S: DB + Send + Sync + Clone + 'static,
{
    /// New world state (ws') after upgrading to V2
    pub new_state: Option<WorldState<'a, S, V2>>,
    /// Transaction receipt, always None.
    pub receipt: Option<ReceiptV1>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validator set, always None.
    pub validator_changes: Option<ValidatorChanges>,
}

/// Result of state transition. It is the return type of `pchain_runtime::Runtime::transition`.
#[derive(Clone)]
pub struct TransitionV1Result<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// New world state (ws') after state transition
    pub new_state: WorldState<'a, S, V>,
    /// Transaction receipt. None if the transition receipt is not includable in the block
    pub receipt: Option<ReceiptV1>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validate set.
    /// It is specific to [Next Epoch](pchain_types::blockchain::Command::NextEpoch) Command. None for other commands.
    pub validator_changes: Option<ValidatorChanges>,
}

/// Result of state transition. It is the return type of `pchain_runtime::Runtime::transition`.
///
/// [V1](TransitionV1Result) -> V2: contains ReceiptV2 instead of ReceiptV1
#[derive(Clone)]
pub struct TransitionV2Result<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// New world state (ws') after state transition
    pub new_state: WorldState<'a, S, V>,
    /// Transaction receipt. None if the transition receipt is not includable in the block
    pub receipt: Option<ReceiptV2>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validate set.
    /// It is specific to [Next Epoch](pchain_types::blockchain::Command::NextEpoch) Command. None for other commands.
    pub validator_changes: Option<ValidatorChanges>,
}

/// Defines changes to validator set. It is the transition result from
/// executing Command [NextEpoch](pchain_types::blockchain::Command::NextEpoch).
#[derive(Clone, Debug)]
pub struct ValidatorChanges {
    /// the new validator set in list of tuple of operator address and power
    pub new_validator_set: Vec<(PublicAddress, u64)>,
    /// the list of address of operator who is removed from state
    pub remove_validator_set: Vec<PublicAddress>,
}
