/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of executing [Staking Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#staking-commands).

use pchain_types::blockchain::Command;
use pchain_types::runtime::{
    CreateDepositInput, SetDepositSettingsInput, SetPoolSettingsInput, StakeDepositInput,
    TopUpDepositInput, UnstakeDepositInput, WithdrawDepositInput,
};
use pchain_types::{cryptography::PublicAddress, runtime::CreatePoolInput};
use pchain_world_state::{
    network::{
        network_account::{NetworkAccount, NetworkAccountStorage},
        pool::PoolKey,
        stake::{Stake, StakeValue},
    },
    storage::WorldStateStorage,
};

use crate::cost::CostChange;
use crate::gas;
use crate::{transition::StateChangesResult, TransitionError};

use super::state::ExecutionState;
use super::{
    execute::TryExecuteResult,
    phase::{self},
};

/// Execution Logic for Staking Commands. Err If the Command is not Staking Command.
/// It transits the state according to Metwork Command, on behalf of actor. Actor is expected
/// to be the signer of the transaction, or the contract that triggers deferred command.
pub(crate) fn try_execute<S>(
    actor: PublicAddress,
    state: ExecutionState<S>,
    command: &Command,
) -> TryExecuteResult<S>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static,
{
    let ret = match command {
        Command::CreatePool(CreatePoolInput { commission_rate }) => {
            create_pool(actor, state, *commission_rate)
        }
        Command::SetPoolSettings(SetPoolSettingsInput { commission_rate }) => {
            set_pool_settings(actor, state, *commission_rate)
        }
        Command::DeletePool => delete_pool(actor, state),
        Command::CreateDeposit(CreateDepositInput {
            operator,
            balance,
            auto_stake_rewards,
        }) => create_deposit(actor, state, *operator, *balance, *auto_stake_rewards),
        Command::SetDepositSettings(SetDepositSettingsInput {
            operator,
            auto_stake_rewards,
        }) => set_deposit_settings(actor, state, *operator, *auto_stake_rewards),
        Command::TopUpDeposit(TopUpDepositInput { operator, amount }) => {
            topup_deposit(actor, state, *operator, *amount)
        }
        Command::WithdrawDeposit(WithdrawDepositInput {
            operator,
            max_amount,
        }) => withdraw_deposit(actor, state, *operator, *max_amount),
        Command::StakeDeposit(StakeDepositInput {
            operator,
            max_amount,
        }) => stake_deposit(actor, state, *operator, *max_amount),
        Command::UnstakeDeposit(UnstakeDepositInput {
            operator,
            max_amount,
        }) => unstake_deposit(actor, state, *operator, *max_amount),
        _ => return TryExecuteResult::Err(state),
    };

    TryExecuteResult::Ok(ret)
}

/// Execution of [pchain_types::blockchain::Command::CreatePool]
pub(crate) fn create_pool<S>(
    operator: PublicAddress,
    mut state: ExecutionState<S>,
    commission_rate: u8,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    if commission_rate > 100 {
        return Err(phase::abort(state, TransitionError::InvalidPoolPolicy));
    }

    // Create Pool
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolAlreadyExists));
    }
    pool.set_operator(operator);
    pool.set_power(0);
    pool.set_commission_rate(commission_rate);
    pool.set_operator_stake(None);

    // Update NVP
    let _ = NetworkAccount::nvp(&mut state.ctx.gas_meter)
        .insert_extract(PoolKey { operator, power: 0 });

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::SetPoolSettings]
pub(crate) fn set_pool_settings<S>(
    operator: PublicAddress,
    mut state: ExecutionState<S>,
    new_commission_rate: u8,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    if new_commission_rate > 100 {
        return Err(phase::abort(state, TransitionError::InvalidPoolPolicy));
    }

    // Update Pool
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists));
    }

    if pool.commission_rate() == Some(new_commission_rate) {
        return Err(phase::abort(state, TransitionError::InvalidPoolPolicy));
    }

    pool.set_commission_rate(new_commission_rate);

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::DeletePool]
pub(crate) fn delete_pool<S>(
    operator: PublicAddress,
    mut state: ExecutionState<S>,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists));
    }

    NetworkAccount::nvp(&mut state.ctx.gas_meter).remove_item(&operator);

    NetworkAccount::pools(&mut state.ctx.gas_meter, operator).delete();

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::CreateDeposit]
pub(crate) fn create_deposit<S>(
    owner: PublicAddress,
    mut state: ExecutionState<S>,
    operator: PublicAddress,
    balance: u64,
    auto_stake_rewards: bool,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists));
    }

    if NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner).exists() {
        return Err(phase::abort(state, TransitionError::DepositsAlreadyExists));
    }

    let owner_balance = state.ctx.gas_meter.ws_get_balance(owner);
    if owner_balance < balance {
        return Err(phase::abort(
            state,
            TransitionError::NotEnoughBalanceForTransfer,
        ));
    }
    state
        .ctx
        .gas_meter
        .ws_set_balance(owner, owner_balance - balance);

    let mut deposits = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    deposits.set_balance(balance);
    deposits.set_auto_stake_rewards(auto_stake_rewards);

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::SetDepositSettings]
pub(crate) fn set_deposit_settings<S>(
    owner: PublicAddress,
    mut state: ExecutionState<S>,
    operator: PublicAddress,
    new_auto_stake_rewards: bool,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let mut deposits = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    if !deposits.exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists));
    }

    if deposits.auto_stake_rewards() == Some(new_auto_stake_rewards) {
        return Err(phase::abort(state, TransitionError::InvalidDepositPolicy));
    }

    deposits.set_auto_stake_rewards(new_auto_stake_rewards);

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::TopUpDeposit]
pub(crate) fn topup_deposit<S>(
    owner: PublicAddress,
    mut state: ExecutionState<S>,
    operator: PublicAddress,
    amount: u64,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    if !NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner).exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists));
    }

    let owner_balance = state.ctx.gas_meter.ws_get_balance(owner);
    if owner_balance < amount {
        return Err(phase::abort(
            state,
            TransitionError::NotEnoughBalanceForTransfer,
        ));
    }

    state
        .ctx
        .gas_meter
        .ws_set_balance(owner, owner_balance - amount); // Always deduct the amount specified in the transaction

    let mut deposits = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    let deposit_balance = deposits.balance().unwrap();
    deposits.set_balance(deposit_balance.saturating_add(amount)); // Ceiling to MAX for safety. Overflow should not happen in real situation.

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::WithdrawDeposit]
pub(crate) fn withdraw_deposit<S>(
    owner: PublicAddress,
    mut state: ExecutionState<S>,
    operator: PublicAddress,
    max_amount: u64,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    // 1. Check if there is any deposit to withdraw
    let deposits = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    if !deposits.exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists));
    }
    let deposit_balance = deposits.balance().unwrap();

    // 2. Compute withdrawal amount
    let prev_epoch_locked_power = NetworkAccount::pvp(&mut state.ctx.gas_meter)
        .pool(operator)
        .map_or(0, |mut pool| {
            if operator == owner {
                pool.operator_stake()
                    .map_or(0, |stake| stake.map_or(0, |s| s.power))
            } else {
                pool.delegated_stakes()
                    .get_by(&owner)
                    .map_or(0, |stake| stake.power)
            }
        });
    let cur_epoch_locked_power = NetworkAccount::vp(&mut state.ctx.gas_meter)
        .pool(operator)
        .map_or(0, |mut pool| {
            if operator == owner {
                pool.operator_stake()
                    .map_or(0, |stake| stake.map_or(0, |s| s.power))
            } else {
                pool.delegated_stakes()
                    .get_by(&owner)
                    .map_or(0, |stake| stake.power)
            }
        });
    let locked_power = std::cmp::max(prev_epoch_locked_power, cur_epoch_locked_power);
    let withdrawal_amount = std::cmp::min(max_amount, deposit_balance.saturating_sub(locked_power));
    let new_deposit_balance = deposit_balance.saturating_sub(withdrawal_amount);

    // 3. Abort if there is no amount currently available to withdraw.
    if new_deposit_balance == deposit_balance {
        // e.g. max_amount = 0  or deposit_balance == locked_power
        return Err(phase::abort(state, TransitionError::InvalidStakeAmount));
    }

    // 4. Update the deposit's balance to reflect the withdrawal.
    if new_deposit_balance == 0 {
        NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner).delete();
    } else {
        NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner)
            .set_balance(new_deposit_balance);
    }

    let owner_balance = state.ctx.gas_meter.ws_get_balance(owner);
    state.ctx.gas_meter.ws_set_balance(
        owner,
        owner_balance.saturating_add(deposit_balance - new_deposit_balance),
    );

    // 5. If the deposit's new balance is now too small to support its Stake in the next Epoch, cap the Stake's power at the new balance.
    if let Some(stake_power) = stake_of_pool(&mut state.ctx.gas_meter, operator, owner) {
        if new_deposit_balance < stake_power {
            if let Some(prev_pool_power) =
                NetworkAccount::pools(&mut state.ctx.gas_meter, operator).power()
            {
                reduce_stake_power(
                    &mut state.ctx.gas_meter,
                    operator,
                    prev_pool_power,
                    owner,
                    stake_power,
                    stake_power - new_deposit_balance,
                );
            }
        }
    }

    // 5. Set the withdrawal amount to return_value
    let return_value = withdrawal_amount.to_le_bytes().to_vec();
    state
        .ctx
        .gas_meter
        .store_txn_post_exec_return_value(return_value);
    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::StakeDeposit]
