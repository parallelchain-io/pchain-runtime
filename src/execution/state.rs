/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct as Execution State which is being updated during execution.
//!
//! This state is not as same as the concept of state in World State. Execution encapsulates the changing information
//! during execution life-cycle. It is the state of execution model, but not referring to blockchain storage.

use pchain_types::blockchain::{
    CommandReceiptV1, CommandReceiptV2, ExitCodeV1, ExitCodeV2, ReceiptV1, ReceiptV2,
};
use pchain_world_state::{VersionProvider, WorldState, DB};
// use pchain_world_state::{states::WorldState, storage::WorldStateStorage, VersionProvider};
use receipt_cache::ReceiptCacher;

use crate::{
    transition::TransitionContext,
    types::{self, BaseTx, CommandKind, DeferredCommand},
    BlockchainParams, TransitionError,
};

use super::cache::{receipt_cache, CommandReceiptCache};

/// ExecutionState is the base struct that contains all information needed for state transition.
/// It implements logic for the different phases of state transition.
pub(crate) struct ExecutionState<'a, S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Base Transaction as a transition input
    pub tx: BaseTx,

    /// Blockchain data as a transition input
    pub bd: BlockchainParams,

    /// Transition Context which also contains World State as input
    pub ctx: TransitionContext<'a, S, V>,

    /// Output cache for Command Receipts
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

pub(crate) trait FinalizeState<'a, S, R, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn finalize_deferred_cmd_receipt<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    );

    fn finalize_cmd_receipt_collect_deferred<Q>(
        &mut self,
        command_kind: CommandKind,
        execution_result: &Result<Q, TransitionError>,
    ) -> Option<Vec<DeferredCommand>>;

    fn finalize_receipt(self) -> (WorldState<'a, S, V>, R);
}
