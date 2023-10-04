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

use pchain_types::blockchain::{Command, CommandReceipt, ExitStatus};
use pchain_world_state::storage::WorldStateStorage;

use crate::{
    commands::{account, staking},
    execution::{
        phases::{self},
        state::ExecutionState,
    },
    transition::StateChangesResult,
    types::DeferredCommand,
    TransitionResult,
};

/// Backbone logic of Commands Execution
pub(crate) fn execute_commands<S>(
    mut state: ExecutionState<S>,
    commands: Vec<Command>,
) -> TransitionResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let pre_charge_result = phases::pre_charge(&mut state);
    // Phase: Pre-Charge

    if let Err(err) = pre_charge_result {
        return TransitionResult {
            new_state: state.ctx.into_ws_cache().ws,
            receipt: None,
            error: Some(err),
            validator_changes: None,
        };
    }
    // Phase: Command(s)
    let mut command_task_results = CommandTaskResults::new();
    let mut command_tasks = CommandTasks::new();
    command_tasks.append(
        commands
            .into_iter()
            .map(CommandTaskItem::TransactionCommmand)
            .collect(),
        None,
    );
    while let Some(command_task) = command_tasks.next_task() {
        let task_id = command_task.task_id;
        let (actor, command) = match command_task.command {
            CommandTaskItem::TransactionCommmand(command) => (state.tx.signer, command),
            CommandTaskItem::DeferredCommand(deferred_command) => {
                (deferred_command.contract_address, deferred_command.command)
            }
        };

        // Execute command triggered from the Transaction
        let ret = account::try_execute(state, &command)
            .or_else(|state| staking::try_execute(actor, state, &command))
            .unwrap();

        // Proceed execution result
        state = match ret {
            // command execution is not completed, continue with resulting state
            Ok(mut state_of_success_execution) => {
                // append command triggered from Call
                if let Some(commands_from_call) = state_of_success_execution.ctx.pop_commands() {
                    command_tasks.append(
                        commands_from_call
                            .into_iter()
                            .map(CommandTaskItem::DeferredCommand)
                            .collect(),
                        Some(task_id),
                    );
                }
                // extract receipt from current execution result
                let cmd_receipt = state_of_success_execution.ctx.extract(ExitStatus::Success);
                command_task_results.push(task_id, cmd_receipt);
                state_of_success_execution
            }
            // in case of error, create the last Command receipt and return result
            Err(StateChangesResult {
                state: mut state_of_abort_result,
                error,
            }) => {
                // extract receipt from last execution result
                let cmd_receipt = state_of_abort_result
                    .ctx
                    .extract(error.as_ref().unwrap().into());
                command_task_results.push(task_id, cmd_receipt);
                return StateChangesResult::new(state_of_abort_result, error)
                    .finalize(command_task_results.command_receipts());
            }
        };
    }

    // Phase: Charge
    phases::charge(state, None).finalize(command_task_results.command_receipts())
}

type TaskID = u32;

/// CommandTasks is a sequence of CommandTask, which follows the properties of CommandTask.
#[derive(Debug)]
pub(crate) struct CommandTasks(Vec<CommandTask>);

impl CommandTasks {
    fn new() -> Self {
        Self(Vec::new())
    }

    /// append a sequence of Commands and store as CommandTask with assigned task ID.
    fn append(&mut self, mut commands: Vec<CommandTaskItem>, same_task_id: Option<u32>) {
        let mut task_id = match same_task_id {
            Some(id) => id,
            None => self.0.last().map_or(0, |t| t.task_id + 1),
        };
        commands.reverse();
        for command in commands {
            self.0.push(CommandTask { task_id, command });
            if same_task_id.is_none() {
                task_id += 1;
            }
        }
    }

    /// Pop the next task to execute
    fn next_task(&mut self) -> Option<CommandTask> {
        self.0.pop()
    }
}

/// CommandTask encapsulates the task to execute a command. An ID number is assigned to a task.
/// There may be multple command tasks sharing the same Task ID. In this case, the commands are
/// considered as one command such that their results should be combined together as one receipt.
#[derive(Debug)]
pub(crate) struct CommandTask {
    task_id: TaskID,
    command: CommandTaskItem,
}

/// CommandTaskItem defines types of command to be executed in a Command Task.
#[derive(Debug)]
pub(crate) enum CommandTaskItem {
    /// The Command that is submitted from Transaction input
    TransactionCommmand(Command),
    /// The Command that is submitted (deferred) from a Contract Call
    DeferredCommand(DeferredCommand),
}

/// CommandTaskResults is a sequence of CommandTaskResult, which follows the properties of CommandTaskResult.
pub(crate) struct CommandTaskResults(Vec<CommandTaskResult>);

impl CommandTaskResults {
    fn new() -> Self {
        Self(Vec::new())
    }

    /// push the next Command Receipt into Results. Combine with the last
    /// receipt if Task ID is as same as the last one.
    fn push(&mut self, task_id: TaskID, command_receipt: CommandReceipt) {
        if let Some(last_result) = self.0.last_mut() {
            if last_result.task_id == task_id {
                last_result.combine(command_receipt);
                return;
            }
        }
        self.0.push(CommandTaskResult {
            task_id,
            command_receipt,
        });
    }

    fn command_receipts(self) -> Vec<CommandReceipt> {
        self.0.into_iter().map(|r| r.command_receipt).collect()
    }
}

/// CommandTaskResult is the result of execution of a CommandTask, which is used to combine
/// the Command Receipt into one if the tasks are sharing same Task ID:
/// - Gas used is added up by the later command receipt
/// - Exit status is overwritten by the later command receipt (i.e. if the last command fails, the exit status should also be failed.)
/// - Return value is overwritten by the later command receipt
pub(crate) struct CommandTaskResult {
    task_id: TaskID,
    command_receipt: CommandReceipt,
}

impl CommandTaskResult {
    /// Combine the information from next Command Receipt
    fn combine(&mut self, next_command_receipt: CommandReceipt) {
        self.command_receipt.gas_used = self
            .command_receipt
            .gas_used
            .saturating_add(next_command_receipt.gas_used);
        self.command_receipt.exit_status = next_command_receipt.exit_status;
        self.command_receipt.return_values = next_command_receipt.return_values;
    }
}

/// TryExecuteResult defines what result information the Command Execution should end up with. In general,
/// it defines two resulting states Ok (command is executed with a result) and Err (command is not accepted to be executed).
pub(crate) enum TryExecuteResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    Ok(Result<ExecutionState<S>, StateChangesResult<S>>),
    Err(ExecutionState<S>),
}

impl<S> TryExecuteResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub fn or_else<O: FnOnce(ExecutionState<S>) -> TryExecuteResult<S>>(
        self,
        op: O,
    ) -> TryExecuteResult<S> {
        match self {
            TryExecuteResult::Ok(t) => TryExecuteResult::Ok(t),
            TryExecuteResult::Err(e) => op(e),
        }
    }

    pub fn unwrap(self) -> Result<ExecutionState<S>, StateChangesResult<S>> {
        match self {
            TryExecuteResult::Ok(ret) => ret,
            TryExecuteResult::Err(_) => panic!(),
        }
    }
}
