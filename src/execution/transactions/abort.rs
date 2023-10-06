/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_world_state::storage::WorldStateStorage;

use crate::{execution::state::ExecutionState, TransitionError};

/// Abort is operation that causes all World State sets in the Commands Phase to be reverted.
macro_rules! abort {
    ($state:ident, $err_var:path ) => {
        return {
            $state.ctx.revert_changes();
            Err($err_var)
        }
    };
}

pub(crate) use abort;

/// Return Error GasExhaust if gas has already been exhausted
pub(crate) fn abort_if_gas_exhausted<S>(
    state: &mut ExecutionState<S>,
) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    if state.tx.gas_limit < state.ctx.gas_meter.total_gas_used() {
        state.ctx.revert_changes();
        return Err(TransitionError::ExecutionProperGasExhausted)
    }
    Ok(())
}