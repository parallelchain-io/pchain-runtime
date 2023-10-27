/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! ### Executing a [View call](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Contracts.md#view-calls).
//!
//! A View Call refers to the execution of view-only methods in a contract without actual gas charging.
//! These methods are not allowed to modify the state of blockchain.
//! Unlike the execution of actual Transaction commands, there is neither a Pre-charge nor Charge phase.
//! The gas used in the resulting command receipt is purely for reference.
//!
use pchain_types::{
    blockchain::{CallReceipt, CommandReceiptV1, CommandReceiptV2, ExitCodeV1, ExitCodeV2},
    cryptography::PublicAddress,
};
use pchain_world_state::storage::WorldStateStorage;

use crate::{commands::account, TransitionError};

use super::state::ExecutionState;

/// Execution entry point for a single View call, returning a result with CommandReceiptV1
pub(crate) fn execute_view_v1<S>(
    mut state: ExecutionState<S, CommandReceiptV1>,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
) -> (CommandReceiptV1, Option<TransitionError>)
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let (exit_code, transition_error) =
        match account::call(&mut state, true, target, method, arguments, None) {
            Ok(()) => (ExitCodeV1::Success, None),
            Err(error) => (ExitCodeV1::from(&error), Some(error)),
        };
    let (gas_used, command_output, _) = state.ctx.extract();

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
pub(crate) fn execute_view_v2<S>(
    mut state: ExecutionState<S, CommandReceiptV2>,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
) -> (CommandReceiptV2, Option<TransitionError>)
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let (exit_code, transition_error) =
        match account::call(&mut state, true, target, method, arguments, None) {
            Ok(()) => (ExitCodeV2::Ok, None),
            Err(error) => (ExitCodeV2::from(&error), Some(error)),
        };
    let (gas_used, command_output, _) = state.ctx.extract();

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
