/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

// TODO 1 - purpose and relationship

use crate::{execution::state::ExecutionState, TransitionError};
use pchain_world_state::{VersionProvider, DB};

/// Causes all World State changes in the Commands Phase to be reverted.
macro_rules! abort {
    ($state:ident, $err_var:path ) => {
        return {
            $state.ctx.revert_changes();
            Err($err_var)
        }
    };
}

pub(crate) use abort;

/// Returns relevant error on gas exhaustion.
pub(crate) fn abort_if_gas_exhausted<S, E, V>(
    state: &mut ExecutionState<S, E, V>,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    if state.tx.gas_limit < state.ctx.gas_meter.total_gas_used() {
        state.ctx.revert_changes();
        return Err(TransitionError::ExecutionProperGasExhausted);
    }
    Ok(())
}
