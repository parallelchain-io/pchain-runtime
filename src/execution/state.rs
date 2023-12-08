/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! ExecutionState encapsulates the entire state of transaction execution.
//! It serves as a central structure holding all inputs and outputs for a transaction's lifecycle,
//! excluding Command data, which is passed separately to execution functions alongside ExecutionState,
//! to emphasize that Command data is separate from mutated state.

use pchain_types::blockchain::{
    CommandReceiptV1, CommandReceiptV2, ExitCodeV1, ExitCodeV2, ReceiptV1, ReceiptV2,
};
use pchain_world_state::{VersionProvider, WorldState, DB};
use receipt_cache::ReceiptCacher;

use crate::{
    context::TransitionContext,
    types::{self, BaseTx, CommandKind, DeferredCommand},
    BlockchainParams, TransitionError,
};

use super::cache::{receipt_cache, CommandReceiptCache};

/// ExecutionState acts as a unified repository of the transaction's current state, including
/// details like submitted transaction data (excluding Commmands), blockchain parameters,
/// and World State (via TransitionContext).
/// It lives for the entire life of a transaction's execution.
pub(crate) struct ExecutionState<'a, S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Base Transaction as a transition input
    pub tx: BaseTx,

    /// Blockchain data as a transition input, which includes the block specific context
    pub bd: BlockchainParams,

    /// Transition Context which also contains World State as input
    pub ctx: TransitionContext<'a, S, V>,

    /// Output cache for Command Receipts, which store the results and metadata of executed commands.
    pub receipt: CommandReceiptCache<E>,
}

impl<'a, S, E, V> ExecutionState<'a, S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    pub fn new(tx: BaseTx, bd: BlockchainParams, ctx: TransitionContext<'a, S, V>) -> Self {
        Self {
            tx,
            bd,
            ctx,
            receipt: CommandReceiptCache::<E>::new(),
        }
    }
}

impl<'a, S, V> FinalizeState<'a, S, ReceiptV1, V> for ExecutionState<'a, S, CommandReceiptV1, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn finalize_receipt(self) -> (WorldState<'a, S, V>, ReceiptV1) {
        let gas_used = self.ctx.gas_meter.total_gas_used_for_executed_commands();
        (
            self.ctx.into_ws_cache().commit_to_world_state(),
            self.receipt.into_receipt(gas_used, &self.tx.command_kinds),
        )
    }
    fn finalize_cmd_receipt_collect_deferred<Q>(
        &mut self,
        _command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    ) -> Option<Vec<DeferredCommand>> {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV1::Success,
            Err(error) => ExitCodeV1::from(error),
        };

        // extract receipt from current execution result
        let (gas_used, command_output, deferred_commands_from_call) =
            self.ctx.complete_cmd_execution();
        self.receipt.push_command_receipt(CommandReceiptV1 {
            exit_code,
            gas_used,
            logs: command_output.logs,
            return_values: command_output.return_value,
        });

        deferred_commands_from_call
    }
    fn finalize_deferred_cmd_receipt<Q>(
        &mut self,
        _command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    ) {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV1::Success,
            Err(error) => ExitCodeV1::from(error),
        };

        // extract receipt from current execution result
        let (gas_used, command_output, _) = self.ctx.complete_cmd_execution();
        self.receipt
            .push_deferred_command_receipt(CommandReceiptV1 {
                exit_code,
                gas_used,
                return_values: command_output.return_value,
                logs: command_output.logs,
            });
    }
}
impl<'a, S, V> FinalizeState<'a, S, ReceiptV2, V> for ExecutionState<'a, S, CommandReceiptV2, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn finalize_receipt(self) -> (WorldState<'a, S, V>, ReceiptV2) {
        let gas_used = self.ctx.gas_meter.total_gas_used_for_executed_commands();
        (
            self.ctx.into_ws_cache().commit_to_world_state(),
            self.receipt.into_receipt(gas_used, &self.tx.command_kinds),
        )
    }
    fn finalize_cmd_receipt_collect_deferred<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    ) -> Option<Vec<DeferredCommand>> {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV2::Ok,
            Err(error) => ExitCodeV2::from(error),
        };

        // extract receipt from current execution result
        let (gas_used, command_output, deferred_commands_from_call) =
            self.ctx.complete_cmd_execution();
        self.receipt
            .push_command_receipt(types::create_executed_cmd_rcp_v2(
                &command_kind,
                exit_code,
                gas_used,
                command_output,
            ));

        deferred_commands_from_call
    }
    fn finalize_deferred_cmd_receipt<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    ) {
        let exit_code = match &execution_result {
            Ok(_) => ExitCodeV2::Ok,
            Err(error) => ExitCodeV2::from(error),
        };

        // extract receipt from current execution result
        let (gas_used, command_output, _) = self.ctx.complete_cmd_execution();
        self.receipt
            .push_deferred_command_receipt(types::create_executed_cmd_rcp_v2(
                &command_kind,
                exit_code,
                gas_used,
                command_output,
            ));
    }
}

/// Methods for finalizing various lifecycle checkpoints during a state transition.
pub(crate) trait FinalizeState<'a, S, R, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Finalize the eectuion of a single deferred command.
    fn finalize_deferred_cmd_receipt<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    );

    /// Finalize the the execution of a single command and return any deferred commands.
    fn finalize_cmd_receipt_collect_deferred<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    ) -> Option<Vec<DeferredCommand>>;

    /// Finalize the state transition and return the final world state and receipt.
    fn finalize_receipt(self) -> (WorldState<'a, S, V>, R);
}
