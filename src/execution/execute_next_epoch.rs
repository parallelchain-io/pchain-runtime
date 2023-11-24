/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! ### Executing Next Epoch Command
//!
//! Next Epoch is a special command that does not go through Pre-Charge Phase or Charge Phase, but
//! will modify the state and update signer's nonce. Its goal is to compute the resulting state of
//! Network Account and return changes to a validator set for next epoch in [TransitionResult].

use pchain_types::blockchain::{Command, CommandReceiptV1, CommandReceiptV2};
use pchain_world_state::{VersionProvider, DB};
// use pchain_world_state::storage::WorldStateStorage;

use crate::{
    commands::protocol, transition::TransitionV2Result, types::CommandKind, TransitionError,
    TransitionV1Result, ValidatorChanges,
};

use super::state::{ExecutionState, FinalizeState};

trait ProtocolCommandHandler<'a, S, E, R, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn handle_invalid_next_epoch_command(state: ExecutionState<'a, S, E, V>) -> R;
    fn handle_post_execution(
        state: ExecutionState<'a, S, E, V>,
        validator_changes: ValidatorChanges,
    ) -> R;
}

/// Execution of NextEpoch Command.
fn execute_next_epoch_command<'a, S, E, R, V, P>(
    state: ExecutionState<'a, S, E, V>,
    commands: Vec<Command>,
) -> R
where
    S: DB + Send + Sync + Clone + 'static,
    P: ProtocolCommandHandler<'a, S, E, R, V>,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    let signer = state.tx.signer;

    // Validate the input transaction:
    // - There can only be one NextEpoch Command in a transaction.
    // - Block performance is required for execution of next epoch transaction.
    // - Transaction nonce matches with the nonce in state

    let ws_cache = state.ctx.inner_ws_cache();
    let nonce = ws_cache
        .ws
        .account_trie()
        .nonce(&signer)
        .expect(&format!("Account trie should get nonce for {:?}", signer));

    if commands.len() != 1
        || commands.first() != Some(&Command::NextEpoch)
        || state.bd.validator_performance.is_none()
        || state.tx.nonce != nonce
    {
        return P::handle_invalid_next_epoch_command(state);
    }

    // State transition
    let (mut state, new_vs) = protocol::next_epoch(state);

    // Update Nonce for the transaction. This step ensures future epoch transaction produced
    // by the signer will have different transaction hash.
    let ws_cache = state.ctx.inner_ws_cache_mut();
    let nonce = nonce.saturating_add(1);
    ws_cache
        .ws
        .account_trie_mut()
        .set_nonce(&signer, nonce)
        .expect(&format!("Account trie should set nonce for {:?}", signer));

    P::handle_post_execution(state, new_vs)
}

struct ExecuteNextEpochV1;

impl<'a, S, V> ProtocolCommandHandler<'a, S, CommandReceiptV1, TransitionV1Result<'a, S, V>, V>
    for ExecuteNextEpochV1
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn handle_invalid_next_epoch_command(
        state: ExecutionState<'a, S, CommandReceiptV1, V>,
    ) -> TransitionV1Result<'a, S, V> {
        TransitionV1Result {
            new_state: state.ctx.into_ws_cache().ws,
            receipt: None,
            error: Some(TransitionError::InvalidNextEpochCommand),
            validator_changes: None,
        }
    }

    fn handle_post_execution(
        mut state: ExecutionState<'a, S, CommandReceiptV1, V>,
        validator_changes: ValidatorChanges,
    ) -> TransitionV1Result<'a, S, V> {
        // Extract receipt from current execution result
        state.finalize_cmd_receipt_collect_deferred(CommandKind::NextEpoch, &Ok(()));

        // Commit to New world state
        let (new_state, receipt) = state.finalize();
        TransitionV1Result {
            new_state,
            error: None,
            validator_changes: Some(validator_changes),
            receipt: Some(receipt),
        }
    }
}

struct ExecuteNextEpochV2;

impl<'a, S, V> ProtocolCommandHandler<'a, S, CommandReceiptV2, TransitionV2Result<'a, S, V>, V>
    for ExecuteNextEpochV2
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    fn handle_invalid_next_epoch_command(
        state: ExecutionState<'a, S, CommandReceiptV2, V>,
    ) -> TransitionV2Result<'a, S, V> {
        TransitionV2Result {
            new_state: state.ctx.into_ws_cache().ws,
            receipt: None,
            error: Some(TransitionError::InvalidNextEpochCommand),
            validator_changes: None,
        }
    }

    fn handle_post_execution(
        mut state: ExecutionState<'a, S, CommandReceiptV2, V>,
        validator_changes: ValidatorChanges,
    ) -> TransitionV2Result<'a, S, V> {
        // Extract receipt from current execution result
        state.finalize_cmd_receipt_collect_deferred(CommandKind::NextEpoch, &Ok(()));

        // Commit to New world state
        let (new_state, receipt) = state.finalize();
        TransitionV2Result {
            new_state,
            error: None,
            validator_changes: Some(validator_changes),
            receipt: Some(receipt),
        }
    }
}

/// Execution entry point for Next Epoch, returning a result with ReceiptV1
pub(crate) fn execute_next_epoch_v1<S, V>(
    state: ExecutionState<S, CommandReceiptV1, V>,
    commands: Vec<Command>,
) -> TransitionV1Result<S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    execute_next_epoch_command::<_, _, _, _, ExecuteNextEpochV1>(state, commands)
}

/// Execution entry point for Next Epoch, returning a result with ReceiptV2
pub(crate) fn execute_next_epoch_v2<S, V>(
    state: ExecutionState<S, CommandReceiptV2, V>,
    commands: Vec<Command>,
) -> TransitionV2Result<S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    execute_next_epoch_command::<_, _, _, _, ExecuteNextEpochV2>(state, commands)
}