pub(crate) fn stake_deposit<S>(
    owner: PublicAddress,
    mut state: ExecutionState<S>,
    operator: PublicAddress,
    max_amount: u64,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    // 1. Check if there is a Deposit to stake
    let deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    if !deposit.exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists));
    }
    let deposit_balance = deposit.balance().unwrap();

    // 2. Check if there is a Pool to stake to.
    let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists));
    }
    let prev_pool_power = pool.power().unwrap();

    // We use this to update the Pool's power after the power of one of its stakes get increased.
    let stake_power = stake_of_pool(&mut state.ctx.gas_meter, operator, owner);

    let stake_power_to_increase = std::cmp::min(
        max_amount,
        deposit_balance.saturating_sub(stake_power.unwrap_or(0)),
    );
    if stake_power_to_increase == 0 {
        return Err(phase::abort(state, TransitionError::InvalidStakeAmount));
    }

    // Update Stakes and the Pool's power and its position in the Next Validator Set.
    match increase_stake_power(
        &mut state.ctx.gas_meter,
        operator,
        prev_pool_power,
        owner,
        stake_power,
        stake_power_to_increase,
        true,
    ) {
        Ok(_) => {}
        Err(_) => return Err(phase::abort(state, TransitionError::InvalidStakeAmount)),
    };

    // Set the staked amount to return_value
    let return_value = stake_power_to_increase.to_le_bytes().to_vec();
    state
        .ctx
        .gas_meter
        .store_txn_post_exec_return_value(return_value);

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::UnstakeDeposit]
pub(crate) fn unstake_deposit<S>(
    owner: PublicAddress,
    mut state: ExecutionState<S>,
    operator: PublicAddress,
    max_amount: u64,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone,
{
    // 1. Check if there is a Deposit to unstake.
    if !NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner).exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists));
    }

    // 2. If there is no Pool, then there is no Stake to unstake.
    let pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists));
    }
    let prev_pool_power = pool.power().unwrap();

    let stake_power = match stake_of_pool(&mut state.ctx.gas_meter, operator, owner) {
        Some(stake_power) => stake_power,
        None => return Err(phase::abort(state, TransitionError::PoolHasNoStakes)),
    };

    // 3. Reduce the Stake's power.
    let amount_unstaked = reduce_stake_power(
        &mut state.ctx.gas_meter,
        operator,
        prev_pool_power,
        owner,
        stake_power,
        max_amount,
    );

    // 4. set the unstaked amount to return_value
    let return_value = amount_unstaked.to_le_bytes().to_vec();
    state
        .ctx
        .gas_meter
        .store_txn_post_exec_return_value(return_value);

    phase::finalize_gas_consumption(state)
}

