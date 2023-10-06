/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct as Execution State which is being updated during execution.
//!
//! This state is not as same as the concept of state in World State. Execution encapsulates the changing information
//! during execution life-cycle. It is the state of execution model, but not referring to blockchain storage.

use pchain_types::blockchain::{ExitCodeV1, ReceiptV1, ExitCodeV2, CommandReceiptV1, ReceiptV2};
use pchain_world_state::{states::WorldState, storage::WorldStateStorage};

use crate::{
    transition::TransitionContext,
    types::{BaseTx, DeferredCommand, CommandKind},
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
    pub(crate) fn finalize_deferred_command_receipt_v1(
        &mut self,
        exit_code: ExitCodeV1
    ) {
        // extract receipt from current execution result
        let (gas_used, logs, return_values, _) = self.ctx.extract();
        self.receipt.push_deferred_command_receipt_v1(
            CommandReceiptV1 {
                exit_code,
                gas_used,
                logs,
                return_values
            }
        );
    }

    pub(crate) fn finalize_deferred_command_receipt_v2(
        &mut self,
        command_kind: CommandKind,
        exit_code: ExitCodeV2
    ) {
        // extract receipt from current execution result
        let (gas_used, _, return_value, _) = self.ctx.extract();
        self.receipt.push_deferred_command_receipt_v2(
            command_kind,
                exit_code,
                gas_used,
                return_value
        );
    }

    pub(crate) fn finalize_command_receipt_v1(
        &mut self,
        exit_code: ExitCodeV1,
    ) -> Option<Vec<DeferredCommand>> {
        // extract receipt from current execution result
        let (gas_used, logs, return_values, deferred_commands_from_call) = self.ctx.extract();
        self.receipt.push_command_receipt_v1(
            CommandReceiptV1 {
                exit_code,
                gas_used,
                return_values,
                logs
            }
        );

        deferred_commands_from_call
    }

    pub(crate) fn finalize_command_receipt_v2(
        &mut self,
        command_kind: CommandKind,
        exit_code: ExitCodeV2,
    ) -> Option<Vec<DeferredCommand>> {
        // extract receipt from current execution result
        let (gas_used, logs, return_value, deferred_commands_from_call) = self.ctx.extract();
        self.receipt.push_command_receipt_v2(
            command_kind,
            exit_code,
            gas_used,
            logs,
            return_value
        );

        deferred_commands_from_call
    }

    /// finalize the world state
    pub(crate) fn finalize_v1(self) -> (WorldState<S>, ReceiptV1) {
        (
            self.ctx.into_ws_cache().commit_to_world_state(),
            self.receipt.into_receipt_v1()
        )
    }

    /// finalize the world state
    pub(crate) fn finalize_v2(self) -> (WorldState<S>, ReceiptV2) {
        let gas_used = self.ctx.gas_meter.total_gas_used_for_executed_commands();
        (
            self.ctx.into_ws_cache().commit_to_world_state(),
            self.receipt.into_receipt_v2(&self.tx.command_kinds, gas_used)
        )
    }
}
