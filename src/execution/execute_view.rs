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
    match account::call(&mut state, true, target, method, arguments, None) {
        Ok(()) => {
            let (cmd_receipt, _) = state.ctx.extract(ExitCodeV1::Success);
            (cmd_receipt, None)
        }
        Err(error) => {
            let (cmd_receipt, _) = state.ctx.extract(ExitCodeV1::from(&error));
            (cmd_receipt, Some(error))
        }
    }
}
