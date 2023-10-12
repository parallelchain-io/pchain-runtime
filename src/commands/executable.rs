/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::{
    blockchain::{Command, ReceiptV1, ReceiptV2},
    cryptography::PublicAddress,
    runtime::{
        CallInput, CreateDepositInput, CreatePoolInput, DeployInput, SetDepositSettingsInput,
        SetPoolSettingsInput, StakeDepositInput, TopUpDepositInput, TransferInput,
        UnstakeDepositInput, WithdrawDepositInput,
    },
};
use pchain_world_state::storage::WorldStateStorage;

use crate::{
    commands::account, execution::state::{ExecutionState, FinalizeState}, types::{DeferredCommand, TxnVersion, CommandKind}, TransitionError,
};

use super::staking;

pub(crate) trait Executable {
    fn execute<S>(
        self,
        state: &mut ExecutionState<S>,
        command_index: usize,
    ) -> Result<Option<Vec<DeferredCommand>>, TransitionError>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static;
}

impl Executable for Command {
    fn execute<S>(
        self,
        state: &mut ExecutionState<S>,
        command_index: usize,
    ) -> Result<Option<Vec<DeferredCommand>>, TransitionError>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static,
    {
        let actor = state.tx.signer;
        let command_kind = CommandKind::from(&self);

        let result = execute(state, command_index, actor, self);

        let deferred_commands =  match state.tx.version {
            TxnVersion::V1 => {
                // TODO
                <ExecutionState<S> as FinalizeState<S, ReceiptV1>>::finalize_command_receipt(state, command_kind, &result)
            },
            TxnVersion::V2 => {
                // TODO
                <ExecutionState<S> as FinalizeState<S, ReceiptV2>>::finalize_command_receipt(state, command_kind, &result)
            }
        };

        result.map(|_| deferred_commands)
    }
}

impl Executable for DeferredCommand {
    fn execute<S>(
        self,
        state: &mut ExecutionState<S>,
        command_index: usize,
    ) -> Result<Option<Vec<DeferredCommand>>, TransitionError>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static,
    {
        let actor = self.contract_address;
        let command = self.command;
        let command_kind = CommandKind::from(&command);
        
        let result = execute(state, command_index, actor, command);

        match state.tx.version {
            TxnVersion::V1 => {
                // TODO
                <ExecutionState<S> as FinalizeState<S, ReceiptV1>>::finalize_deferred_command_receipt(state, command_kind, &result);
            },
            TxnVersion::V2 => {
                // TODO
                <ExecutionState<S> as FinalizeState<S, ReceiptV2>>::finalize_deferred_command_receipt(state, command_kind, &result);
            }
        }

        result.map(|_| None)
    }
}

fn execute<S>(
    state: &mut ExecutionState<S>,
    command_index: usize,
    actor: PublicAddress,
    command: Command,
) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
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
