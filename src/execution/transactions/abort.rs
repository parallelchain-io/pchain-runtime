/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_world_state::storage::WorldStateStorage;

use crate::{execution::state::ExecutionState, TransitionError};

/// Abort is operation that causes all World State sets in the Commands Phase to be reverted.
pub(crate) fn abort<S>(
    mut state: ExecutionState<S>,
    transition_err: TransitionError,
) -> AbortResult<S>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static,
{
    state.ctx.revert_changes();
    AbortResult::new(state, transition_err)
}

/// finalize gas consumption of this Command Phase. Return Error GasExhaust if gas has already been exhausted
pub(crate) fn abort_if_gas_exhausted<S>(
    state: ExecutionState<S>,
) -> Result<ExecutionState<S>, AbortResult<S>>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static,
{
    if state.tx.gas_limit < state.ctx.gas_meter.total_gas_used() {
        return Err(abort(state, TransitionError::ExecutionProperGasExhausted));
    }
    Ok(state)
}

pub(crate) struct AbortResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// resulting state in transit
    pub state: ExecutionState<S>,
    /// transition error
    pub error: TransitionError,
}

impl<S> AbortResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub(crate) fn new(
        state: ExecutionState<S>,
        transition_error: TransitionError,
    ) -> AbortResult<S> {
        Self {
            state,
            error: transition_error,
        }
    }
}
