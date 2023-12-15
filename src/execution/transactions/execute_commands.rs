/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

// TODO 1 - better phrase this

//! Module for managing the lifecycle and execution strategy of the Account or Staking commands.
//
//! These commands are sent by users in a signed transaction,
//! and are executed in accordance to a lifecycle briefly described below.
//! The [CommandStrategy] trait encapsulates strategies for handling different
//! versions of command execution.
//!
//! ### Lifecycle
//! A brief description of the lifecycle is as follows:
//!
//! Firstly, the transaction undergoes validation during the Pre-Charge phase.
//! The execution is cancelled if these checks fail.
//! If the checks pass, the transaction signer's balance will be deducted upfront according to the specified gas limit.
//!
//! Next, Commands are encapsulated into `Command Tasks`. Each command task is an item in
//! a stack. Execution order starts from the top item. When a [Call](pchain_types::blockchain::Command::Call)
//! Command is executed successfully and outputs `Deferred Commands`, these Deferred Commands will be
//! encapsulated into Command Tasks and pushed onto stack. This stack model allows the Deferred Command
//! to be executed sequentially after its parent Call Command.
//!
//! Each Command Task completes with a Command Receipt. If execution fails,
//! the process aborts and then proceeds immediately to the Charge Phase.
//! Alternatively, the subsequent Command Task will
//! be executed until all Tasks are completed.
//!
//! Finally in the Charge Phase, the Signer's balance will be refunded according to the actual gas used.
//! Some fees are also transferred to Proposer and Treasury.

use pchain_types::blockchain::{Command, CommandReceiptV1, CommandReceiptV2, ReceiptV1, ReceiptV2};
use pchain_world_state::{VersionProvider, DB};

use crate::{
    execution::{
        execute::Execute,
        phases::{self},
        state::{ExecutionState, FinalizeState},
    },
    transition::TransitionV2Result,
    types::{CommandKind, DeferredCommand},
    TransitionError, TransitionV1Result,
};

/// Generic command executor
/// which delegates to a specific version of CommandStrategy
fn execute_commands<'a, S, E, V, R, P>(
    mut state: ExecutionState<'a, S, E, V>,
    commands: Vec<Command>,
) -> R
where
    S: DB + Send + Sync + Clone,
    P: CommandStrategy<'a, S, E, R, V>,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    // Phase: Pre-Charge
    let pre_charge_result = phases::pre_charge(&mut state);
    if let Err(err) = pre_charge_result {
        return P::handle_precharge_error(state, err);
    }

    // Phase: Command(s)
    let mut executable_commands = ExecutableCommands::new(commands);
    let mut command_index = 0;

    while let Some(executable_cmd) = executable_commands.next_command() {
        let is_txn_sent_cmd = executable_cmd.is_txn_sent();

        // Execute command
        let cmd_kind = executable_cmd.command_kind();
        let execution_result = executable_cmd.consume_and_execute(&mut state, command_index);

        let deferred_cmds_from_execution = P::handle_command_execution_result(
            &mut state,
            cmd_kind,
            &execution_result,
            is_txn_sent_cmd,
        );

        // Handle potential execution errors
        match execution_result {
            // command execution is not completed, continue with resulting state
            Ok(()) => {
                // append command triggered from Call
                if let Some(cmd) = deferred_cmds_from_execution {
                    executable_commands.push_deferred_commands(cmd);
                }
            }
            // in case of error, stop and return result
            Err(error) => {
                // Phase: Charge (abort)
                return P::handle_abort(state, error);
            }
        }

        // Increment index per user sent command
        if is_txn_sent_cmd {
            command_index += 1;
        }
    }

    // Phase: Charge
    P::handle_charge(state)
}

trait CommandStrategy<'a, S, E, R, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    fn handle_precharge_error(state: ExecutionState<'a, S, E, V>, error: TransitionError) -> R;
    fn handle_command_execution_result(
        state: &mut ExecutionState<S, E, V>,
        command_kind: CommandKind,
        execution_result: &Result<(), TransitionError>,
        is_deferred: bool,
    ) -> Option<Vec<DeferredCommand>>;
    fn handle_abort(state: ExecutionState<'a, S, E, V>, error: TransitionError) -> R;
    fn handle_charge(state: ExecutionState<'a, S, E, V>) -> R;
}

/// Strategy struct for V1 specific execution output
struct ExecuteCommandsV1;

impl<'a, S, V> CommandStrategy<'a, S, CommandReceiptV1, TransitionV1Result<'a, S, V>, V>
    for ExecuteCommandsV1
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    fn handle_precharge_error(
        state: ExecutionState<'a, S, CommandReceiptV1, V>,
        error: TransitionError,
    ) -> TransitionV1Result<'a, S, V> {
        let (new_state, _): (_, ReceiptV1) = state.finalize_receipt();
        TransitionV1Result {
            new_state,
            receipt: None,
            error: Some(error),
            validator_changes: None,
        }
    }

    fn handle_command_execution_result(
        state: &mut ExecutionState<S, CommandReceiptV1, V>,
        command_kind: CommandKind,
        execution_result: &Result<(), TransitionError>,
        is_user_sent: bool,
    ) -> Option<Vec<DeferredCommand>> {
        if is_user_sent {
            return state.finalize_cmd_receipt_collect_deferred(command_kind, execution_result);
        }
        state.finalize_deferred_cmd_receipt(command_kind, execution_result);
        None
    }

    fn handle_abort(
        state: ExecutionState<'a, S, CommandReceiptV1, V>,
        error: TransitionError,
    ) -> TransitionV1Result<'a, S, V> {
        let (new_state, receipt) = phases::charge(state).finalize_receipt();
        TransitionV1Result {
            new_state,
            error: Some(error),
            receipt: Some(receipt),
            validator_changes: None,
        }
    }

    fn handle_charge(
        state: ExecutionState<'a, S, CommandReceiptV1, V>,
    ) -> TransitionV1Result<'a, S, V> {
        let (new_state, receipt) = phases::charge(state).finalize_receipt();
        TransitionV1Result {
            new_state,
            error: None,
            receipt: Some(receipt),
            validator_changes: None,
        }
    }
}

