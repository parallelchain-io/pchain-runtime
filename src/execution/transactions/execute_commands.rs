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

use pchain_types::blockchain::{Command, ReceiptV2, ReceiptV1, CommandReceiptV2, CommandReceiptV1};
use pchain_world_state::storage::WorldStateStorage;

use crate::{
    commands::executable::Executable,
    execution::{
        phases::{self},
        state::{ExecutionState, FinalizeState},
    },
    types::{DeferredCommand, CommandKind},
    TransitionResultV1, transition::TransitionResultV2, TransitionError,
};

trait ExecutionBehavior<S, E, R>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn handle_precharge_error(state: ExecutionState<S, E>, error: TransitionError) -> R;
    fn handle_command_execution_result(state: &mut ExecutionState<S, E>, command_kind: CommandKind, execution_result: &Result<(), TransitionError>) -> Option<Vec<DeferredCommand>>;
    fn handle_abort(state: ExecutionState<S, E>, error: TransitionError) -> R;
    fn handle_charge(state: ExecutionState<S, E>) -> R;
}

fn execute_commands<S, E, R, P>(
    mut state: ExecutionState<S, E>,
    commands: Vec<Command>,
) -> R
where
    S: WorldStateStorage + Send + Sync + Clone,
    P: ExecutionBehavior<S, E, R>
{
    // Phase: Pre-Charge
    let pre_charge_result = phases::pre_charge(&mut state);
    if let Err(err) = pre_charge_result {
        return P::handle_precharge_error(state, err)
    }

    // Phase: Command(s)
    let mut executable_commands = ExecutableCommands::new(commands);
    let mut command_index = 0;

    while let Some(executable_command) = executable_commands.next_command() {
        // Execute command
        let (command_kind, execution_result) = match executable_command {
            ExecutableCommand::TransactionCommmand(command) => {
                let command_kind = CommandKind::from(&command);
                let execute_result = command.execute(&mut state, command_index);

                command_index += 1;
                (command_kind, execute_result)
            },
            ExecutableCommand::DeferredCommand(deferred_command) => {
                (
                    CommandKind::from(&deferred_command.command),
                    deferred_command.execute(&mut state, command_index)
                )
            }
        };

        let deferred_commands_from_call = P::handle_command_execution_result(&mut state, command_kind, &execution_result);

        // Proceed execution result
        match execution_result {
            // command execution is not completed, continue with resulting state
            Ok(()) => {
                // append command triggered from Call
                if let Some(commands_from_call) = deferred_commands_from_call {
                    executable_commands.push_deferred_commands(commands_from_call);
                }
            }
            // in case of error, stop and return result
            Err(error) => {
                // Phase: Charge (abort)
                return P::handle_abort(state, error)
            }
        }
    }

    // Phase: Charge
    P::handle_charge(state)
}

pub(crate) fn execute_commands_v1<S>(
    state: ExecutionState<S, CommandReceiptV1>,
    commands: Vec<Command>
) -> TransitionResultV1<S> 
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    execute_commands::<_, _, _, ExecuteCommandsV1>(state, commands)
}

struct ExecuteCommandsV1;

impl<S> ExecutionBehavior<S, CommandReceiptV1, TransitionResultV1<S>> for ExecuteCommandsV1
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn handle_precharge_error(state: ExecutionState<S, CommandReceiptV1>, error: TransitionError) -> TransitionResultV1<S> {
        let (new_state, _): (_, ReceiptV1) = state.finalize();
        return TransitionResultV1 {
            new_state,
            receipt: None,
            error: Some(error),
            validator_changes: None,
        };
    }

    fn handle_command_execution_result(state: &mut ExecutionState<S, CommandReceiptV1>, command_kind: CommandKind, execution_result: &Result<(), TransitionError>) -> Option<Vec<DeferredCommand>> {
        state.finalize_command_receipt(command_kind, &execution_result)
    }

    fn handle_abort(state: ExecutionState<S, CommandReceiptV1>, error: TransitionError) -> TransitionResultV1<S> {
        let (new_state, receipt) = phases::charge(state).finalize();
        
        TransitionResultV1 {
            new_state,
            error: Some(error),
            receipt: Some(receipt),
            validator_changes: None,
        }
    }

    fn handle_charge(state: ExecutionState<S, CommandReceiptV1>) -> TransitionResultV1<S> {
        let (new_state, receipt) = phases::charge(state).finalize();

        TransitionResultV1 {
            new_state,
            error: None,
            receipt: Some(receipt),
            validator_changes: None,
        }
    }
}

pub(crate) fn execute_commands_v2<S>(
    state: ExecutionState<S, CommandReceiptV2>,
    commands: Vec<Command>
) -> TransitionResultV2<S> 
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    execute_commands::<_, _, _, ExecuteCommandsV2>(state, commands)
}

struct ExecuteCommandsV2;

impl<S> ExecutionBehavior<S, CommandReceiptV2, TransitionResultV2<S>> for ExecuteCommandsV2
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn handle_precharge_error(state: ExecutionState<S, CommandReceiptV2>, error: TransitionError) -> TransitionResultV2<S> {
        let (new_state, _): (_, ReceiptV2) = state.finalize();
        return TransitionResultV2 {
            new_state,
            receipt: None,
            error: Some(error),
            validator_changes: None,
        };
    }
    
    fn handle_command_execution_result(state: &mut ExecutionState<S, CommandReceiptV2>, command_kind: CommandKind, execution_result: &Result<(), TransitionError>) -> Option<Vec<DeferredCommand>> {
        state.finalize_command_receipt(command_kind, &execution_result)
    }
    
    fn handle_abort(state: ExecutionState<S, CommandReceiptV2>, error: TransitionError) -> TransitionResultV2<S> {
        let (new_state, receipt) = phases::charge(state).finalize();
        TransitionResultV2 {
            new_state,
            error: Some(error),
            receipt: Some(receipt),
            validator_changes: None,
        }
    }

    fn handle_charge(state: ExecutionState<S, CommandReceiptV2>) -> TransitionResultV2<S> {
        let (new_state, receipt) = phases::charge(state).finalize();
        TransitionResultV2 {
            new_state,
            error: None,
            receipt: Some(receipt),
            validator_changes: None,
        }
    }
}

/// ExecutableCommands is a sequence of ExecutableCommand.
#[derive(Debug)]
pub(crate) struct ExecutableCommands(Vec<ExecutableCommand>);

impl ExecutableCommands {
    // initialize from transaction commands
    fn new(commands: Vec<Command>) -> Self {
        Self(
            commands
                .into_iter()
                .map(ExecutableCommand::TransactionCommmand)
                .rev()
                .collect()
        )
    }

    /// append a sequence of Commands and store as CommandTask with assigned task ID.
    fn push_deferred_commands(&mut self, commands: Vec<DeferredCommand>) {
        self.0.append(&mut Vec::<ExecutableCommand>::from_iter(
            commands.
                into_iter()
                .map(ExecutableCommand::DeferredCommand)
                .rev(),
        ));
    }

    /// Pop the next command to execute
    fn next_command(&mut self) -> Option<ExecutableCommand> {
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