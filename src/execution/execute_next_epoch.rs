/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::blockchain::{Command, ExitCodeV1, ExitCodeV2, CommandReceiptV1};
use pchain_world_state::storage::WorldStateStorage;

use crate::{commands::protocol, TransitionError, TransitionResultV1, transition::TransitionResultV2, types::CommandKind};

use super::{state::{ExecutionState, FinalizeState}, cache::receipt_cache::{self, ReceiptCacher}};

/// Execution of NextEpoch Command.
pub(crate) fn execute_next_epoch_command_v1<S>(
    state: ExecutionState<S>,
    commands: Vec<Command>,
) -> TransitionResultV1<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
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
        return TransitionResultV1 {
            new_state: state.ctx.into_ws_cache().ws,
            receipt: None,
            error: Some(TransitionError::InvalidNextEpochCommand),
            validator_changes: None,
        };
    }

    // State transition
    let (mut state, new_vs) = protocol::next_epoch(state);

    // Update Nonce for the transaction. This step ensures future epoch transaction produced
    // by the signer will have different transaction hash.
    let ws_cache = state.ctx.inner_ws_cache_mut();
    let nonce = ws_cache.ws.nonce(signer).saturating_add(1);
    ws_cache.ws.with_commit().set_nonce(signer, nonce);

    // Extract receipt from current execution result
    let (gas_used, command_output, _) = state.ctx.extract();
    state.receipt.push_command_receipt(
        CommandReceiptV1 {
            exit_code: ExitCodeV1::Success,
            gas_used,
            logs: command_output.logs,
            return_values: command_output.return_values
        }
    );

    // Commit to New world state
    let (new_state, receipt) = state.finalize();

    TransitionResultV1 {
        new_state,
        error: None,
        validator_changes: Some(new_vs),
        receipt: Some(receipt),
    }
}

/// Execution of NextEpoch Command.
pub(crate) fn execute_next_epoch_command_v2<S>(
    state: ExecutionState<S>,
    commands: Vec<Command>,
) -> TransitionResultV2<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
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
        return TransitionResultV2 {
            new_state: state.ctx.into_ws_cache().ws,
            receipt: None,
            error: Some(TransitionError::InvalidNextEpochCommand),
            validator_changes: None,
        };
    }

    // State transition
    let (mut state, new_vs) = protocol::next_epoch(state);

    // Update Nonce for the transaction. This step ensures future epoch transaction produced
    // by the signer will have different transaction hash.
    let ws_cache = state.ctx.inner_ws_cache_mut();
    let nonce = ws_cache.ws.nonce(signer).saturating_add(1);
    ws_cache.ws.with_commit().set_nonce(signer, nonce);

    // Extract receipt from current execution result
    let (gas_used, command_output, _) = state.ctx.extract();
    state.receipt.push_command_receipt(
        receipt_cache::create_executed_receipt_v2(&CommandKind::NextEpoch, ExitCodeV2::Ok, gas_used, command_output)
    );

    // Commit to New world state
    let (new_state, receipt) = state.finalize();

    TransitionResultV2 {
        new_state,
        error: None,
        validator_changes: Some(new_vs),
        receipt: Some(receipt),
    }
}