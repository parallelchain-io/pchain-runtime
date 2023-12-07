/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Specifies the execution behaviour of Commands (and resulting DeferredCommands)
//! that are sent by users to the blockchain.
//! Note that the NextEpoch Command is handled separately in [execute_next_epoch_command](crate::execution::execute_next_epoch).

use pchain_types::{
    blockchain::Command,
    cryptography::PublicAddress,
    runtime::{
        CallInput, CreateDepositInput, CreatePoolInput, DeployInput, SetDepositSettingsInput,
        SetPoolSettingsInput, StakeDepositInput, TopUpDepositInput, TransferInput,
        UnstakeDepositInput, WithdrawDepositInput,
    },
};
use pchain_world_state::{VersionProvider, DB};

use crate::{
    commands::{account, staking},
    execution::state::ExecutionState,
    types::DeferredCommand,
    TransitionError,
};

/// The Execute trait must be implemented by Commands and DeferredCommands.
pub(crate) trait Execute<S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn execute(
        self,
        state: &mut ExecutionState<S, E, V>,
        command_index: usize,
    ) -> Result<(), TransitionError>;
}

impl<S, E, V> Execute<S, E, V> for Command
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    fn execute(
        self,
        state: &mut ExecutionState<S, E, V>,
        command_index: usize,
    ) -> Result<(), TransitionError> {
        let actor = state.tx.signer;
        execute(state, command_index, actor, self)
    }
}

impl<S, E, V> Execute<S, E, V> for DeferredCommand
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    fn execute(
        self,
        state: &mut ExecutionState<S, E, V>,
        command_index: usize,
    ) -> Result<(), TransitionError> {
        let actor = self.contract_address;
        let command = self.command;
        execute(state, command_index, actor, command)
    }
}

fn execute<'a, 'b, S, E, V>(
    state: &'b mut ExecutionState<'a, S, E, V>,
    command_index: usize,
    actor: PublicAddress,
    command: Command,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    match command {
        Command::Transfer(TransferInput { recipient, amount }) => {
            account::transfer(state, recipient, amount)
        }
        Command::Deploy(DeployInput {
            contract,
            cbi_version,
        }) => account::deploy(state, command_index as u32, contract.to_vec(), cbi_version),
        Command::Call(CallInput {
            target,
            method,
            arguments,
            amount,
        }) => account::call(
            state,
            false,
            target,
            method.clone(),
            arguments.clone(),
            amount,
        ),
        Command::CreatePool(CreatePoolInput { commission_rate }) => {
            staking::create_pool(actor, state, commission_rate)
        }
        Command::SetPoolSettings(SetPoolSettingsInput { commission_rate }) => {
            staking::set_pool_settings(actor, state, commission_rate)
        }
        Command::DeletePool => staking::delete_pool(actor, state),
        Command::CreateDeposit(CreateDepositInput {
            operator,
            balance,
            auto_stake_rewards,
        }) => staking::create_deposit(actor, state, operator, balance, auto_stake_rewards),
        Command::SetDepositSettings(SetDepositSettingsInput {
            operator,
            auto_stake_rewards,
        }) => staking::set_deposit_settings(actor, state, operator, auto_stake_rewards),
        Command::TopUpDeposit(TopUpDepositInput { operator, amount }) => {
            staking::topup_deposit(actor, state, operator, amount)
        }
        Command::WithdrawDeposit(WithdrawDepositInput {
            operator,
            max_amount,
        }) => staking::withdraw_deposit(actor, state, operator, max_amount),
        Command::StakeDeposit(StakeDepositInput {
            operator,
            max_amount,
        }) => staking::stake_deposit(actor, state, operator, max_amount),
        Command::UnstakeDeposit(UnstakeDepositInput {
            operator,
            max_amount,
        }) => staking::unstake_deposit(actor, state, operator, max_amount),
        _ => unreachable!(), // Next Epoch Command
    }
}
