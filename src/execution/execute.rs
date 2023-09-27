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
use pchain_types::cryptography::PublicAddress;
use pchain_world_state::storage::WorldStateStorage;

use crate::{
    transition::StateChangesResult, types::DeferredCommand, TransitionError, TransitionResult,
};

use super::{
    account,
    phase::{self},
    protocol, staking,
    state::ExecutionState,
};

/// Backbone logic of Commands Execution
pub(crate) fn execute_commands<S>(
    mut state: ExecutionState<S>,
    commands: Vec<Command>,
) -> TransitionResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let pre_charge_result = phase::pre_charge(&mut state);
    // Phase: Pre-Charge

    if let Err(err) = pre_charge_result {
        return TransitionResult {
            new_state: state.ctx.rw_set.lock().unwrap().clone().ws,
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
                let cmd_receipt = state_of_success_execution.extract(ExitStatus::Success);
                command_task_results.push(task_id, cmd_receipt);
                state_of_success_execution
            }
            // in case of error, create the last Command receipt and return result
            Err(StateChangesResult {
                state: mut state_of_abort_result,
                error,
            }) => {
                // extract receipt from last execution result
                let cmd_receipt = state_of_abort_result.extract(error.as_ref().unwrap().into());
                command_task_results.push(task_id, cmd_receipt);
                return StateChangesResult::new(state_of_abort_result, error)
                    .finalize(command_task_results.command_receipts());
            }
        };
    }

    // Phase: Charge
    phase::charge(state, None).finalize(command_task_results.command_receipts())
}

/// Execute a View Call
pub(crate) fn execute_view<S>(
    state: ExecutionState<S>,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
) -> (CommandReceipt, Option<TransitionError>)
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    match account::call(state, true, target, method, arguments, None) {
        // not yet finish. continue command execution with resulting state
        Ok(mut state_of_success_execution) => {
            let cmd_receipt = state_of_success_execution.extract(ExitStatus::Success);
            (cmd_receipt, None)
        }
        Err(StateChangesResult {
            state: mut state_of_abort_result,
            error,
        }) => {
            let cmd_receipt = state_of_abort_result.extract(error.as_ref().unwrap().into());
            (cmd_receipt, error)
        }
    }
}

