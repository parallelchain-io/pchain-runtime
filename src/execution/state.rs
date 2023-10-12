/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct as Execution State which is being updated during execution.
//!
//! This state is not as same as the concept of state in World State. Execution encapsulates the changing information
//! during execution life-cycle. It is the state of execution model, but not referring to blockchain storage.

use pchain_types::blockchain::{ExitCodeV1, ReceiptV1, ExitCodeV2, ReceiptV2, CommandReceiptV1, CommandReceiptV2};
use pchain_world_state::{states::WorldState, storage::WorldStateStorage};
use receipt_cache::ReceiptCacher;

use crate::{
    transition::TransitionContext,
    types::{BaseTx, DeferredCommand, CommandKind, self},
    BlockchainParams, TransitionError,
};

use super::cache::{CommandReceiptCache, receipt_cache};

/// ExecutionState is a collection of all useful information required to transit an state through Phases.
/// Methods defined in ExecutionState do not directly update data to world state, but associate with the
/// [crate::read_write_set::ReadWriteSet] in [TransitionContext] which serves as a data cache in between runtime and world state.
pub(crate) struct ExecutionState<S, E>
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
    pub receipt: CommandReceiptCache<E>,
}

impl<S, E> ExecutionState<S, E> 
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub fn new(tx: BaseTx, bd: BlockchainParams, ctx: TransitionContext<S>) -> Self {
        Self { tx, bd, ctx, receipt: CommandReceiptCache::<E>::new() }
    }
}

impl<S> FinalizeState<S, CommandReceiptV1, ReceiptV1> for ExecutionState<S, CommandReceiptV1>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    fn finalize(self) -> (WorldState<S>, ReceiptV1) {
        let gas_used = self.ctx.gas_meter.total_gas_used_for_executed_commands();
        (
            self.ctx.into_ws_cache().commit_to_world_state(),
            self.receipt.into_receipt(gas_used, &self.tx.command_kinds)
        )
    }
    fn finalize_command_receipt<Q>(
        &mut self,
        _command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>
    ) -> Option<Vec<DeferredCommand>> {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV1::Success,
            Err(error) => ExitCodeV1::from(error)
        };

        // extract receipt from current execution result
        let (gas_used, command_output, deferred_commands_from_call) = self.ctx.extract();
        self.receipt.push_command_receipt(
            CommandReceiptV1 {
                exit_code,
                gas_used,
                logs: command_output.logs,
                return_values: command_output.return_values
            }
        );

        deferred_commands_from_call
    }
    fn finalize_deferred_command_receipt<Q>(
        &mut self,
        _command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>
    ) {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV1::Success,
            Err(error) => ExitCodeV1::from(error)
        };

        // extract receipt from current execution result
        let (gas_used, command_output, _) = self.ctx.extract();
        self.receipt.push_deferred_command_receipt(
            CommandReceiptV1 {
                exit_code,
                gas_used,
                return_values: command_output.return_values,
                logs: command_output.logs
            }
        );
    }
}
impl<S> FinalizeState<S, CommandReceiptV2, ReceiptV2> for ExecutionState<S, CommandReceiptV2>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    fn finalize(self) -> (WorldState<S>, ReceiptV2) {
        let gas_used = self.ctx.gas_meter.total_gas_used_for_executed_commands();
        (
            self.ctx.into_ws_cache().commit_to_world_state(),
            self.receipt.into_receipt(gas_used, &self.tx.command_kinds)
        )
    }
    fn finalize_command_receipt<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>
    ) -> Option<Vec<DeferredCommand>> {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV2::Ok,
            Err(error) => ExitCodeV2::from(error)
        };

        // extract receipt from current execution result
        let (gas_used, command_output, deferred_commands_from_call) = self.ctx.extract();
        self.receipt.push_command_receipt(
            types::create_executed_receipt_v2(&command_kind, exit_code, gas_used, command_output)
        );

        deferred_commands_from_call
    }
    fn finalize_deferred_command_receipt<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>
    ) {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV2::Ok,
            Err(error) => ExitCodeV2::from(error)
        };

        // extract receipt from current execution result
        let (gas_used, command_output, _) = self.ctx.extract();
        self.receipt.push_deferred_command_receipt(
            types::create_executed_receipt_v2(&command_kind, exit_code, gas_used, command_output)
        );
    }
}

pub(crate) trait FinalizeState<S, E, R>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    fn finalize_deferred_command_receipt<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>
    );

    fn finalize_command_receipt<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>
    ) -> Option<Vec<DeferredCommand>>;

    fn finalize(self) -> (WorldState<S>, R);
}