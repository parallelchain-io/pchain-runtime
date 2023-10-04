/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use crate::{execution::state::ExecutionState, transition::StateChangesResult, TransitionError};

use super::phases::charge;

// TODO 8 - Potentially part of command lifecycle refactor
//
// This is technically not a phase, it's an Event that transits the phase
// Can be re-organised
/// Abort is operation that causes all World State sets in the Commands Phase to be reverted.
pub(crate) fn abort<S>(
    mut state: ExecutionState<S>,
    transition_err: TransitionError,
) -> StateChangesResult<S>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static,
{
    state.ctx.revert_changes();
    charge(state, Some(transition_err))
}

/// finalize gas consumption of this Command Phase. Return Error GasExhaust if gas has already been exhausted
pub(crate) fn abort_if_gas_exhausted<S>(
    state: ExecutionState<S>,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static,
{
    // TODO 8 - Potentially part of command lifecycle refactor

    if state.tx.gas_limit < state.ctx.gas_meter.get_gas_to_be_used_in_theory() {
        return Err(abort(state, TransitionError::ExecutionProperGasExhausted));
    }
    Ok(state)
}