/// Execution of NextEpoch Command.
pub(crate) fn execute_next_epoch_command<S>(
    state: ExecutionState<S>,
    commands: Vec<Command>,
) -> TransitionResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let signer = state.tx.signer;

    // Validate the input transaction:
    // - There can only be one NextEpoch Command in a transaction.
    // - Block performance is required for execution of next epoch transaction.
    // - Transaction nonce matches with the nonce in state

    let rw_set = state.ctx.rw_set.lock().unwrap();
    if commands.len() != 1
        || commands.first() != Some(&Command::NextEpoch)
        || state.bd.validator_performance.is_none()
        || state.tx.nonce != rw_set.ws.nonce(signer)
    {
        return TransitionResult {
            new_state: rw_set.ws.clone(),
            receipt: None,
            error: Some(TransitionError::InvalidNextEpochCommand),
            validator_changes: None,
        };
    }
    drop(rw_set);

    // State transition
    let (mut state, new_vs) = protocol::next_epoch(state);

    // Update Nonce for the transaction. This step ensures future epoch transaction produced
    // by the signer will have different transaction hash.
    let mut rw_set = state.ctx.rw_set.lock().unwrap();
    let nonce = rw_set.ws.nonce(signer).saturating_add(1);
    rw_set.ws.with_commit().set_nonce(signer, nonce);
    drop(rw_set);

    // Extract receipt from current execution result
    let cmd_receipt = state.extract(ExitStatus::Success);

    let mut result = StateChangesResult::new(state, None).finalize(vec![cmd_receipt]);
    result.validator_changes = Some(new_vs);
    result
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

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use pchain_types::blockchain::{Command, ExitStatus, Transaction};
    use pchain_types::cryptography::PublicAddress;
    use pchain_types::runtime::*;
    use pchain_types::serialization::Serializable;
    use pchain_world_state::network::constants;
    use pchain_world_state::network::pool::Pool;
    use pchain_world_state::{
        network::{
            network_account::NetworkAccountSized,
            pool::PoolKey,
            stake::{Stake, StakeValue},
        },
        states::WorldState,
        storage::{Key, Value, WorldStateStorage},
    };

    use crate::{
        execution::{
            execute::{execute_commands, execute_next_epoch_command},
            state::ExecutionState,
        },
        transition::TransitionContext,
        types::BaseTx,
        BlockProposalStats, BlockchainParams, TransitionError, ValidatorPerformance,
    };
    use crate::{gas, TransitionResult};

    const TEST_MAX_VALIDATOR_SET_SIZE: u16 = constants::MAX_VALIDATOR_SET_SIZE;
    const TEST_MAX_STAKES_PER_POOL: u16 = constants::MAX_STAKES_PER_POOL;
    const MIN_BASE_FEE: u64 = 8;
    type NetworkAccount<'a, S> =
        NetworkAccountSized<'a, S, { TEST_MAX_VALIDATOR_SET_SIZE }, { TEST_MAX_STAKES_PER_POOL }>;

    #[derive(Clone)]
    struct SimpleStore {
        inner: HashMap<Key, Value>,
    }
    impl WorldStateStorage for SimpleStore {
        fn get(&self, key: &Key) -> Option<Value> {
            match self.inner.get(key) {
                Some(v) => Some(v.clone()),
                None => None,
            }
        }
    }

    const ACCOUNT_A: [u8; 32] = [1u8; 32];
    const ACCOUNT_B: [u8; 32] = [2u8; 32];
    const ACCOUNT_C: [u8; 32] = [3u8; 32];
    const ACCOUNT_D: [u8; 32] = [4u8; 32];

    /// Null test on empty transaction commands
    #[test]
    fn test_empty_commands() {
        let mut state = create_state(None);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_A);
        drop(rw_set);

        let tx_base_cost = set_tx(&mut state, ACCOUNT_A, 0, &vec![]);
        let ret = execute_commands(state, vec![]);
        assert_eq!((&ret.error, &ret.receipt), (&None, &Some(vec![])));
        let gas_used = extract_gas_used(&ret);
        assert_eq!(gas_used, 0);

        let state = create_state(Some(ret.new_state));

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_A);
        drop(rw_set);

        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost
        );
    }

    #[test]
    // Commands Transfer
    fn test_transfer() {
        let state = create_state(None);

        let amount = 999_999;
        let ret = execute_commands(
            state,
            vec![Command::Transfer(TransferInput {
                recipient: ACCOUNT_B,
                amount,
            })],
        );

        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );

        assert_eq!(extract_gas_used(&ret), 32820);
        let owner_balance_after = ret.new_state.balance(ACCOUNT_B);

        assert_eq!(owner_balance_after, 500_000_000 + amount);
    }
    // Commands: Create Pool
    // Exception:
    // - Create Pool again
    // - Pool commission rate > 100
    #[test]
    fn test_create_pool() {
        let state = create_state(None);
        let ret = execute_commands(
            state,
            vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 334610);

        let mut state = create_state(Some(ret.new_state));
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .operator()
                .unwrap(),
            ACCOUNT_A
        );
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .commission_rate()
                .unwrap(),
            1
        );

        ///// Exceptions: /////

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        state.tx.nonce = 1;
        let ret = execute_commands(
            state,
            vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
        );
        assert_eq!(ret.error, Some(TransitionError::PoolAlreadyExists));
        assert_eq!(extract_gas_used(&ret), 1980);

        let mut state = create_state(Some(ret.new_state));
        state.tx.nonce = 2;
        let ret = execute_commands(
            state,
            vec![Command::CreatePool(CreatePoolInput {
                commission_rate: 101,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));
        assert_eq!(extract_gas_used(&ret), 0);
    }

    // Commands: Create Pool, Set Pool Settings
    // Exception:
    // - Pool Not exist
    // - Pool commission rate > 100
    // - Same commission rate
    #[test]
    fn test_create_pool_set_policy() {
        let state = create_state(None);
        let ret = execute_commands(
            state,
            vec![
                Command::CreatePool(CreatePoolInput { commission_rate: 1 }),
                Command::SetPoolSettings(SetPoolSettingsInput { commission_rate: 2 }),
            ],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );

        assert_eq!(extract_gas_used(&ret), 354770);

        let mut state = create_state(Some(ret.new_state));
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .commission_rate()
                .unwrap(),
            2
        );

        ///// Exceptions: /////

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        state.tx.signer = ACCOUNT_B;
        let ret = execute_commands(
            state,
            vec![Command::SetPoolSettings(SetPoolSettingsInput {
                commission_rate: 3,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::PoolNotExists));

        assert_eq!(extract_gas_used(&ret), 1980);

        let mut state = create_state(Some(ret.new_state));
        state.tx.signer = ACCOUNT_A;
        state.tx.nonce = 1;
        let ret = execute_commands(
            state,
            vec![Command::SetPoolSettings(SetPoolSettingsInput {
                commission_rate: 101,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));

        assert_eq!(extract_gas_used(&ret), 0);

        let mut state = create_state(Some(ret.new_state));
        state.tx.nonce = 2;
        let ret = execute_commands(
            state,
            vec![Command::SetPoolSettings(SetPoolSettingsInput {
                commission_rate: 2,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));

        assert_eq!(extract_gas_used(&ret), 4010);
    }

    // Commands: Create Pool, Delete Pool
    // Exception:
    // - Pool Not exist
    #[test]
    fn test_create_delete_pool() {
        let state = create_state(None);
        let ret = execute_commands(
            state,
            vec![
                Command::CreatePool(CreatePoolInput { commission_rate: 1 }),
                Command::DeletePool,
            ],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 334610);
        let mut state = create_state(Some(ret.new_state));
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .operator()
            .is_none());
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .commission_rate()
            .is_none());
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .operator_stake()
            .is_none());
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .power()
            .is_none());
        assert!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .delegated_stakes()
                .length()
                == 0
        );

        ///// Exceptions: /////

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        state.tx.signer = ACCOUNT_B;
        let ret = execute_commands(state, vec![Command::DeletePool]);
        assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
        assert_eq!(extract_gas_used(&ret), 1980);
    }

    // Command 1 (account a): Create Pool
    // Command 2 (account b): Create Deposit
    // Exception:
    // - Pool Not exist
    // - Deposit already exists
    // - Not enough balance
    #[test]
    fn test_create_pool_create_deposit() {
        let state = create_state(None);
        let ret = execute_commands(
            state,
            vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );

        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_A,
            balance: 500_000,
            auto_stake_rewards: false,
        })];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 82810);

        let mut state = create_state(Some(ret.new_state));
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            500_000
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .auto_stake_rewards()
                .unwrap(),
            false
        );

        ///// Exceptions: /////

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        state.tx.nonce = 1;
        let ret = execute_commands(
            state,
            vec![Command::CreateDeposit(CreateDepositInput {
                operator: ACCOUNT_B,
                balance: 500_000,
                auto_stake_rewards: false,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
        assert_eq!(extract_gas_used(&ret), 1980);

        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_A,
            balance: 500_000,
            auto_stake_rewards: false,
        })];
        set_tx(&mut state, ACCOUNT_B, 1, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::DepositsAlreadyExists));
        assert_eq!(extract_gas_used(&ret), 4600);

        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_A,
            balance: 500_000_000,
            auto_stake_rewards: false,
        })];
        set_tx(&mut state, ACCOUNT_C, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            ret.error,
            Some(TransitionError::NotEnoughBalanceForTransfer)
        );
        assert_eq!(extract_gas_used(&ret), 5660);
    }

    // Prepare: pool (account a) in world state
    // Commands (account b): Create Deposit, Set Deposit Settings
    // Exception:
    // - Deposit not exist
    // - same deposit policy
    #[test]
    fn test_create_deposit_set_policy() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![
            Command::CreateDeposit(CreateDepositInput {
                operator: ACCOUNT_A,
                balance: 500_000,
                auto_stake_rewards: false,
            }),
            Command::SetDepositSettings(SetDepositSettingsInput {
                operator: ACCOUNT_A,
                auto_stake_rewards: true,
            }),
        ];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 109050);

        let mut state = create_state(Some(ret.new_state));
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            500_000
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .auto_stake_rewards()
                .unwrap(),
            true
        );

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        drop(rw_set);

        let ret = execute_commands(
            state,
            vec![Command::SetDepositSettings(SetDepositSettingsInput {
                operator: ACCOUNT_B,
                auto_stake_rewards: true,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
        assert_eq!(extract_gas_used(&ret), 2620);

        let mut state = create_state(Some(ret.new_state));
        let commands = vec![
            Command::SetDepositSettings(SetDepositSettingsInput {
                operator: ACCOUNT_A,
                auto_stake_rewards: true,
            }), // Same deposit plocy
        ];
        set_tx(&mut state, ACCOUNT_B, 1, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::InvalidDepositPolicy));
        assert_eq!(extract_gas_used(&ret), 5290);
    }

    // Prepare: pool (account a) in world state
    // Commands (account b): Create Deposit, Topup Deposit
    // Exception:
    // - Deposit not exist
    // - Not enough balance
    #[test]
    fn test_create_deposit_topupdeposit() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![
            Command::CreateDeposit(CreateDepositInput {
                operator: ACCOUNT_A,
                balance: 500_000,
                auto_stake_rewards: false,
            }),
            Command::TopUpDeposit(TopUpDepositInput {
                operator: ACCOUNT_A,
                amount: 100,
            }),
        ];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 134910);

        let mut state = create_state(Some(ret.new_state));
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            500_100
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .auto_stake_rewards()
                .unwrap(),
            false
        );

        ///// Exceptions: /////
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let state = create_state(Some(rw_set.ws.to_owned()));
        drop(rw_set);
        let ret = execute_commands(
            state,
            vec![Command::TopUpDeposit(TopUpDepositInput {
                operator: ACCOUNT_A,
                amount: 100,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
        assert_eq!(extract_gas_used(&ret), 2620);

        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_A,
            balance: 500_000_000,
            auto_stake_rewards: false,
        })];
        set_tx(&mut state, ACCOUNT_C, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            ret.error,
            Some(TransitionError::NotEnoughBalanceForTransfer)
        );
        assert_eq!(extract_gas_used(&ret), 5660);
    }

    // Prepare: pool (account a) in world state
    // Prepare: deposits (account b) to pool (account a)
    // Commands (account b): Stake Deposit
    // Exception:
    // - Deposit not exist
    // - Reach limit (Deposit amount)
    // - Pool not exist
    #[test]
    fn test_stake_deposit_delegated_stakes() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
        deposit.set_balance(20_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![
            Command::StakeDeposit(StakeDepositInput {
                operator: ACCOUNT_A,
                max_amount: 20_000 + 1,
            }), // stake more than deposit
        ];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            20_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 382740);

        let mut state = create_state(Some(ret.new_state));
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 120_000);
        let delegated_stake = pool.delegated_stakes();
        let delegated_stake = delegated_stake.get_by(&ACCOUNT_B).unwrap();
        assert_eq!(delegated_stake.power, 20_000);

        ///// Exceptions: /////
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        drop(rw_set);

        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 20_000,
        })];
        set_tx(&mut state, ACCOUNT_C, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
        assert_eq!(extract_gas_used(&ret), 2620);

        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 1,
        })];
        set_tx(&mut state, ACCOUNT_B, 1, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
        assert_eq!(extract_gas_used(&ret), 16920);

        // Delete Pool first
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::DeletePool];
        set_tx(&mut state, ACCOUNT_A, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, None);
        assert_eq!(extract_gas_used(&ret), 0);

        // and then stake deposit
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 20_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 2, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
        assert_eq!(extract_gas_used(&ret), 7620);
    }

    // // Prepare: set maximum number of pools in world state, pool (account a) has the minimum power.
    // // Prepare: deposits (account b) to pool (account a)
    // // Commands (account b): Stake Deposit (to increase the power of pool (account a))
    #[test]
    fn test_stake_deposit_delegated_stakes_nvp_change_key() {
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 100_000);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
        deposit.set_balance(6_300_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 6_300_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            6_300_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 1308410);

        let mut state = create_state(Some(ret.new_state));
        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 6_400_000);
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            [
                2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                1, 1, 1, 1
            ]
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            200_000
        );
    }

    // // Prepare: set maximum number of pools in world state, pool (account b) is not inside nvp.
    // // Prepare: deposits (account c) to pool (account b)
    // // Commands (account c): Stake Deposit (to increase the power of pool (account b) to be included in nvp)
    #[test]
    fn test_stake_deposit_delegated_stakes_nvp_insert() {
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_B);
        pool.set_operator(ACCOUNT_B);
        pool.set_commission_rate(1);
        pool.set_power(0);
        pool.set_operator_stake(None);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_B, ACCOUNT_C);
        deposit.set_balance(6_500_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_B,
            max_amount: 6_500_000,
        })];
        set_tx(&mut state, ACCOUNT_C, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            6_500_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 1247750);
        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            [
                2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                1, 1, 1, 1
            ]
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            200_000
        );
        let pool_in_nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter)
            .get_by(&ACCOUNT_B)
            .unwrap();
        assert_eq!(
            (pool_in_nvp.operator, pool_in_nvp.power),
            (ACCOUNT_B, 6_500_000)
        );
    }

    // // Prepare: pool (account a), with maximum number of stakes in world state
    // // Prepare: deposits (account c) to pool (account a)
    // // Commands (account c): Stake Deposit (to be included in delegated stakes)
    // // Exception
    // // - stake is too small to insert
    #[test]
    fn test_stake_deposit_delegated_stakes_insert() {
        let mut state = create_state(None);
        create_full_stakes_in_pool(&mut state, ACCOUNT_A);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_C);
        deposit.set_balance(250_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let prev_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .power()
            .unwrap();
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 250_000,
        })];
        set_tx(&mut state, ACCOUNT_C, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            250_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 2811240);

        let mut state = create_state(Some(ret.new_state));
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        let cur_pool_power = pool.power().unwrap();
        assert_eq!(cur_pool_power, prev_pool_power + 50_000);
        let delegated_stakes = pool.delegated_stakes();
        assert_eq!(delegated_stakes.get(0).unwrap().power, 250_000);
        assert_eq!(delegated_stakes.get(0).unwrap().owner, ACCOUNT_C);

        ///// Exceptions: /////
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        drop(rw_set);
        // create deposit first (too low to join deledated stake )
        let commands = vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_A,
            balance: 100_000,
            auto_stake_rewards: false,
        })];
        set_tx(&mut state, ACCOUNT_D, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 82810);
        // and then stake deposit
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 100_000,
        })];
        set_tx(&mut state, ACCOUNT_D, 1, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
        assert_eq!(extract_gas_used(&ret), 18920);
    }

    // Prepare: pool (account c), with maximum number of stakes in world state, stakes (account b) is the minimum value.
    // Prepare: deposits (account b) to pool (account c)
    // Commands (account b): Stake Deposit (to be included in delegated stakes, but not the minimum one)
    #[test]
    fn test_stake_deposit_delegated_stakes_change_key() {
        let mut state = create_state(None);
        create_full_stakes_in_pool(&mut state, ACCOUNT_C);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_C, ACCOUNT_B);
        deposit.set_balance(310_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let prev_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C)
            .power()
            .unwrap();
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_C,
            max_amount: 110_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            110_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 542720);
        let mut state = create_state(Some(ret.new_state));

        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C);
        let cur_pool_power = pool.power().unwrap();
        assert_eq!(cur_pool_power, prev_pool_power + 110_000);
        let min_stake = pool.delegated_stakes().get(0).unwrap();
        assert_eq!(min_stake.power, 300_000);
        assert_eq!(
            min_stake.owner,
            [
                3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
                2, 2, 2, 2
            ]
        );
    }

    // Prepare: pool (account a) in world state, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account a)
    // Commands (account b): Stake Deposit (to increase the stake in the delegated stakes)
    #[test]
    fn test_stake_deposit_delegated_stakes_existing() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        pool.delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 50_000,
            }))
            .unwrap();
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
        deposit.set_balance(100_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 40_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            40_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 314340);
        let mut state = create_state(Some(ret.new_state));

        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 140_000);
        let delegated_stake = pool.delegated_stakes();
        let delegated_stake = delegated_stake.get_by(&ACCOUNT_B).unwrap();
        assert_eq!(delegated_stake.power, 90_000);
    }

    // Prepare: pool (account a) in world state
    // Prepare: deposits (account a) to pool (account a)
    // Commands (account a): Stake Deposit
    #[test]
    fn test_stake_deposit_same_owner() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
        deposit.set_balance(150_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let state = create_state(Some(ws));
        let ret = execute_commands(
            state,
            vec![Command::StakeDeposit(StakeDepositInput {
                operator: ACCOUNT_A,
                max_amount: 20_000,
            })],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            20_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 323880);
        let mut state = create_state(Some(ret.new_state));

        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        let operator_state = pool.operator_stake().unwrap().unwrap();
        assert_eq!(operator_state.power, 20_000);
        assert_eq!(pool.power().unwrap(), 120_000);
        let delegated_stake = pool.delegated_stakes();
        assert_eq!(delegated_stake.length(), 0);
    }

    // Prepare: set maximum number of pools in world state, pool (account a) has the minimum power.
    // Prepare: deposits (account a) to pool (account a)
    // Commands (account a): Stake Deposit (to increase the power of pool (account a))
    #[test]
    fn test_stake_deposit_same_owner_nvp_change_key() {
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 100_000);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
        deposit.set_balance(210_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 110_000,
        })];
        set_tx(&mut state, ACCOUNT_A, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            110_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 420710);
        let mut state = create_state(Some(ret.new_state));

        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 210_000);
        assert_eq!(pool.operator_stake().unwrap().unwrap().power, 210_000);
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            [
                2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                1, 1, 1, 1
            ]
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            200_000
        );
    }

    // Prepare: set maximum number of pools in world state, pool (account c) is not inside nvp.
    // Prepare: deposits (account c) to pool (account c)
    // Commands (account c): Stake Deposit (to increase the power of pool (account c) to be included in nvp)
    #[test]
    fn test_stake_deposit_same_owner_nvp_insert() {
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C)
            .operator()
            .is_none());
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C);
        pool.set_operator(ACCOUNT_C);
        pool.set_commission_rate(1);
        pool.set_power(0);
        pool.set_operator_stake(None);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_C, ACCOUNT_C);
        deposit.set_balance(150_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_C,
            max_amount: 150_000,
        })];
        set_tx(&mut state, ACCOUNT_C, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            150_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 2279890);
        let mut state = create_state(Some(ret.new_state));

        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C);
        assert_eq!(pool.power().unwrap(), 150_000);
        assert_eq!(pool.operator_stake().unwrap().unwrap().power, 150_000);
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_C
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            150_000
        );
    }

    // Prepare: pool (account a) in world state, with non-zero value of Operator Stake
    // Prepare: deposits (account a) to pool (account a)
    // Commands (account a): Stake Deposit
    #[test]
    fn test_stake_deposit_same_owner_existing() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(Some(Stake {
            owner: ACCOUNT_A,
            power: 80_000,
        }));
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
        deposit.set_balance(100_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let state = create_state(Some(ws));
        let ret = execute_commands(
            state,
            vec![Command::StakeDeposit(StakeDepositInput {
                operator: ACCOUNT_A,
                max_amount: 10_000,
            })],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            10_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 277880);
        let mut state = create_state(Some(ret.new_state));

        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        let operator_state = pool.operator_stake().unwrap().unwrap();
        assert_eq!(operator_state.power, 90_000);
        assert_eq!(pool.power().unwrap(), 110_000);
        let delegated_stake = pool.delegated_stakes();
        assert_eq!(delegated_stake.length(), 0);
    }

    // Prepare: pool (account a) in world state, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account a)
    // Commands (account b): Unstake Deposit
    // Exception:
    // - Stakes not exists
    // - Pool has no delegated stake
    // - Pool not exists
    #[test]
    fn test_unstake_deposit_delegated_stakes() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        pool.delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 50_000,
            }))
            .unwrap();
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
        deposit.set_balance(100_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 40_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            40_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 311320);
        let mut state = create_state(Some(ret.new_state));

        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 60_000);
        let delegated_stake = pool.delegated_stakes();
        let delegated_stake = delegated_stake.get_by(&ACCOUNT_B).unwrap();
        assert_eq!(delegated_stake.power, 10_000);

        ///// Exceptions: /////
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_C,
            max_amount: 40_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 1, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
        assert_eq!(extract_gas_used(&ret), 2620);
        // create Pool and deposit first
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })];
        set_tx(&mut state, ACCOUNT_C, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 516870);
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_C,
            balance: 10_000,
            auto_stake_rewards: false,
        })];
        set_tx(&mut state, ACCOUNT_B, 2, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 82810);
        // and then UnstakeDeposit
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_C,
            max_amount: 10_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 3, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::PoolHasNoStakes));
        assert_eq!(extract_gas_used(&ret), 9620);
        // delete pool first
        let state = create_state(Some(ret.new_state));
        let ret = execute_commands(state, vec![Command::DeletePool]);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 0);
        // then UnstakeDeposit
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 10_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 4, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
        assert_eq!(extract_gas_used(&ret), 4600);
    }

    // Prepare: pool (account a) in world state, with delegated stakes of account X, X has the biggest stake
    // Prepare: deposits (account X) to pool (account a)
    // Commands (account X): Unstake Deposit
    #[test]
    fn test_unstake_deposit_delegated_stakes_remove() {
        let mut state = create_state(None);
        create_full_deposits_in_pool(&mut state, ACCOUNT_A, false);
        create_full_stakes_in_pool(&mut state, ACCOUNT_A);
        let biggest = [
            129u8, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2, 2,
        ];
        state.ctx.gas_meter.ws_set_balance(biggest, 500_000_000);
        let origin_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .power()
            .unwrap();
        let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .get_by(&biggest)
            .unwrap();

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: stake.power,
        })];
        set_tx(&mut state, biggest, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            stake.power.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 0);
        let mut state = create_state(Some(ret.new_state));

        let new_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .power()
            .unwrap();
        assert_eq!(origin_pool_power - new_pool_power, stake.power);
        let stakers = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .unordered_values();
        assert!(!stakers.iter().any(|v| v.owner == biggest));
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .get_by(&biggest)
            .is_none());
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account t)
    // Commands (account b): Unstake Deposit (to decrease the power of pool (account t))
    #[test]
    fn test_unstake_deposit_delegated_stakes_nvp_change_key() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 150_000,
            }))
            .unwrap();
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_B);
        deposit.set_balance(200_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![
            Command::UnstakeDeposit(UnstakeDepositInput {
                operator: ACCOUNT_T,
                max_amount: 150_000 + 1,
            }), // unstake more than staked
        ];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            150_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 42590);
        let mut state = create_state(Some(ret.new_state));

        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 50_000);
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_T
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            50_000
        );
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account t)
    // Commands (account b): Unstake Deposit (to empty the power of pool (account t), and to be kicked out from nvp)
    #[test]
    fn test_unstake_deposit_delegated_stakes_nvp_remove() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 200_000,
            }))
            .unwrap();
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_B);
        deposit.set_balance(200_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);
        let mut state = create_state(Some(ws));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_T,
            max_amount: 200_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            200_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 423900);
        let mut state = create_state(Some(ret.new_state));

        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 0);
        assert!(pool.delegated_stakes().get_by(&ACCOUNT_B).is_none());
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32 - 1
        );
        assert_ne!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_T
        );
    }

    // Prepare: pool (account a) in world state, with non-zero value of Operator Stake
    // Prepare: deposits (account a) to pool (account a)
    // Commands (account a): Unstake Deposit
    // Exception:
    // - Pool has no operator stake
    #[test]
    fn test_unstake_deposit_same_owner() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(Some(Stake {
            owner: ACCOUNT_A,
            power: 100_000,
        }));
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
        deposit.set_balance(150_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let state = create_state(Some(ws));
        let ret = execute_commands(
            state,
            vec![Command::UnstakeDeposit(UnstakeDepositInput {
                operator: ACCOUNT_A,
                max_amount: 100_000,
            })],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            100_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 6630);
        let mut state = create_state(Some(ret.new_state));

        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(pool.power().unwrap(), 0);
        assert!(pool.operator_stake().unwrap().is_none());

        ///// Exceptions: /////

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        state.tx.nonce = 1;
        let ret = execute_commands(
            state,
            vec![Command::UnstakeDeposit(UnstakeDepositInput {
                operator: ACCOUNT_A,
                max_amount: 50_000,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::PoolHasNoStakes));
        assert_eq!(extract_gas_used(&ret), 9010);
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with non-zero value of Operator Stake
    // Prepare: deposits (account t) to pool (account t)
    // Commands (account t): Unstake Deposit (to decrease the power of pool (account t))
    #[test]
    fn test_unstake_deposit_same_owner_nvp_change_key() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.set_operator_stake(Some(Stake {
            owner: ACCOUNT_T,
            power: 200_000,
        }));
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_T);
        deposit.set_balance(200_000);
        deposit.set_auto_stake_rewards(false);

        let mut rw_set = state.ctx.rw_set.lock().unwrap();
        rw_set.ws.cached().set_balance(ACCOUNT_T, 500_000_000);
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_T,
            max_amount: 190_000,
        })];
        set_tx(&mut state, ACCOUNT_T, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            190_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 388730);
        let mut state = create_state(Some(ret.new_state));

        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 10_000);
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_T
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            10_000
        );
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with non-zero value of Operator Stake
    // Prepare: deposits (account t) to pool (account t)
    // Commands (account t): Unstake Deposit (to empty the power of pool (account t), and to be kicked out from nvp)
    #[test]
    fn test_unstake_deposit_same_owner_nvp_remove() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.set_operator_stake(Some(Stake {
            owner: ACCOUNT_T,
            power: 200_000,
        }));
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_T);
        deposit.set_balance(200_000);
        deposit.set_auto_stake_rewards(false);

        let mut rw_set = state.ctx.lock().unwrap();
        rw_set.ws.cached().set_balance(ACCOUNT_T, 500_000_000);
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));
        let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_T,
            max_amount: 200_000,
        })];
        set_tx(&mut state, ACCOUNT_T, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            200_000_u64.to_le_bytes().to_vec()
        );
        assert_eq!(extract_gas_used(&ret), 670040);

        let mut state = create_state(Some(ret.new_state));

        let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 0);
        assert!(pool.operator_stake().unwrap().is_none());
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32 - 1
        );
        assert_ne!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_T
        );
    }

    // Prepare: pool (account a) in world state, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account a)
    // Commands (account b): Withdraw Deposit (to reduce the delegated stakes in pool (account a))
    // Exception:
    // - Deposit not exist
    // - deposit amount = locked stake amount (vp)
    // - deposit amount = locked stake amount (pvp)
    #[test]
    fn test_withdrawal_deposit_delegated_stakes() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
        deposit.set_balance(100_000);
        deposit.set_auto_stake_rewards(false);
        NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 100_000,
            }))
            .unwrap();

        let rw_set = state.ctx.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();

        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 40_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            40_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = extract_gas_used(&ret);
        assert_eq!(gas_used, 362780);

        let mut state = create_state(Some(ret.new_state));
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            60_000
        );
        let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .get_by(&ACCOUNT_B)
            .unwrap();
        assert_eq!((stake.owner, stake.power), (ACCOUNT_B, 60_000));
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .power()
                .unwrap(),
            60_000
        );
        let rw_set = state.ctx.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);

        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 40_000
        );

        ///// Exceptions: /////
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        let ret = execute_commands(
            state,
            vec![Command::WithdrawDeposit(WithdrawDepositInput {
                operator: ACCOUNT_A,
                max_amount: 40_000,
            })],
        );
        assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
        assert_eq!(extract_gas_used(&ret), 2620);

        // First proceed next epoch
        let mut state = create_state(Some(ret.new_state));
        state.tx.nonce = 1;
        let ret = execute_next_epoch_command(state, vec![Command::NextEpoch]);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 0);
        // Then unstake
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![
            Command::UnstakeDeposit(UnstakeDepositInput {
                operator: ACCOUNT_A,
                max_amount: 10_000,
            }), // 60_000 - 10_000
        ];
        set_tx(&mut state, ACCOUNT_B, 1, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 242150);
        // pvp: 0, vp: 60_000, nvp: 50_000, deposit: 60_000, Try withdraw
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 10_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 2, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
        assert_eq!(extract_gas_used(&ret), 19780);

        // Proceed next epoch
        let mut state = create_state(Some(ret.new_state));
        state.tx.nonce = 2;
        state.bd.validator_performance = Some(single_node_performance(
            ACCOUNT_A,
            TEST_MAX_VALIDATOR_SET_SIZE as u32,
        ));
        let ret = execute_next_epoch_command(state, vec![Command::NextEpoch]);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 0);
        // pvp: 60_000, vp: 50_000, nvp: 50_000, deposit: 60_013, Deduce deposit to 60_000
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![
            Command::WithdrawDeposit(WithdrawDepositInput {
                operator: ACCOUNT_A,
                max_amount: 13,
            }), // reduce deposit to 60_000
        ];
        set_tx(&mut state, ACCOUNT_B, 3, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 83580);
        // pvp: 60_000, vp: 50_000, nvp: 50_000, deposit: 60_000, Try Withdraw
        let mut state = create_state(Some(ret.new_state));
        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 10_000,
        })];
        set_tx(&mut state, ACCOUNT_B, 4, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
        assert_eq!(extract_gas_used(&ret), 29960);
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account t)
    // Commands (account b): Withdraw Deposit (to decrease the power of pool (account t))
    #[test]
    fn test_withdrawal_deposit_delegated_stakes_nvp_change_key() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.set_operator_stake(None);
        NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
            .delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 150_000,
            }))
            .unwrap();
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_B);
        deposit.set_balance(200_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();

        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_T,
            max_amount: 200_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            200_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = ret
            .receipt
            .as_ref()
            .unwrap()
            .iter()
            .map(|g| g.gas_used)
            .sum::<u64>();
        assert_eq!(extract_gas_used(&ret), 0);

        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_B).balance(),
            None
        );
        let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
            .delegated_stakes()
            .get_by(&ACCOUNT_B);
        assert!(stake.is_none());
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
                .power()
                .unwrap(),
            50_000
        );
        let rw_set = state.ctx.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);

        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 200_000
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_T
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            50_000
        );
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account t)
    // Commands (account b): Withdraw Deposit (to empty the power of pool (account t), and to be kicked out from nvp)
    #[test]
    fn test_withdrawal_deposit_delegated_stakes_nvp_remove() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.set_operator_stake(None);
        NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
            .delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 200_000,
            }))
            .unwrap();
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_B);
        deposit.set_balance(300_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();

        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_A);
        drop(rw_set);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_T,
            max_amount: 300_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            300_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = ret
            .receipt
            .as_ref()
            .unwrap()
            .iter()
            .map(|g| g.gas_used)
            .sum::<u64>();
        assert_eq!(extract_gas_used(&ret), 146310);

        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_B).balance(),
            None
        );
        let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
            .delegated_stakes()
            .get_by(&ACCOUNT_B);
        assert!(stake.is_none());
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
                .power()
                .unwrap(),
            0
        );
        let rw_set = state.ctx.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);

        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 300_000
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32 - 1
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_A
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            100_000
        );
    }

    // Prepare: pool (account a) in world state, with non-zero value of Operator Stake
    // Prepare: deposits (account a) to pool (account a)
    // Commands (account a): Withdraw Deposit (to reduce the operator stake of pool (account a))
    #[test]
    fn test_withdrawal_deposit_same_owner() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(Some(Stake {
            owner: ACCOUNT_A,
            power: 100_000,
        }));
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
        deposit.set_balance(100_000);
        deposit.set_auto_stake_rewards(false);

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();

        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_A);
        drop(rw_set);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 45_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_A, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            45_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = extract_gas_used(&ret);
        assert_eq!(gas_used, 326320);

        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A)
                .balance()
                .unwrap(),
            55_000
        );
        let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .operator_stake()
            .unwrap()
            .unwrap();
        assert_eq!((stake.owner, stake.power), (ACCOUNT_A, 55_000));
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .power()
                .unwrap(),
            55_000
        );
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_A);
        drop(rw_set);
        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 45_000
        );
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with non-zero value of Operator Stake
    // Prepare: deposits (account t) to pool (account t)
    // Commands (account t): Withdraw Deposit (to decrease the power of pool (account t))

    #[test]
    fn test_withdrawal_deposit_same_owner_nvp_change_key() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.set_operator_stake(Some(Stake {
            owner: ACCOUNT_T,
            power: 150_000,
        }));
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_T);
        deposit.set_balance(200_000);
        deposit.set_auto_stake_rewards(false);

        let mut rw_set = state.ctx.lock().unwrap();
        rw_set.ws.cached().set_balance(ACCOUNT_T, 500_000_000);
        let ws = rw_set.clone().commit_to_world_state();

        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_T);
        drop(rw_set);
        println!("owner_balance_before: {}", owner_balance_before);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_T,
            max_amount: 200_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_T, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            200_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = extract_gas_used(&ret);
        assert_eq!(gas_used, 11140);

        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_T).balance(),
            None
        );
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
            .operator_stake()
            .unwrap()
            .is_none());
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
                .power()
                .unwrap(),
            50_000
        );
        let rw_set = state.ctx.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_T);
        drop(rw_set);

        // TODO confirm if this is passing
        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 200_000
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_T
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            50_000
        );
    }

    // Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with non-zero value of Operator Stake
    // Prepare: deposits (account t) to pool (account t)
    // Commands (account t): Withdraw Deposit (to empty the power of pool (account t), and to be kicked out from nvp)
    #[test]
    fn test_withdrawal_deposit_same_owner_nvp_remove() {
        const ACCOUNT_T: [u8; 32] = [
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1,
        ];
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
        assert_eq!(pool.power().unwrap(), 200_000);
        pool.set_operator_stake(Some(Stake {
            owner: ACCOUNT_T,
            power: 200_000,
        }));
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_T);
        deposit.set_balance(300_000);
        deposit.set_auto_stake_rewards(false);

        let mut rw_set = state.ctx.lock().unwrap();
        rw_set.ws.cached().set_balance(ACCOUNT_T, 500_000_000);
        let ws = rw_set.clone().commit_to_world_state();

        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_A);
        drop(rw_set);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_T,
            max_amount: 300_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_T, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            300_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = extract_gas_used(&ret);
        assert_eq!(gas_used, 392450);

        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_T).balance(),
            None
        );
        assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
            .operator_stake()
            .unwrap()
            .is_none());
        let rw_set = state.ctx.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_T);
        drop(rw_set);

        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 300_000
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32 - 1
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .operator,
            ACCOUNT_A
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter)
                .get(0)
                .unwrap()
                .power,
            100_000
        );
    }

    // Prepare: pool (account a) in world state, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account a)
    // Prepare: 0 < pvp.power < vp.power
    // Commands (account b): Withdraw Deposit (to reduce the delegated stakes in pool (account a))
    #[test]
    fn test_withdrawal_deposit_bounded_by_vp() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
        deposit.set_balance(100_000);
        deposit.set_auto_stake_rewards(false);
        NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 100_000,
            }))
            .unwrap();
        NetworkAccount::pvp(&mut state.ctx.gas_meter)
            .push(
                Pool {
                    operator: ACCOUNT_A,
                    commission_rate: 1,
                    power: 100_000,
                    operator_stake: None,
                },
                vec![StakeValue::new(Stake {
                    owner: ACCOUNT_B,
                    power: 70_000,
                })],
            )
            .unwrap();
        NetworkAccount::vp(&mut state.ctx.gas_meter)
            .push(
                Pool {
                    operator: ACCOUNT_A,
                    commission_rate: 1,
                    power: 100_000,
                    operator_stake: None,
                },
                vec![StakeValue::new(Stake {
                    owner: ACCOUNT_B,
                    power: 80_000,
                })],
            )
            .unwrap();

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 40_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            20_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = extract_gas_used(&ret);
        assert_eq!(gas_used, 383140);

        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            80_000
        );
        let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .get_by(&ACCOUNT_B)
            .unwrap();
        assert_eq!((stake.owner, stake.power), (ACCOUNT_B, 80_000));
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .power()
                .unwrap(),
            80_000
        );

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);
        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 20_000
        );
    }

    // Prepare: pool (account a) in world state, with delegated stakes of account b
    // Prepare: deposits (account b) to pool (account a)
    // Prepare: 0 < vp.power < pvp.power
    // Commands (account b): Withdraw Deposit (to reduce the delegated stakes in pool (account a))
    #[test]
    fn test_withdrawal_deposit_bounded_by_pvp() {
        let mut state = create_state(None);
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        pool.set_operator(ACCOUNT_A);
        pool.set_power(100_000);
        pool.set_commission_rate(1);
        pool.set_operator_stake(None);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
        deposit.set_balance(100_000);
        deposit.set_auto_stake_rewards(false);
        NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: ACCOUNT_B,
                power: 100_000,
            }))
            .unwrap();
        NetworkAccount::pvp(&mut state.ctx.gas_meter)
            .push(
                Pool {
                    operator: ACCOUNT_A,
                    commission_rate: 1,
                    power: 100_000,
                    operator_stake: None,
                },
                vec![StakeValue::new(Stake {
                    owner: ACCOUNT_B,
                    power: 90_000,
                })],
            )
            .unwrap();
        NetworkAccount::vp(&mut state.ctx.gas_meter)
            .push(
                Pool {
                    operator: ACCOUNT_A,
                    commission_rate: 1,
                    power: 100_000,
                    operator_stake: None,
                },
                vec![StakeValue::new(Stake {
                    owner: ACCOUNT_B,
                    power: 80_000,
                })],
            )
            .unwrap();

        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        let mut state = create_state(Some(ws));
        let owner_balance_before = rw_set.ws.balance(ACCOUNT_B);
        drop(rw_set);

        let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 40_000,
        })];
        let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
        let ret = execute_commands(state, commands);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(
            ret.receipt.as_ref().unwrap().last().unwrap().return_values,
            10_000_u64.to_le_bytes().to_vec()
        );
        let gas_used = extract_gas_used(&ret);
        assert_eq!(gas_used, 383140);

        let mut state = create_state(Some(ret.new_state));

        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            90_000
        );
        let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .delegated_stakes()
            .get_by(&ACCOUNT_B)
            .unwrap();
        assert_eq!((stake.owner, stake.power), (ACCOUNT_B, 90_000));
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .power()
                .unwrap(),
            90_000
        );
        let rw_set = state.ctx.lock().unwrap();
        let owner_balance_after = rw_set.ws.balance(ACCOUNT_B);

        assert_eq!(
            owner_balance_before,
            owner_balance_after + gas_used + tx_base_cost - 10_000
        );
    }

    // Prepare: no pool in world state
    // Prepare: empty pvp and vp.
    // Commands (account a): Next Epoch
    #[test]
    fn test_next_epoch_no_pool() {
        let mut state = create_state(None);
        NetworkAccount::new(&mut state.ctx.gas_meter).set_current_epoch(0);
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        let state = create_state(Some(ws));
        let mut state = execute_next_epoch(state);
        assert_eq!(
            NetworkAccount::new(&mut state.ctx.gas_meter).current_epoch(),
            1
        );
    }

    // Prepare: pool (account a) in world state, included in nvp.
    //              with delegated stakes of account b, auto_stake_reward = false
    //              with non-zero value of Operator Stake, auto_stake_reward = false
    // Prepare: empty pvp and vp.
    // Commands (account a): Next Epoch
    #[test]
    fn test_next_epoch_single_pool() {
        let ws = prepare_single_pool(false, false);
        let state = create_state(Some(ws));
        let mut state = execute_next_epoch(state);

        // PVP should be empty
        assert_eq!(NetworkAccount::pvp(&mut state.ctx.gas_meter).length(), 0);
        // VP is copied by nvp (nvp is not changed as auto_stake_rewards = false)
        let mut vp = NetworkAccount::vp(&mut state.ctx.gas_meter);
        assert_eq!(vp.length(), 1);
        let pool_in_vp: Pool = vp.pool_at(0).unwrap().try_into().unwrap();
        let stakes_in_vp = vp
            .pool(ACCOUNT_A)
            .unwrap()
            .delegated_stakes()
            .get(0)
            .unwrap();
        // No rewards at first epoch
        assert_eq!(
            (
                pool_in_vp.operator,
                pool_in_vp.commission_rate,
                pool_in_vp.power,
                pool_in_vp.operator_stake
            ),
            (
                ACCOUNT_A,
                1,
                100_000,
                Some(Stake {
                    owner: ACCOUNT_A,
                    power: 10_000
                })
            )
        );
        assert_eq!(
            (stakes_in_vp.owner, stakes_in_vp.power),
            (ACCOUNT_B, 90_000)
        );
        // NVP unchanged
        let nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
        assert_eq!(nvp.length(), 1);
        let pool_in_nvp = nvp.get(0).unwrap();
        assert_eq!(
            (pool_in_nvp.operator, pool_in_nvp.power),
            (ACCOUNT_A, 100_000)
        );
        // pool unchanged
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
        assert_eq!(
            (
                pool.operator().unwrap(),
                pool.commission_rate().unwrap(),
                pool.power().unwrap(),
                pool.operator_stake().unwrap()
            ),
            (
                ACCOUNT_A,
                1,
                100_000,
                Some(Stake {
                    owner: ACCOUNT_A,
                    power: 10_000
                })
            )
        );
        let delegated_stakes = pool.delegated_stakes();
        let delegated_stake = delegated_stakes.get(0).unwrap();
        assert_eq!(
            (delegated_stake.owner, delegated_stake.power),
            (ACCOUNT_B, 90_000)
        );
        // deposits unchanged
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A)
                .balance()
                .unwrap(),
            10_000
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            90_000
        );

        // Epoch increased by 1
        assert_eq!(
            NetworkAccount::new(&mut state.ctx.gas_meter).current_epoch(),
            1
        );
    }

    // Prepare: pool (account a) in world state, included in nvp.
    //              with delegated stakes of account b, auto_stake_reward = false
    //              with non-zero value of Operator Stake, auto_stake_reward = false
    // Prepare: empty pvp. valid vp with pool (account a) and stakes (account b).
    // Commands (account a): Next Epoch, Next Epoch
    #[test]
    fn test_next_epoch_single_pool_with_vp() {
        let ws = prepare_single_pool(false, false);
        let mut state = create_state(Some(ws));
        state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
        // prepare data by executing first epoch, assume test result is correct from test_next_epoch_single_pool
        let mut state = execute_next_epoch(state);
        state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
        // second epoch
        state.tx.nonce = 1;
        let mut state = execute_next_epoch(state);

        // PVP is copied by VP
        let mut pvp = NetworkAccount::pvp(&mut state.ctx.gas_meter);
        assert_eq!(pvp.length(), 1);
        let pool_in_pvp: Pool = pvp.pool_at(0).unwrap().try_into().unwrap();
        let stakes_in_pvp = pvp
            .pool(ACCOUNT_A)
            .unwrap()
            .delegated_stakes()
            .get(0)
            .unwrap();
        assert_eq!(
            (
                pool_in_pvp.operator,
                pool_in_pvp.commission_rate,
                pool_in_pvp.power,
                pool_in_pvp.operator_stake
            ),
            (
                ACCOUNT_A,
                1,
                100_000,
                Some(Stake {
                    owner: ACCOUNT_A,
                    power: 10_000
                })
            )
        );
        assert_eq!(
            (stakes_in_pvp.owner, stakes_in_pvp.power),
            (ACCOUNT_B, 90_000)
        );
        // VP is copied by nvp (nvp is not changed as auto_stake_rewards = false)
        let mut vp = NetworkAccount::pvp(&mut state.ctx.gas_meter);
        assert_eq!(vp.length(), 1);
        let pool_in_vp: Pool = vp.pool_at(0).unwrap().try_into().unwrap();
        let stakes_in_vp = vp
            .pool(ACCOUNT_A)
            .unwrap()
            .delegated_stakes()
            .get(0)
            .unwrap();
        assert_eq!(
            (
                pool_in_vp.operator,
                pool_in_vp.commission_rate,
                pool_in_vp.power,
                pool_in_vp.operator_stake
            ),
            (
                ACCOUNT_A,
                1,
                100_000,
                Some(Stake {
                    owner: ACCOUNT_A,
                    power: 10_000
                })
            )
        );
        assert_eq!(
            (stakes_in_vp.owner, stakes_in_vp.power),
            (ACCOUNT_B, 90_000)
        );

        // deposits are rewarded, assume 64 blocks per epoch (test setup)
        // pool rewards = (100_000 * 8.346 / 100) / 365 = 22
        // reward for b = 22 * 9 / 10 = 19
        // reward for a = 22 * 1 / 10 = 2
        // commission fee from b = 19 * 1% = 0
        // reward for b after commission fee = 19 - 0 = 19
        // reward for a after commission fee = 2 + 0 = 2

        // NVP unchanged (auto stakes reward = false)
        let nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
        assert_eq!(nvp.length(), 1);
        let pool_in_nvp = nvp.get(0).unwrap();
        assert_eq!(
            (pool_in_nvp.operator, pool_in_nvp.power),
            (ACCOUNT_A, 100_000)
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A)
                .balance()
                .unwrap(),
            10_002
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            90_019
        );

        // Epoch increased by 1
        assert_eq!(
            NetworkAccount::new(&mut state.ctx.gas_meter).current_epoch(),
            2
        );
    }

    // Prepare: pool (account a) in world state, included in nvp.
    //              with delegated stakes of account b, auto_stake_reward = true
    //              with non-zero value of Operator Stake, auto_stake_reward = true
    // Prepare: empty pvp and vp.
    // Commands (account a): Next Epoch, Next Epoch
    #[test]
    fn test_next_epoch_single_pool_auto_stake() {
        let ws = prepare_single_pool(true, true);
        let mut state = create_state(Some(ws));
        state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
        // prepare data by executing first epoch, assume test result is correct from test_next_epoch_single_pool
        let mut state = execute_next_epoch(state);
        state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
        // second epoch
        state.tx.nonce = 1;
        let mut state = execute_next_epoch(state);

        // PVP is copied by VP
        let mut pvp = NetworkAccount::pvp(&mut state.ctx.gas_meter);
        assert_eq!(pvp.length(), 1);
        let pool_in_pvp: Pool = pvp.pool_at(0).unwrap().try_into().unwrap();
        let stakes_in_pvp = pvp
            .pool(ACCOUNT_A)
            .unwrap()
            .delegated_stakes()
            .get(0)
            .unwrap();
        assert_eq!(
            (
                pool_in_pvp.operator,
                pool_in_pvp.commission_rate,
                pool_in_pvp.power,
                pool_in_pvp.operator_stake
            ),
            (
                ACCOUNT_A,
                1,
                100_000,
                Some(Stake {
                    owner: ACCOUNT_A,
                    power: 10_000
                })
            )
        );
        assert_eq!(
            (stakes_in_pvp.owner, stakes_in_pvp.power),
            (ACCOUNT_B, 90_000)
        );
        // VP is copied by nvp (nvp is not changed as auto_stake_rewards = false)
        let mut vp = NetworkAccount::pvp(&mut state.ctx.gas_meter);
        assert_eq!(vp.length(), 1);
        let pool_in_vp: Pool = vp.pool_at(0).unwrap().try_into().unwrap();
        let stakes_in_vp = vp
            .pool(ACCOUNT_A)
            .unwrap()
            .delegated_stakes()
            .get(0)
            .unwrap();
        assert_eq!(
            (
                pool_in_vp.operator,
                pool_in_vp.commission_rate,
                pool_in_vp.power,
                pool_in_vp.operator_stake
            ),
            (
                ACCOUNT_A,
                1,
                100_000,
                Some(Stake {
                    owner: ACCOUNT_A,
                    power: 10_000
                })
            )
        );
        assert_eq!(
            (stakes_in_vp.owner, stakes_in_vp.power),
            (ACCOUNT_B, 90_000)
        );
        // deposits are rewarded, assume 64 blocks per epoch (test setup)
        // pool rewards = (100_000 * 8.346 / 100) / 365 = 22
        // reward for b = 22 * 9 / 10 = 19
        // reward for a = 22 * 1 / 10 = 2
        // commission fee from b = 19 * 1% = 0
        // reward for b after commission fee = 19 - 0 = 19
        // reward for a after commission fee = 2 + 0 = 2

        // NVP changed (auto stakes reward = false)
        let nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
        assert_eq!(nvp.length(), 1);
        let pool_in_nvp = nvp.get(0).unwrap();
        assert_eq!(
            (pool_in_nvp.operator, pool_in_nvp.power),
            (ACCOUNT_A, 100_021) // + pool increase in pool power = 19 + 2 = 21
        );
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .operator_stake()
                .unwrap()
                .unwrap()
                .power,
            10_002
        );
        assert_eq!(
            NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
                .delegated_stakes()
                .get_by(&ACCOUNT_B)
                .unwrap()
                .power,
            90_019
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A)
                .balance()
                .unwrap(),
            10_002
        );
        assert_eq!(
            NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B)
                .balance()
                .unwrap(),
            90_019
        );
    }

    // Prepare: add max. number of pools in world state, included in nvp.
    //              with max. number of delegated stakes of accounts, auto_stake_reward = false
    //              with non-zero value of Operator Stake, auto_stake_reward = false
    // Prepare: empty pvp and vp.
    // Commands (account a): Next Epoch, Next Epoch
    #[test]
    fn test_next_epoch_multiple_pools_and_stakes() {
        let mut state = create_state(None);

        let mut rw_set = state.ctx.rw_set.lock().unwrap();
        prepare_accounts_balance(&mut rw_set.ws);
        drop(rw_set);

        create_full_nvp_pool_stakes_deposits(&mut state, false, false, false);
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let mut state = create_state(Some(ws));

        // First Epoch
        state.bd.validator_performance = Some(all_nodes_performance());
        let t = std::time::Instant::now();
        let mut state = execute_next_epoch(state);
        println!("next epoch 1 exec time: {}", t.elapsed().as_millis());

        assert_eq!(NetworkAccount::pvp(&mut state.ctx.gas_meter).length(), 0);
        assert_eq!(
            NetworkAccount::vp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );

        {
            // open account storage state for speed up read operations
            let rw_set = state.ctx.rw_set.lock().unwrap();
            let acc_state = rw_set
                .ws
                .account_storage_state(constants::NETWORK_ADDRESS)
                .unwrap();
            drop(rw_set);
            let mut state = super::protocol::NetworkAccountWorldState::new(&mut state, acc_state);

            // Pool power of vp and nvp are equal
            let l = NetworkAccount::vp(&mut state).length();
            for i in 0..l {
                let vp: Pool = NetworkAccount::vp(&mut state)
                    .pool_at(i)
                    .unwrap()
                    .try_into()
                    .unwrap();
                let nvp = NetworkAccount::nvp(&mut state)
                    .get_by(&vp.operator)
                    .unwrap();
                assert_eq!(vp.power, nvp.power);
            }

            // Stakes in VP and Deposits are not rewarded
            let mut pool_operator_stakes = HashMap::new();
            for i in 0..l {
                let mut vp_dict = NetworkAccount::vp(&mut state);
                let vp = vp_dict.pool_at(i).unwrap();
                let vp_operator = vp.operator().unwrap();
                let vp_power = vp.power().unwrap();
                let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
                let mut sum = 0;
                for j in 0..TEST_MAX_STAKES_PER_POOL {
                    let (address, power) = init_setup_stake_of_owner(j);
                    let stake = NetworkAccount::vp(&mut state)
                        .pool(vp_operator)
                        .unwrap()
                        .delegated_stakes()
                        .get_by(&address)
                        .unwrap();
                    assert_eq!(stake.power, power);
                    sum += stake.power;
                    let deposit = NetworkAccount::deposits(&mut state, vp_operator, address)
                        .balance()
                        .unwrap();
                    assert_eq!(deposit, power);
                }
                pool_operator_stakes.insert(vp_operator, vp_operator_stake_power);
                sum += vp_operator_stake_power;
                assert_eq!(sum, vp_power);
            }
            // Operator Stakes and Deposits are not rewarded
            for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
                let (operator, power, _) = init_setup_pool_power(i);
                assert_eq!(pool_operator_stakes.get(&operator).unwrap(), &power);
                assert!(NetworkAccount::deposits(&mut state, operator, operator)
                    .balance()
                    .is_none());
            }
        }

        // Second Epoch
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        drop(rw_set);

        state.bd.validator_performance = Some(all_nodes_performance());
        state.tx.nonce = 1;
        let t = std::time::Instant::now();
        let mut state = execute_next_epoch(state);
        println!("next epoch 2 exec time: {}", t.elapsed().as_millis());

        assert_eq!(
            NetworkAccount::pvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::vp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );

        {
            // open account storage state for speed up read operations
            let rw_set = state.ctx.rw_set.lock().unwrap();
            let acc_state = rw_set
                .ws
                .account_storage_state(constants::NETWORK_ADDRESS)
                .unwrap();
            drop(rw_set);
            let mut state = super::protocol::NetworkAccountWorldState::new(&mut state, acc_state);

            // Pool power of pvp, vp and nvp are equal
            let l = NetworkAccount::vp(&mut state).length();
            for i in 0..l {
                let pvp: Pool = NetworkAccount::pvp(&mut state)
                    .pool_at(i)
                    .unwrap()
                    .try_into()
                    .unwrap();
                let nvp = NetworkAccount::nvp(&mut state)
                    .get_by(&pvp.operator)
                    .unwrap();
                assert_eq!(pvp.power, nvp.power);

                let vp: Pool = NetworkAccount::vp(&mut state)
                    .pool_at(i)
                    .unwrap()
                    .try_into()
                    .unwrap();
                let nvp = NetworkAccount::nvp(&mut state)
                    .get_by(&vp.operator)
                    .unwrap();
                assert_eq!(vp.power, nvp.power);
            }

            // Stakes are not rewarded, Desposits are rewarded
            let mut pool_operator_stakes = HashMap::new();
            for i in 0..l {
                let mut vp_dict = NetworkAccount::vp(&mut state);
                let vp = vp_dict.pool_at(i).unwrap();
                let vp_operator = vp.operator().unwrap();
                let vp_power = vp.power().unwrap();
                let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
                let mut sum = 0;
                for j in 0..TEST_MAX_STAKES_PER_POOL {
                    let (address, power) = init_setup_stake_of_owner(j);
                    let stake = NetworkAccount::vp(&mut state)
                        .pool(vp_operator)
                        .unwrap()
                        .delegated_stakes()
                        .get_by(&address)
                        .unwrap();
                    sum += stake.power;
                    assert_eq!(stake.power, power);
                    let deposit = NetworkAccount::deposits(&mut state, vp_operator, address)
                        .balance()
                        .unwrap();
                    assert!(deposit > power);
                }
                pool_operator_stakes.insert(vp_operator, vp_operator_stake_power);
                sum += vp_operator_stake_power;
                assert_eq!(sum, vp_power);
            }
            // Operator Stakes are not reward, Deposits are rewarded
            for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
                let (operator, power, _) = init_setup_pool_power(i);
                assert_eq!(pool_operator_stakes.get(&operator).unwrap(), &power);
                assert!(
                    NetworkAccount::deposits(&mut state, operator, operator).balance() > Some(0)
                );
            }
        }
    }

    // Prepare: add max. number of pools in world state, included in nvp.
    //              with max. number of delegated stakes of accounts, auto_stake_reward = true
    //              with non-zero value of Operator Stake, auto_stake_reward = true
    // Prepare: empty pvp and vp.
    // Commands (account a): Next Epoch, Next Epoch
    #[test]
    fn test_next_epoch_multiple_pools_and_stakes_auto_stake() {
        let mut state = create_state(None);

        let mut rw_set = state.ctx.rw_set.lock().unwrap();
        prepare_accounts_balance(&mut rw_set.ws);
        drop(rw_set);

        create_full_nvp_pool_stakes_deposits(&mut state, true, true, true);
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);
        let mut state = create_state(Some(ws));

        // First Epoch
        state.bd.validator_performance = Some(all_nodes_performance());
        let t = std::time::Instant::now();
        let mut state = execute_next_epoch(state);
        println!("next epoch 1 exec time: {}", t.elapsed().as_millis());

        assert_eq!(NetworkAccount::pvp(&mut state.ctx.gas_meter).length(), 0);
        assert_eq!(
            NetworkAccount::vp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );

        {
            // open account storage state for speed up read operations
            let rw_set = state.ctx.rw_set.lock().unwrap();
            let acc_state = rw_set
                .ws
                .account_storage_state(constants::NETWORK_ADDRESS)
                .unwrap();
            drop(rw_set);
            let mut state = super::protocol::NetworkAccountWorldState::new(&mut state, acc_state);

            // Pool power of vp and nvp are equal
            let l = NetworkAccount::vp(&mut state).length();
            for i in 0..l {
                let vp: Pool = NetworkAccount::vp(&mut state)
                    .pool_at(i)
                    .unwrap()
                    .try_into()
                    .unwrap();
                let nvp = NetworkAccount::nvp(&mut state)
                    .get_by(&vp.operator)
                    .unwrap();
                assert_eq!(vp.power, nvp.power);
            }

            // Stakes in VP and Deposits are not rewarded
            let mut pool_operator_stakes = HashMap::new();
            for i in 0..l {
                let mut vp_dict = NetworkAccount::vp(&mut state);
                let vp = vp_dict.pool_at(i).unwrap();
                let vp_operator = vp.operator().unwrap();
                let vp_power = vp.power().unwrap();
                let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
                let mut sum = 0;
                for j in 0..TEST_MAX_STAKES_PER_POOL {
                    let (address, power) = init_setup_stake_of_owner(j);
                    let stake = NetworkAccount::vp(&mut state)
                        .pool(vp_operator)
                        .unwrap()
                        .delegated_stakes()
                        .get_by(&address)
                        .unwrap();
                    assert_eq!(stake.power, power);
                    sum += stake.power;
                    let deposit = NetworkAccount::deposits(&mut state, vp_operator, address)
                        .balance()
                        .unwrap();
                    assert_eq!(deposit, power);
                }
                pool_operator_stakes.insert(vp_operator, vp_operator_stake_power);
                sum += vp_operator_stake_power;
                assert_eq!(sum, vp_power);
            }
            // Operator Stakes and Deposits are not rewarded
            for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
                let (operator, power, _) = init_setup_pool_power(i);
                assert_eq!(pool_operator_stakes.get(&operator).unwrap(), &power);
                assert_eq!(
                    NetworkAccount::deposits(&mut state, operator, operator).balance(),
                    Some(power)
                );
            }
        }

        // Second Epoch
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        drop(rw_set);
        state.bd.validator_performance = Some(all_nodes_performance());
        state.tx.nonce = 1;
        let t = std::time::Instant::now();
        let mut state = execute_next_epoch(state);
        println!("next epoch 2 exec time: {}", t.elapsed().as_millis());

        assert_eq!(
            NetworkAccount::pvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::vp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
        assert_eq!(
            NetworkAccount::nvp(&mut state.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );

        {
            // open account storage state for speed up read operations
            let rw_set = state.ctx.rw_set.lock().unwrap();
            let acc_state = rw_set
                .ws
                .account_storage_state(constants::NETWORK_ADDRESS)
                .unwrap();
            drop(rw_set);
            let mut state = super::protocol::NetworkAccountWorldState::new(&mut state, acc_state);

            // Pool power of vp and nvp are equal and greater than pool power of pvp
            let l = NetworkAccount::vp(&mut state).length();
            for i in 0..l {
                let pvp: Pool = NetworkAccount::pvp(&mut state)
                    .pool_at(i)
                    .unwrap()
                    .try_into()
                    .unwrap();
                let nvp = NetworkAccount::nvp(&mut state)
                    .get_by(&pvp.operator)
                    .unwrap();
                assert!(pvp.power < nvp.power);

                let vp: Pool = NetworkAccount::vp(&mut state)
                    .pool_at(i)
                    .unwrap()
                    .try_into()
                    .unwrap();
                let nvp = NetworkAccount::nvp(&mut state)
                    .get_by(&vp.operator)
                    .unwrap();
                assert_eq!(vp.power, nvp.power);
            }

            // Stakes and Desposits are rewarded
            let mut pool_operator_stakes = HashMap::new();
            for i in 0..l {
                let mut vp_dict = NetworkAccount::vp(&mut state);
                let vp = vp_dict.pool_at(i).unwrap();
                let vp_operator = vp.operator().unwrap();
                let vp_power = vp.power().unwrap();
                let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
                let mut sum = 0;
                for j in 0..TEST_MAX_STAKES_PER_POOL {
                    let (address, power) = init_setup_stake_of_owner(j);
                    let stake = NetworkAccount::vp(&mut state)
                        .pool(vp_operator)
                        .unwrap()
                        .delegated_stakes()
                        .get_by(&address)
                        .unwrap();
                    sum += stake.power;
                    assert!(stake.power > power);
                    let deposit = NetworkAccount::deposits(&mut state, vp_operator, address)
                        .balance()
                        .unwrap();
                    assert_eq!(deposit, stake.power);
                }
                pool_operator_stakes.insert(vp_operator, vp_operator_stake_power);
                sum += vp_operator_stake_power;
                assert_eq!(sum, vp_power);
            }
            // Operator Stakes and Deposits are rewarded (As Operator enable auto-stake-reward)
            for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
                let (operator, power, _) = init_setup_pool_power(i);
                assert!(pool_operator_stakes.get(&operator).unwrap() > &power);
                assert_eq!(
                    pool_operator_stakes.get(&operator).unwrap(),
                    &NetworkAccount::deposits(&mut state, operator, operator)
                        .balance()
                        .unwrap()
                );
            }
        }
    }

    // Prepare: add max. number of pools in world state, included in nvp.
    // Prepare: empty pvp and vp.
    // Commands: Next Epoch, Delete Pool (account a), Next Epoch, Create Pool (account b), Next Epoch
    #[test]
    fn test_change_of_validators() {
        let mut state = create_state(None);
        create_full_pools_in_nvp(&mut state, false, false);
        let rw_set = state.ctx.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        drop(rw_set);

        let state = create_state(Some(ws));
        let mut state = execute_next_epoch(state);

        state.tx.nonce = 1;
        let ret = execute_commands(state, vec![Command::DeletePool]);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 357250);

        let mut state = create_state(Some(ret.new_state));

        state.tx.nonce = 2;
        let state = execute_next_epoch(state);

        let rw_set = state.ctx.lock().unwrap();
        let mut state = create_state(Some(rw_set.ws.to_owned()));
        drop(rw_set);

        state.tx.signer = ACCOUNT_B;
        state.tx.nonce = 0;
        let ret = execute_commands(
            state,
            vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
        );
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        assert_eq!(extract_gas_used(&ret), 1432070);
        let mut state = create_state(Some(ret.new_state));

        state.tx.nonce = 3;
        execute_next_epoch(state);
    }

    fn create_state(init_ws: Option<WorldState<SimpleStore>>) -> ExecutionState<SimpleStore> {
        let ws = match init_ws {
            Some(ws) => ws,
            None => {
                let mut ws = WorldState::initialize(SimpleStore {
                    inner: HashMap::new(),
                });
                ws.with_commit().set_balance(ACCOUNT_A, 500_000_000);
                ws.with_commit().set_balance(ACCOUNT_B, 500_000_000);
                ws.with_commit().set_balance(ACCOUNT_C, 500_000_000);
                ws.with_commit().set_balance(ACCOUNT_D, 500_000_000);
                ws
            }
        };
        let tx = create_tx(ACCOUNT_A);
        let ctx = TransitionContext::new(ws, tx.gas_limit);
        let base_tx = BaseTx::from(&tx);

        ExecutionState {
            bd: create_bd(),
            tx_size: tx.serialize().len(),
            commands_len: 0,
            tx: base_tx,
            ctx,
        }
    }

    fn set_tx(
        state: &mut ExecutionState<SimpleStore>,
        signer: PublicAddress,
        nonce: u64,
        commands: &Vec<Command>,
    ) -> u64 {
        let mut tx = create_tx(signer);
        tx.nonce = nonce;
        state.tx_size = tx.serialize().len();
        state.tx = BaseTx::from(&tx);
        state.commands_len = commands.len();
        gas::tx_inclusion_cost(state.tx_size, state.commands_len)
    }

    fn create_tx(signer: PublicAddress) -> Transaction {
        Transaction {
            signer,
            gas_limit: 10_000_000,
            priority_fee_per_gas: 0,
            max_base_fee_per_gas: MIN_BASE_FEE,
            nonce: 0,
            hash: [0u8; 32],
            signature: [0u8; 64],
            commands: Vec::new(),
        }
    }

    fn create_bd() -> BlockchainParams {
        let mut validator_performance = ValidatorPerformance::default();
        validator_performance.blocks_per_epoch = TEST_MAX_VALIDATOR_SET_SIZE as u32;
        for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
            let mut address = [1u8; 32];
            address[0] = i as u8;
            validator_performance
                .stats
                .insert(address, BlockProposalStats::new(1));
        }
        BlockchainParams {
            this_block_number: 1,
            prev_block_hash: [3u8; 32],
            this_base_fee: 1,
            timestamp: 1665370157,
            random_bytes: [255u8; 32],
            proposer_address: [99u8; 32],
            treasury_address: [100u8; 32],
            cur_view: 1234,
            validator_performance: Some(validator_performance),
        }
    }

    fn single_node_performance(address: PublicAddress, num_of_blocks: u32) -> ValidatorPerformance {
        let mut validator_performance = ValidatorPerformance::default();
        validator_performance.blocks_per_epoch = num_of_blocks;
        validator_performance
            .stats
            .insert(address, BlockProposalStats::new(num_of_blocks));
        validator_performance
    }

    fn all_nodes_performance() -> ValidatorPerformance {
        let mut validator_performance = ValidatorPerformance::default();
        validator_performance.blocks_per_epoch = TEST_MAX_STAKES_PER_POOL as u32;

        for i in 0..TEST_MAX_STAKES_PER_POOL {
            let mut address = [1u8; 32];
            address[0] = i as u8;
            validator_performance
                .stats
                .insert(address, BlockProposalStats::new(1));
        }
        validator_performance
    }

    /// Account address range from \[X, X, X, X, ... , 2\] where X starts with u32(\[2,2,2,2\]). Number of Accounts = MAX_STAKES_PER_POOL
    /// all balance = 500_000_000
    fn prepare_accounts_balance(ws: &mut WorldState<SimpleStore>) {
        let start = u32::from_le_bytes([2u8, 2, 2, 2]);
        for i in 0..TEST_MAX_STAKES_PER_POOL {
            let mut address = [2u8; 32];
            address[0..4].copy_from_slice(&(start + i as u32).to_le_bytes().to_vec());
            ws.cached().set_balance(address, 500_000_000);
        }
        ws.commit();
    }

    /// Pools address range from \[X, 1, 1, 1, ... , 1\] where X \in \[1, TEST_MAX_VALIDATOR_SET_SIZE\]
    /// Pool powers = 100_000, 200_000, ... , 6_400_000
    fn create_full_pools_in_nvp(
        ws: &mut ExecutionState<SimpleStore>,
        add_operators_deposit: bool,
        operators_auto_stake_rewards: bool,
    ) {
        NetworkAccount::nvp(&mut ws.ctx.gas_meter).clear();
        for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
            let (address, power, rate) = init_setup_pool_power(i);
            let mut pool = NetworkAccount::pools(&mut ws.ctx.gas_meter, address);
            pool.set_operator(address);
            pool.set_power(power);
            pool.set_commission_rate(rate);
            pool.set_operator_stake(Some(Stake {
                owner: address,
                power,
            }));
            NetworkAccount::nvp(&mut ws.ctx.gas_meter)
                .insert(PoolKey {
                    operator: address,
                    power,
                })
                .unwrap();
            if add_operators_deposit {
                NetworkAccount::deposits(&mut ws.ctx.gas_meter, address, address)
                    .set_balance(power);
                NetworkAccount::deposits(&mut ws.ctx.gas_meter, address, address)
                    .set_auto_stake_rewards(operators_auto_stake_rewards);
            }
        }
        assert_eq!(
            NetworkAccount::nvp(&mut ws.ctx.gas_meter).length(),
            TEST_MAX_VALIDATOR_SET_SIZE as u32
        );
    }

    /// Stake address = [i, 1, 1, 1, 1, 1, 1, 1, ...]
    /// Pool powers = 100_000 * (i)
    /// Commission_rate = i % 100
    fn init_setup_pool_power(i: u16) -> (PublicAddress, u64, u8) {
        let mut address = [1u8; 32];
        address[0] = i as u8;
        let power = 100_000 * i as u64;
        (address, power, i as u8 % 100)
    }

    /// Staker address range from \[X, X, X, X, ... , 2\] where X starts with u32(\[2,2,2,2\]). Number of stakers = TEST_MAX_STAKES_PER_POOL
    /// Stake powers = 200_000, 300_000, ...
    fn create_full_stakes_in_pool(ws: &mut ExecutionState<SimpleStore>, operator: PublicAddress) {
        NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
            .delegated_stakes()
            .clear();
        let mut sum = 0;
        let mut vs = vec![];
        for i in 0..TEST_MAX_STAKES_PER_POOL {
            let (address, power) = init_setup_stake_of_owner(i);
            sum += power;
            let stake = StakeValue::new(Stake {
                owner: address,
                power,
            });
            vs.push(stake);
        }
        NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
            .delegated_stakes()
            .reset(vs)
            .unwrap();
        let operator_stake = NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
            .operator_stake()
            .map_or(0, |p| p.map_or(0, |v| v.power));
        NetworkAccount::pools(&mut ws.ctx.gas_meter, operator).set_operator(operator);
        NetworkAccount::pools(&mut ws.ctx.gas_meter, operator).set_power(sum + operator_stake);
        NetworkAccount::nvp(&mut ws.ctx.gas_meter).change_key(PoolKey {
            operator,
            power: sum + operator_stake,
        });
        assert_eq!(
            NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
                .delegated_stakes()
                .length(),
            TEST_MAX_STAKES_PER_POOL as u32
        );
    }

    /// Stake address = [X, X, X, X, 2, 2, 2, 2, ...] where X,X,X,X is i as LE bytes
    /// Stake powers = 100_000 * (i+2)
    fn init_setup_stake_of_owner(i: u16) -> (PublicAddress, u64) {
        let start = u32::from_le_bytes([2u8, 2, 2, 2]);
        let mut address = [2u8; 32];
        address[0..4].copy_from_slice(&(start + i as u32).to_le_bytes().to_vec());
        (address, 100_000 * (i + 2) as u64)
    }

    /// Staker address range from \[X, X, X, X, ... , 2\] where X starts with u32(\[2,2,2,2\]). Number of stakers = TEST_MAX_STAKES_PER_POOL
    /// Deposits = 200_000, 300_000, ...
    fn create_full_deposits_in_pool(
        ws: &mut ExecutionState<SimpleStore>,
        operator: PublicAddress,
        auto_stake_rewards: bool,
    ) {
        for i in 0..TEST_MAX_STAKES_PER_POOL {
            let (address, balance) = init_setup_stake_of_owner(i);
            NetworkAccount::deposits(&mut ws.ctx.gas_meter, operator, address).set_balance(balance);
            NetworkAccount::deposits(&mut ws.ctx.gas_meter, operator, address)
                .set_auto_stake_rewards(auto_stake_rewards);
        }
    }
    fn create_full_nvp_pool_stakes_deposits(
        ws: &mut ExecutionState<SimpleStore>,
        auto_stake_rewards: bool,
        add_operators_deposit: bool,
        operators_auto_stake_rewards: bool,
    ) {
        create_full_pools_in_nvp(ws, add_operators_deposit, operators_auto_stake_rewards);
        let mut nvps = vec![];
        for i in 0..TEST_MAX_VALIDATOR_SET_SIZE {
            let p = NetworkAccount::nvp(&mut ws.ctx.gas_meter)
                .get(i as u32)
                .unwrap();
            nvps.push(p);
        }
        for p in nvps {
            create_full_stakes_in_pool(ws, p.operator);
            create_full_deposits_in_pool(ws, p.operator, auto_stake_rewards);
        }
    }

    // pool (account a) in world state, included in nvp.
    //      with delegated stakes of account b, auto_stake_reward = false
    //      with non-zero value of Operator Stake, auto_stake_reward = false
    // pool[A].power = 100_000
    // pool[A].operator_stake = 10_000
    // pool[A].delegated_stakes[B] = 90_000
    // deposits[A, A] = 10_000
    // deposits[A, B] = 90_000
    fn prepare_single_pool(
        auto_stake_rewards_a: bool,
        auto_stake_rewards_b: bool,
    ) -> WorldState<SimpleStore> {
        let mut state = create_state(None);
        setup_pool(
            &mut state,
            ACCOUNT_A,
            10_000,
            ACCOUNT_B,
            90_000,
            auto_stake_rewards_a,
            auto_stake_rewards_b,
        );
        let rw_set = state.ctx.rw_set.lock().unwrap();
        let ws = rw_set.clone().commit_to_world_state();
        ws
    }

    // pool[A].power = 100_000
    // pool[A].operator_stake = 10_000
    // pool[A].delegated_stakes[B] = 90_000
    // deposits[A, A] = 10_000
    // deposits[A, B] = 90_000
    fn setup_pool(
        state: &mut ExecutionState<SimpleStore>,
        operator: PublicAddress,
        operator_power: u64,
        owner: PublicAddress,
        owner_power: u64,
        auto_stake_rewards_a: bool,
        auto_stake_rewards_b: bool,
    ) {
        let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
        pool.set_operator(operator);
        pool.set_power(operator_power + owner_power);
        pool.set_commission_rate(1);
        pool.set_operator_stake(Some(Stake {
            owner: operator,
            power: operator_power,
        }));
        NetworkAccount::pools(&mut state.ctx.gas_meter, operator)
            .delegated_stakes()
            .insert(StakeValue::new(Stake {
                owner: owner,
                power: owner_power,
            }))
            .unwrap();
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, operator);
        deposit.set_balance(operator_power);
        deposit.set_auto_stake_rewards(auto_stake_rewards_a);
        let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
        deposit.set_balance(owner_power);
        deposit.set_auto_stake_rewards(auto_stake_rewards_b);
        NetworkAccount::nvp(&mut state.ctx.gas_meter)
            .insert(PoolKey {
                operator,
                power: operator_power + owner_power,
            })
            .unwrap();
    }

    fn execute_next_epoch(state: ExecutionState<SimpleStore>) -> ExecutionState<SimpleStore> {
        let ret = execute_next_epoch_command(state, vec![Command::NextEpoch]);
        assert_eq!(
            (
                &ret.error,
                &ret.receipt.as_ref().unwrap().last().unwrap().exit_status
            ),
            (&None, &ExitStatus::Success)
        );
        let gas_used = ret.receipt.unwrap().iter().map(|g| g.gas_used).sum::<u64>();
        println!("gas_consumed {}", gas_used);
        println!(
            "new validators {}",
            ret.validator_changes
                .as_ref()
                .unwrap()
                .new_validator_set
                .len()
        );
        println!(
            "remove validators {}",
            ret.validator_changes
                .as_ref()
                .unwrap()
                .remove_validator_set
                .len()
        );
        create_state(Some(ret.new_state))
    }

    fn extract_gas_used(ret: &TransitionResult<SimpleStore>) -> u64 {
        ret.receipt
            .as_ref()
            .unwrap()
            .iter()
            .map(|g| g.gas_used)
            .sum::<u64>()
    }
}
