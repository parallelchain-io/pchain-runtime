/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::{
    blockchain::{CommandReceiptV1, ExitCodeV1},
    cryptography::PublicAddress,
};
use pchain_world_state::storage::WorldStateStorage;

use crate::{commands::account, TransitionError};

use super::state::ExecutionState;

/// Execute a View Call
pub(crate) fn execute_view<S>(
    mut state: ExecutionState<S>,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
) -> (CommandReceiptV1, Option<TransitionError>)
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let (exit_code, transition_error) = match account::call(&mut state, true, target, method, arguments, None) {
        Ok(()) => (ExitCodeV1::Success, None),
        Err(error) => (ExitCodeV1::from(&error), Some(error))
    };
    let (gas_used, command_output, _) = state.ctx.extract();

    (
        CommandReceiptV1 {
            exit_code,
            gas_used, 
            logs: command_output.logs, 
            return_values: command_output.return_values
        },
        transition_error
    )
}
