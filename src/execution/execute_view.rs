/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::{
    blockchain::{CommandReceipt, ExitStatus},
    cryptography::PublicAddress,
};
use pchain_world_state::storage::WorldStateStorage;

use crate::{commands::account, transition::StateChangesResult, TransitionError};

use super::state::ExecutionState;

/// Execute a View Call
pub(crate) fn execute_view<S>(
    state: ExecutionState<S>,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
) -> (CommandReceipt, Option<TransitionError>)
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    match account::call(state, true, target, method, arguments, None) {
        // not yet finish. continue command execution with resulting state
        Ok(mut state_of_success_execution) => {
            let cmd_receipt = state_of_success_execution.ctx.extract(ExitStatus::Success);
            (cmd_receipt, None)
        }
        Err(StateChangesResult {
            state: mut state_of_abort_result,
            error,
        }) => {
            let cmd_receipt = state_of_abort_result
                .ctx
                .extract(error.as_ref().unwrap().into());
            (cmd_receipt, error)
        }
    }
}
