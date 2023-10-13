/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::blockchain::{Command, CommandReceiptV1, CommandReceiptV2};
use pchain_world_state::storage::WorldStateStorage;

use crate::{commands::protocol, TransitionError, TransitionResultV1, transition::TransitionResultV2, types::CommandKind, ValidatorChanges};

use super::state::{ExecutionState, FinalizeState};

trait ExecutionBehavior<S, E, R>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn handle_invalid_next_epoch_command(state: ExecutionState<S, E>) -> R;
    fn handle_post_execution(state: ExecutionState<S, E>, validator_changes: ValidatorChanges) -> R;
}

/// Execution of NextEpoch Command.
fn execute_next_epoch_command<S, E, R, P>(
    state: ExecutionState<S, E>,
    commands: Vec<Command>,
) -> R
where
    S: WorldStateStorage + Send + Sync + Clone,
    P: ExecutionBehavior<S, E, R>
{
    let signer = state.tx.signer;

    // Validate the input transaction:
    // - There can only be one NextEpoch Command in a transaction.
    // - Block performance is required for execution of next epoch transaction.
    // - Transaction nonce matches with the nonce in state

    let ws_cache = state.ctx.inner_ws_cache();
    if commands.len() != 1
        || commands.first() != Some(&Command::NextEpoch)
        || state.bd.validator_performance.is_none()
        || state.tx.nonce != ws_cache.ws.nonce(signer)
    {
        return P::handle_invalid_next_epoch_command(state)
    }

    // State transition
    let (mut state, new_vs) = protocol::next_epoch(state);

    // Update Nonce for the transaction. This step ensures future epoch transaction produced
    // by the signer will have different transaction hash.
    let ws_cache = state.ctx.inner_ws_cache_mut();
    let nonce = ws_cache.ws.nonce(signer).saturating_add(1);
    ws_cache.ws.with_commit().set_nonce(signer, nonce);

    P::handle_post_execution(state, new_vs)
}


pub(crate) fn execute_next_epoch_v1<S>(
    state: ExecutionState<S, CommandReceiptV1>,
    commands: Vec<Command>
) -> TransitionResultV1<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    execute_next_epoch_command::<_, _, _, ExecuteNextEpochV1>(state, commands)
}

struct ExecuteNextEpochV1;

impl<S> ExecutionBehavior<S, CommandReceiptV1, TransitionResultV1<S>> for ExecuteNextEpochV1
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn handle_invalid_next_epoch_command(state: ExecutionState<S, CommandReceiptV1>) -> TransitionResultV1<S> {
        TransitionResultV1 {
            new_state: state.ctx.into_ws_cache().ws,
            receipt: None,
            error: Some(TransitionError::InvalidNextEpochCommand),
            validator_changes: None,
        }
    }

    fn handle_post_execution(mut state: ExecutionState<S, CommandReceiptV1>, validator_changes: ValidatorChanges) -> TransitionResultV1<S> {
        // Extract receipt from current execution result
        state.finalize_command_receipt(CommandKind::NextEpoch, &Ok(()));

        // Commit to New world state
        let (new_state, receipt) = state.finalize();

        TransitionResultV1 {
            new_state,
            error: None,
            validator_changes: Some(validator_changes),
            receipt: Some(receipt),
        }
    }
}

pub(crate) fn execute_next_epoch_v2<S>(
    state: ExecutionState<S, CommandReceiptV2>,
    commands: Vec<Command>
) -> TransitionResultV2<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    execute_next_epoch_command::<_, _, _, ExecuteNextEpochV2>(state, commands)
}

struct ExecuteNextEpochV2;

impl<S> ExecutionBehavior<S, CommandReceiptV2, TransitionResultV2<S>> for ExecuteNextEpochV2
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn handle_invalid_next_epoch_command(state: ExecutionState<S, CommandReceiptV2>) -> TransitionResultV2<S> {
        TransitionResultV2 {
            new_state: state.ctx.into_ws_cache().ws,
            receipt: None,
            error: Some(TransitionError::InvalidNextEpochCommand),
            validator_changes: None,
        }
    }

    fn handle_post_execution(mut state: ExecutionState<S, CommandReceiptV2>, validator_changes: ValidatorChanges) -> TransitionResultV2<S> {
        // Extract receipt from current execution result
        state.finalize_command_receipt(CommandKind::NextEpoch, &Ok(()));

        // Commit to New world state
        let (new_state, receipt) = state.finalize();

        TransitionResultV2 {
            new_state,
            error: None,
            validator_changes: Some(validator_changes),
            receipt: Some(receipt),
        }
    }
}