/// return owner's stake from operator's pool (NVS)
pub(crate) fn stake_of_pool<T>(
    state: &mut T,
    operator: PublicAddress,
    owner: PublicAddress,
) -> Option<u64>
where
    T: NetworkAccountStorage,
{
    if operator == owner {
        match NetworkAccount::pools(state, operator).operator_stake() {
            Some(Some(stake)) => Some(stake.power),
            _ => None,
        }
    } else {
        NetworkAccount::pools(state, operator)
            .delegated_stakes()
            .get_by(&owner)
            .map(|stake| stake.power)
    }
}

/// Reduce stake's power and update Pool position in Next validator set.
pub(crate) fn reduce_stake_power<T>(
    state: &mut T,
    operator: PublicAddress,
    pool_power: u64,
    owner: PublicAddress,
    stake_power: u64,
    reduce_amount: u64,
) -> u64
where
    T: NetworkAccountStorage,
{
    // Reduce the Stake's power.
    let amount_unstaked = if stake_power <= reduce_amount {
        // If the Stake's power is less than the amount to be reduced, remove the Stake.
        if operator == owner {
            NetworkAccount::pools(state, operator).set_operator_stake(None);
        } else {
            NetworkAccount::pools(state, operator)
                .delegated_stakes()
                .remove_item(&owner);
        }
        stake_power
    } else {
        // Otherwise, reduce the Stake's power.
        let new_state = Stake {
            owner,
            power: stake_power - reduce_amount,
        };
        if operator == owner {
            NetworkAccount::pools(state, operator).set_operator_stake(Some(new_state));
        } else {
            NetworkAccount::pools(state, operator)
                .delegated_stakes()
                .change_key(StakeValue::new(new_state));
        }
        reduce_amount
    };
    let new_pool_power = pool_power.saturating_sub(amount_unstaked);

    // Update the Pool's power and its position in the Next Validator Set.
    NetworkAccount::pools(state, operator).set_power(new_pool_power);
    match NetworkAccount::nvp(state).get_by(&operator) {
        Some(mut pool_key) => {
            if new_pool_power == 0 {
                NetworkAccount::nvp(state).remove_item(&operator);
            } else {
                pool_key.power = new_pool_power;
                NetworkAccount::nvp(state).change_key(pool_key);
            }
        }
        None => {
            if new_pool_power > 0 {
                let _ = NetworkAccount::nvp(state).insert_extract(PoolKey {
                    operator,
                    power: new_pool_power,
                });
            }
        }
    }
    amount_unstaked
}

