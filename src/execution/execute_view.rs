/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Manages the execution of [view calls](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Contracts.md#view-calls) in smart contracts.
//!
//! View Calls enable the invocation of read-only methods in contracts without gas charging, as they do not modify the blockchain state.
//! Unlike transactions involving state changes, View Calls omit the Pre-charge and Charge phases, focusing solely on execution.
//!
//! They result in a [CommandReceiptV1] or [CommandReceiptV2], similar to regular command executions.
//! However, the `gas_used` in View Calls serves only as a reference, given the absence of actual gas consumption.

use pchain_types::{
    blockchain::{CallReceipt, CommandReceiptV1, CommandReceiptV2, ExitCodeV1, ExitCodeV2},
    cryptography::PublicAddress,
};
use pchain_world_state::{VersionProvider, DB};

use crate::{commands::account, TransitionError};

use super::state::ExecutionState;

/// Execution entry point for a single View call, returning a result with CommandReceiptV1
pub(crate) fn execute_view_v1<S, V>(
    mut state: ExecutionState<S, CommandReceiptV1, V>,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
) -> (CommandReceiptV1, Option<TransitionError>)
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    let (exit_code, transition_error) =
        match account::call(&mut state, true, target, method, arguments, None) {
            Ok(()) => (ExitCodeV1::Success, None),
            Err(error) => (ExitCodeV1::from(&error), Some(error)),
        };
    let (gas_used, command_output, _) = state.ctx.complete_cmd_execution();

    (
        CommandReceiptV1 {
            exit_code,
            gas_used,
            logs: command_output.logs,
            return_values: command_output.return_value,
        },
        transition_error,
    )
}

/// Execution entry point for a single View call, returning a result with CommandReceiptV2
pub(crate) fn execute_view_v2<S, V>(
    mut state: ExecutionState<S, CommandReceiptV2, V>,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
) -> (CommandReceiptV2, Option<TransitionError>)
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    let (exit_code, transition_error) =
        match account::call(&mut state, true, target, method, arguments, None) {
            Ok(()) => (ExitCodeV2::Ok, None),
            Err(error) => (ExitCodeV2::from(&error), Some(error)),
        };
    let (gas_used, command_output, _) = state.ctx.complete_cmd_execution();

    (
        CommandReceiptV2::Call(CallReceipt {
            exit_code,
            gas_used,
            logs: command_output.logs,
            return_value: command_output.return_value,
        }),
        transition_error,
    )
}
