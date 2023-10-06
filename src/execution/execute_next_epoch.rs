/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::blockchain::{Command, ExitCodeV1};
use pchain_world_state::storage::WorldStateStorage;

use crate::{commands::protocol, TransitionError, TransitionResult};

use super::state::ExecutionState;

/// Execution of NextEpoch Command.
pub(crate) fn execute_next_epoch_command<S>(
    state: ExecutionState<S>,
    commands: Vec<Command>,
) -> TransitionResult<S>
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
        return TransitionResult {
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
    let (cmd_receipt, _) = state.ctx.extract(ExitCodeV1::Success);
    state.receipt.push_command_receipt(cmd_receipt);

    // Commit to New world state
    let (new_state, receipt) = state.finalize();

    TransitionResult {
        new_state,
        error: None,
        validator_changes: Some(new_vs),
        receipt: Some(receipt),
    }
}