/// increase_stake_power increases stake's power and also update the NVP.
// 1a. pool[i].delegated_stakes[j] .change_key or .insert_extract
// 1b. pool[i].operator_stake += v
// 2. pool[i].power += v
// 3. nas.pool[i] .change_key or insert_extract
pub(crate) fn increase_stake_power<T>(
    state: &mut T,
    operator: PublicAddress,
    pool_power: u64,
    owner: PublicAddress,
    stake_power: Option<u64>,
    stake_power_to_increase: u64,
    exit_on_insert_fail: bool,
) -> Result<(), ()>
where
    T: NetworkAccountStorage,
{
    let mut pool = NetworkAccount::pools(state, operator);

    let power_to_add = if operator == owner {
        let stake_power = stake_power.unwrap_or(0);
        pool.set_operator_stake(Some(Stake {
            owner: operator,
            power: stake_power.saturating_add(stake_power_to_increase),
        }));
        stake_power_to_increase
    } else {
        let mut delegated_stakes = pool.delegated_stakes();
        match stake_power {
            Some(stake_power) => {
                delegated_stakes.change_key(StakeValue::new(Stake {
                    owner,
                    power: stake_power.saturating_add(stake_power_to_increase),
                }));
                stake_power_to_increase
            }
            None => {
                match delegated_stakes.insert_extract(StakeValue::new(Stake {
                    owner,
                    power: stake_power_to_increase,
                })) {
                    Ok(Some(replaced_stake)) => {
                        stake_power_to_increase.saturating_sub(replaced_stake.power)
                    }
                    Ok(None) => stake_power_to_increase,
                    Err(_) => {
                        if exit_on_insert_fail {
                            return Err(());
                        }
                        stake_power_to_increase
                    }
                }
            }
        }
    };

    let new_pool_power = pool_power.saturating_add(power_to_add);
    pool.set_power(new_pool_power);
    match NetworkAccount::nvp(state).get_by(&operator) {
        Some(mut pool_key) => {
            pool_key.power = new_pool_power;
            NetworkAccount::nvp(state).change_key(pool_key);
        }
        None => {
            let _ = NetworkAccount::nvp(state).insert_extract(PoolKey {
                operator,
                power: new_pool_power,
            });
        }
    }
    Ok(())
}
