/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct as Execution State which is being updated during execution.
//!
//! This state is not as same as the concept of state in World State. Execution encapsulates the changing information
//! during execution life-cycle. It is the state of execution model, but not referring to blockchain storage.

use pchain_types::blockchain::{ExitStatus, Receipt};
use pchain_world_state::{states::WorldState, storage::WorldStateStorage};

use crate::{
    transition::TransitionContext,
    types::{BaseTx, DeferredCommand},
    BlockchainParams,
};

use super::cache::ReceiptCache;

/// ExecutionState is a collection of all useful information required to transit an state through Phases.
/// Methods defined in ExecutionState do not directly update data to world state, but associate with the
/// [crate::read_write_set::ReadWriteSet] in [TransitionContext] which serves as a data cache in between runtime and world state.
pub(crate) struct ExecutionState<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /*** Transaction ***/
    /// Base Transaction as a transition input
    pub tx: BaseTx,

    /*** Blockchain ***/
    /// Blockchain data as a transition input
    pub bd: BlockchainParams,

    /*** World State ***/
    /// Transition Context which also contains world state as input
    pub ctx: TransitionContext<S>,

    /*** Command Receipts ***/
    pub receipt: ReceiptCache,
}

impl<S> ExecutionState<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub(crate) fn finalize_command_receipt(
        &mut self,
        exit_status: ExitStatus,
        is_deferred_command: bool,
    ) -> Option<Vec<DeferredCommand>> {
        // extract receipt from current execution result
        let (cmd_receipt, deferred_commands_from_call) = self.ctx.extract(exit_status);

        if is_deferred_command {
            self.receipt.push_deferred_command_receipt(cmd_receipt);
        } else {
            self.receipt.push_command_receipt(cmd_receipt);
        }

        deferred_commands_from_call
    }

    /// finalize the world state
    pub(crate) fn finalize(self) -> (WorldState<S>, Receipt) {
        (
            self.ctx.into_ws_cache().commit_to_world_state(),
            Receipt::from(self.receipt),
        )
    }
}