/// Strategy struct for V2 specific execution output
struct ExecuteCommandsV2;

impl<'a, S, V> CommandStrategy<'a, S, CommandReceiptV2, TransitionV2Result<'a, S, V>, V>
    for ExecuteCommandsV2
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    fn handle_precharge_error(
        state: ExecutionState<'a, S, CommandReceiptV2, V>,
        error: TransitionError,
    ) -> TransitionV2Result<'a, S, V> {
        let (new_state, _): (_, ReceiptV2) = state.finalize_receipt();
        TransitionV2Result {
            new_state,
            receipt: None,
            error: Some(error),
            validator_changes: None,
        }
    }

    fn handle_command_execution_result(
        state: &mut ExecutionState<S, CommandReceiptV2, V>,
        command_kind: CommandKind,
        execution_result: &Result<(), TransitionError>,
        is_user_sent: bool,
    ) -> Option<Vec<DeferredCommand>> {
        if is_user_sent {
            return state.finalize_cmd_receipt_collect_deferred(command_kind, execution_result);
        }
        state.finalize_deferred_cmd_receipt(command_kind, execution_result);
        None
    }

    fn handle_abort(
        state: ExecutionState<'a, S, CommandReceiptV2, V>,
        error: TransitionError,
    ) -> TransitionV2Result<'a, S, V> {
        let (new_state, receipt) = phases::charge(state).finalize_receipt();
        TransitionV2Result {
            new_state,
            error: Some(error),
            receipt: Some(receipt),
            validator_changes: None,
        }
    }

    fn handle_charge(
        state: ExecutionState<'a, S, CommandReceiptV2, V>,
    ) -> TransitionV2Result<'a, S, V> {
        let (new_state, receipt) = phases::charge(state).finalize_receipt();
        TransitionV2Result {
            new_state,
            error: None,
            receipt: Some(receipt),
            validator_changes: None,
        }
    }
}
/// ExecutableCommands is a (LIFO) stack of ExecutableCommand
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
                .collect(),
        )
    }

    /// append a sequence of Commands and store as CommandTask with assigned task ID.
    fn push_deferred_commands(&mut self, commands: Vec<DeferredCommand>) {
        self.0.append(&mut Vec::<ExecutableCommand>::from_iter(
            commands
                .into_iter()
                .map(ExecutableCommand::DeferredCommand)
                .rev(),
        ));
    }

    /// Pop the next command to execute
    fn next_command(&mut self) -> Option<ExecutableCommand> {
        self.0.pop()
    }
}

/// Enum to distinguish between Transaction and Deferred Commands
#[derive(Debug)]
pub(crate) enum ExecutableCommand {
    /// The Command that is submitted from a user's Transaction input
    TransactionCommmand(Command),
    /// The Command that is submitted (deferred) from a Contract Call
    DeferredCommand(DeferredCommand),
}

impl ExecutableCommand {
    /// Returns true if the command originated from Transaction input, not deferred from contract call
    pub fn is_txn_sent(&self) -> bool {
        matches!(self, ExecutableCommand::TransactionCommmand(_))
    }

    /// Returns the CommandKind of the command
    pub fn command_kind(&self) -> CommandKind {
        match self {
            ExecutableCommand::TransactionCommmand(command) => CommandKind::from(command),
            ExecutableCommand::DeferredCommand(deferred_command) => {
                CommandKind::from(&deferred_command.command)
            }
        }
    }

    /// Consumes the Executable Command and returns the result
    pub fn consume_and_execute<S, E, V>(
        self,
        state: &mut ExecutionState<S, E, V>,
        command_index: usize,
    ) -> Result<(), TransitionError>
    where
        S: DB + Send + Sync + Clone,
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        match self {
            ExecutableCommand::TransactionCommmand(command) => {
                command.execute(state, command_index)
            }
            ExecutableCommand::DeferredCommand(deferred_command) => {
                deferred_command.execute(state, command_index)
            }
        }
    }
}

/// Execution entry point for commands in TransactionV1
pub(crate) fn execute_commands_v1<'a, S, V>(
    state: ExecutionState<'a, S, CommandReceiptV1, V>,
    commands: Vec<Command>,
) -> TransitionV1Result<'a, S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    execute_commands::<_, _, _, _, ExecuteCommandsV1>(state, commands)
}

/// Execution entry point for commands in TransactionV2
pub(crate) fn execute_commands_v2<'a, S, V>(
    state: ExecutionState<'a, S, CommandReceiptV2, V>,
    commands: Vec<Command>,
) -> TransitionV2Result<'a, S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    execute_commands::<_, _, _, _, ExecuteCommandsV2>(state, commands)
}
