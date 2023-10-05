/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of execution process on a sequence of Commands. The process starts from Pre-Charge phase,
//! and then goes into Commands Phases, and finally Charge Phase.
//!
//! Processes include execution of:
//! - [Commands](pchain_types::blockchain::Command) from a transaction (Account Command and Staking Command).
//! - [View call](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Contracts.md#view-calls).
//! - [Next Epoch](pchain_types::blockchain::Command::NextEpoch) Command.
//!
//! ### Executing Commands from a Transaction
//!
//! It is the normal flow of a transaction. Firstly, basic checking is performed and
//! cancel the execution if it fails, and Balance of signer is deducted beforehand (Pre-Charge).
//!
//! Then Commands are encapsulated into `Command Tasks`. Each command task is an item in
//! a stack. Execution order starts from the top item. When [Call](pchain_types::blockchain::Command::Call)
//! Command is executed successfully with `Deferred Command`, the Deferred Commands are then
//! encapsulated into Command Task and put to the stack. This stack model allows the Deferred Command
//! to be executed right after its parent Call Command in the same way other commands do.
//!
//! Each command task completes with a [Command Receipt](pchain_types::blockchain::CommandReceipt). If
//! it fails, the process aborts and then goes to Charge Phase immediately.
//!
//! Finally in the Charge Phase, the signer balance is adjusted according to the gas used. Some fees are also
//! transferred to Proposer and Treasury.
//!
//! ### Executing a View Call
//!
//! View Call means execution of a contract by calling its view-only methods. There is not Pre-Charge Phase nor
//! Charge Phase. The gas used in the resulting command receipt is only catered for the gas consumption of this
//! contract call.
//!
//! ### Executing Next Epoch Command
//!
//! Next Epoch Command is a special command that does not go through Pre-Charge Phase or Charge Phase, but
//! will modify the state and update signers nonce. Its goal is to compute the resulting state of
//! Network Account and return changes to a validator set for next epoch in [TransitionResult].

use pchain_types::blockchain::{Command, ExitStatus};
use pchain_world_state::storage::WorldStateStorage;

use crate::{
    commands::executable::Executable,
    execution::{
        phases::{self},
        state::ExecutionState,
    },
    types::DeferredCommand,
    TransitionResult,
};

use super::abort::AbortResult;

/// Backbone logic of Commands Execution
pub(crate) fn execute_commands<S>(
    mut state: ExecutionState<S>,
    commands: Vec<Command>,
) -> TransitionResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    // Phase: Pre-Charge
    let pre_charge_result = phases::pre_charge(&mut state);
    if let Err(err) = pre_charge_result {
        return TransitionResult {
            new_state: state.finalize().0,
            receipt: None,
            error: Some(err),
            validator_changes: None,
        };
    }

    // Phase: Command(s)
    // let mut command_task_results = CommandTaskResults::new();
    let mut command_tasks = ExecutableCommands::new();
    command_tasks.append(commands);

    while let Some(command_task) = command_tasks.next_task() {
        // Execute command triggered from the Transaction
        let (ret, is_deferred_command) = match command_task {
            ExecutableCommand::TransactionCommmand(command) => (command.execute(state), false),
            ExecutableCommand::DeferredCommand(deferred_command) => {
                (deferred_command.execute(state), true)
            }
        };

        // Proceed execution result
        state = match ret {
            // command execution is not completed, continue with resulting state
            Ok(mut state_of_success_execution) => {
                // extract receipt from current execution result
                let deferred_commands_from_call = state_of_success_execution
                    .finalize_command_receipt(ExitStatus::Success, is_deferred_command);

                // append command triggered from Call
                if let Some(commands_from_call) = deferred_commands_from_call {
                    command_tasks.append(commands_from_call);
                }

                state_of_success_execution
            }
            // in case of error, create the last Command receipt and return result
            Err(AbortResult {
                state: mut state_of_abort_result,
                error,
            }) => {
                // extract receipt from last execution result
                state_of_abort_result
                    .finalize_command_receipt(ExitStatus::from(&error), is_deferred_command);

                // Phase: Charge (abort)
                let (new_state, receipt) = phases::charge(state_of_abort_result).finalize();

                return TransitionResult {
                    new_state,
                    error: Some(error),
                    receipt: Some(receipt),
                    validator_changes: None,
                };
            }
        };
    }

    // Phase: Charge
    let (new_state, receipt) = phases::charge(state).finalize();

    TransitionResult {
        new_state,
        error: None,
        receipt: Some(receipt),
        validator_changes: None,
    }
}

/// ExecutableCommands is a sequence of ExecutableCommand.
#[derive(Debug)]
pub(crate) struct ExecutableCommands(Vec<ExecutableCommand>);

impl ExecutableCommands {
    fn new() -> Self {
        Self(Vec::new())
    }

    /// append a sequence of Commands and store as CommandTask with assigned task ID.
    fn append<T: Into<ExecutableCommand>>(&mut self, commands: Vec<T>) {
        self.0.append(&mut Vec::<ExecutableCommand>::from_iter(
            commands.into_iter().map(|command| command.into()).rev(),
        ));
    }

    /// Pop the next task to execute
    fn next_task(&mut self) -> Option<ExecutableCommand> {
        self.0.pop()
    }
}

/// Defines types of command to be executed in the Command Execution Phase.
#[derive(Debug)]
pub(crate) enum ExecutableCommand {
    /// The Command that is submitted from Transaction input
    TransactionCommmand(Command),
    /// The Command that is submitted (deferred) from a Contract Call
    DeferredCommand(DeferredCommand),
}

impl From<Command> for ExecutableCommand {
    fn from(command: Command) -> Self {
        ExecutableCommand::TransactionCommmand(command)
    }
}

impl From<DeferredCommand> for ExecutableCommand {
    fn from(deferred_command: DeferredCommand) -> Self {
        ExecutableCommand::DeferredCommand(deferred_command)
    }
}