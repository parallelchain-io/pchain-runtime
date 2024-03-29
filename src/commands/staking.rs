/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Business logic used by [Execute](crate::execution::execute) trait implementations for
//! [Staking Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#staking-commands).
//!
//! These commands allow users who wish to operate staking pools (operators)
//! and other users who wish to delegate their tokens as stakes to these pools (delegators)
//! to perform the relevant actions.
//!
//! Note that delegation is a two-stage process involving first, the creation of a deposit,
//! and secondly, staking that deposit to become part of a pool.

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{
    NetworkAccount, NetworkAccountStorage, PoolKey, Stake, StakeValue, VersionProvider, DB,
};

use crate::{
    execution::{
        abort::{abort, abort_if_gas_exhausted},
        state::ExecutionState,
    },
    gas::{blockchain_storage_cost, CostChange},
    types::TxnVersion,
    TransitionError,
};

/* ↓↓↓ Create Pool Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::CreatePool]
pub(crate) fn create_pool<S, E, V>(
    operator: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    commission_rate: u8,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    if commission_rate > 100 {
        abort!(state, TransitionError::InvalidPoolPolicy)
    }

    // Create Pool
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if pool.exists() {
        abort!(state, TransitionError::PoolAlreadyExists)
    }
    pool.set_operator(operator);
    pool.set_power(0);
    pool.set_commission_rate(commission_rate);
    pool.set_operator_stake(None);

    // Update NVP
    let _ = NetworkAccount::nvp(&mut state.ctx.gas_meter)
        .insert_extract(PoolKey { operator, power: 0 });

    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Set Pool Settings Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::SetPoolSettings]
pub(crate) fn set_pool_settings<S, E, V>(
    operator: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    new_commission_rate: u8,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    if new_commission_rate > 100 {
        abort!(state, TransitionError::InvalidPoolPolicy)
    }

    // Update Pool
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        abort!(state, TransitionError::PoolNotExists)
    }

    if pool.commission_rate() == Some(new_commission_rate) {
        abort!(state, TransitionError::InvalidPoolPolicy)
    }

    pool.set_commission_rate(new_commission_rate);

    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Delete Pool Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::DeletePool]
pub(crate) fn delete_pool<S, E, V>(
    operator: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        abort!(state, TransitionError::PoolNotExists)
    }

    NetworkAccount::nvp(&mut state.ctx.gas_meter).remove_item(&operator);

    NetworkAccount::pools(&mut state.ctx.gas_meter, operator).delete();

    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Create Deposit Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::CreateDeposit]
pub(crate) fn create_deposit<S, E, V>(
    owner: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    operator: PublicAddress,
    balance: u64,
    auto_stake_rewards: bool,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    if !pool.exists() {
        abort!(state, TransitionError::PoolNotExists)
    }

    if NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner).exists() {
        abort!(state, TransitionError::DepositsAlreadyExists)
    }

    let owner_balance = state.ctx.gas_meter.ws_balance(owner);
    if owner_balance < balance {
        abort!(state, TransitionError::NotEnoughBalanceForTransfer)
    }
    state
        .ctx
        .gas_meter
        .ws_set_balance(owner, owner_balance - balance);

    let mut deposits = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    deposits.set_balance(balance);
    deposits.set_auto_stake_rewards(auto_stake_rewards);

    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Set Deposit Settings Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::SetDepositSettings]
pub(crate) fn set_deposit_settings<S, E, V>(
    owner: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    operator: PublicAddress,
    new_auto_stake_rewards: bool,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    let mut deposits = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    if !deposits.exists() {
        abort!(state, TransitionError::DepositsNotExists)
    }

    if deposits.auto_stake_rewards() == Some(new_auto_stake_rewards) {
        abort!(state, TransitionError::InvalidDepositPolicy)
    }

    deposits.set_auto_stake_rewards(new_auto_stake_rewards);

    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Top Up Deposit Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::TopUpDeposit]
pub(crate) fn topup_deposit<S, E, V>(
    owner: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    operator: PublicAddress,
    amount: u64,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    if !NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner).exists() {
        abort!(state, TransitionError::DepositsNotExists)
    }

    let owner_balance = state.ctx.gas_meter.ws_balance(owner);
    if owner_balance < amount {
        abort!(state, TransitionError::NotEnoughBalanceForTransfer)
    }

    state
        .ctx
        .gas_meter
        .ws_set_balance(owner, owner_balance - amount); // Always deduct the amount specified in the transaction

    let mut deposits = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    let deposit_balance = deposits.balance().unwrap();
    deposits.set_balance(deposit_balance.saturating_add(amount)); // Ceiling to MAX for safety. Overflow should not happen in real situation.

    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Withdraw Deposit Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::WithdrawDeposit]
pub(crate) fn withdraw_deposit<S, E, V>(
    owner: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    operator: PublicAddress,
    max_amount: u64,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    let gas_meter = &mut state.ctx.gas_meter;

    // 1. Check if there is any deposit to withdraw
    let mut deposits = NetworkAccount::deposits(gas_meter, operator, owner);
    if !deposits.exists() {
        abort!(state, TransitionError::DepositsNotExists)
    }
    let deposit_balance = deposits.balance().unwrap();

    // 2. Compute withdrawal amount
    let prev_epoch_locked_power =
        NetworkAccount::pvp(gas_meter)
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
    let cur_epoch_locked_power =
        NetworkAccount::vp(gas_meter)
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
        abort!(state, TransitionError::InvalidStakeAmount)
    }

    // 4. Update the deposit's balance to reflect the withdrawal.
    if new_deposit_balance == 0 {
        NetworkAccount::deposits(gas_meter, operator, owner).delete();
    } else {
        NetworkAccount::deposits(gas_meter, operator, owner).set_balance(new_deposit_balance);
    }

    let owner_balance = gas_meter.ws_balance(owner);
    gas_meter.ws_set_balance(
        owner,
        owner_balance.saturating_add(deposit_balance - new_deposit_balance),
    );

    // 5. If the deposit's new balance is now too small to support its Stake in the next Epoch, cap the Stake's power at the new balance.
    if let Some(stake_power) = stake_of_pool(gas_meter, operator, owner) {
        if new_deposit_balance < stake_power {
            if let Some(prev_pool_power) = NetworkAccount::pools(gas_meter, operator).power() {
                reduce_stake_power(
                    gas_meter,
                    operator,
                    prev_pool_power,
                    owner,
                    stake_power,
                    stake_power - new_deposit_balance,
                );
            }
        }
    }

    let ret_val_bytes = withdrawal_amount.to_le_bytes().to_vec();
    let ret_val_cost = match state.txn_meta.version {
        TxnVersion::V1 => {
            CostChange::deduct(blockchain_storage_cost(ret_val_bytes.len()))
                .net_cost()
                .0
        }
        TxnVersion::V2 => {
            CostChange::deduct(blockchain_storage_cost(std::mem::size_of::<u64>()))
                .net_cost()
                .0
        }
    };

    // check gas before return value, to preserve behaviour of v0.4
    // in future versions, to refactor it such that the gas meter operation itself checks for gas exhaustion and aborts
    if gas_meter.total_gas_used().saturating_add(ret_val_cost) > state.txn_meta.gas_limit {
        // manually deduct to full exhuastion
        gas_meter.manually_charge_gas(ret_val_cost);
        return abort_if_gas_exhausted(state);
    }

    // 6. Set the withdrawal amount to return_value
    match state.txn_meta.version {
        TxnVersion::V1 => {
            gas_meter.command_output_set_return_value(ret_val_bytes);
        }
        TxnVersion::V2 => {
            gas_meter.command_output_set_amount_withdrawn(withdrawal_amount);
        }
    }

    // technically redundant but still leaving for consistency
    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Stake Deposit Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::StakeDeposit]
pub(crate) fn stake_deposit<S, E, V>(
    owner: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    operator: PublicAddress,
    max_amount: u64,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    let gas_meter = &mut state.ctx.gas_meter;
    // 1. Check if there is a Deposit to stake
    let mut deposit = NetworkAccount::deposits(gas_meter, operator, owner);
    if !deposit.exists() {
        abort!(state, TransitionError::DepositsNotExists)
    }
    let deposit_balance = deposit.balance().unwrap();

    // 2. Check if there is a Pool to stake to.
    let mut pool = NetworkAccount::pools(gas_meter, operator);
    if !pool.exists() {
        abort!(state, TransitionError::PoolNotExists)
    }
    let prev_pool_power = pool.power().unwrap();

    // We use this to update the Pool's power after the power of one of its stakes get increased.
    let stake_power = stake_of_pool(gas_meter, operator, owner);

    let stake_power_to_increase = std::cmp::min(
        max_amount,
        deposit_balance.saturating_sub(stake_power.unwrap_or(0)),
    );
    if stake_power_to_increase == 0 {
        abort!(state, TransitionError::InvalidStakeAmount)
    }

    // Update Stakes and the Pool's power and its position in the Next Validator Set.
    match increase_stake_power(
        gas_meter,
        operator,
        prev_pool_power,
        owner,
        stake_power,
        stake_power_to_increase,
        true,
    ) {
        Ok(_) => {}
        Err(_) => abort!(state, TransitionError::InvalidStakeAmount),
    };

    let amt_staked_bytes = stake_power_to_increase.to_le_bytes().to_vec();
    let amt_staked_bytes_cost = match state.txn_meta.version {
        TxnVersion::V1 => {
            CostChange::deduct(blockchain_storage_cost(amt_staked_bytes.len()))
                .net_cost()
                .0
        }
        TxnVersion::V2 => {
            CostChange::deduct(blockchain_storage_cost(std::mem::size_of::<u64>()))
                .net_cost()
                .0
        }
    };

    // check gas before return value, to preserve behaviour of v0.4
    // in future versions, to refactor it such that the gas meter operation itself checks for gas exhaustion and aborts
    if gas_meter
        .total_gas_used()
        .saturating_add(amt_staked_bytes_cost)
        > state.txn_meta.gas_limit
    {
        // manually deduct to full exhuastion
        gas_meter.manually_charge_gas(amt_staked_bytes_cost);
        return abort_if_gas_exhausted(state);
    }

    // Set the staked amount to return_value
    match state.txn_meta.version {
        TxnVersion::V1 => {
            gas_meter.command_output_set_return_value(amt_staked_bytes);
        }
        TxnVersion::V2 => {
            gas_meter.command_output_set_amount_staked(stake_power_to_increase);
        }
    }

    // technically redundant but still leaving for consistency
    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Unstake Deposit Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::UnstakeDeposit]
pub(crate) fn unstake_deposit<S, E, V>(
    owner: PublicAddress,
    state: &mut ExecutionState<S, E, V>,
    operator: PublicAddress,
    max_amount: u64,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    let gas_meter = &mut state.ctx.gas_meter;
    // 1. Check if there is a Deposit to unstake.
    if !NetworkAccount::deposits(gas_meter, operator, owner).exists() {
        abort!(state, TransitionError::DepositsNotExists)
    }

    // 2. If there is no Pool, then there is no Stake to unstake.
    let mut pool = NetworkAccount::pools(gas_meter, operator);
    if !pool.exists() {
        abort!(state, TransitionError::PoolNotExists)
    }
    let prev_pool_power = pool.power().unwrap();

    let stake_power = match stake_of_pool(gas_meter, operator, owner) {
        Some(stake_power) => stake_power,
        None => abort!(state, TransitionError::PoolHasNoStakes),
    };

    // 3. Reduce the Stake's power.
    let amount_unstaked = reduce_stake_power(
        gas_meter,
        operator,
        prev_pool_power,
        owner,
        stake_power,
        max_amount,
    );

    let amt_unstaked_bytes = amount_unstaked.to_le_bytes().to_vec();
    let amt_unstaked_bytes_cost = match state.txn_meta.version {
        TxnVersion::V1 => {
            CostChange::deduct(blockchain_storage_cost(amt_unstaked_bytes.len()))
                .net_cost()
                .0
        }
        TxnVersion::V2 => {
            CostChange::deduct(blockchain_storage_cost(std::mem::size_of::<u64>()))
                .net_cost()
                .0
        }
    };

    // check gas before return value, to preserve behaviour of v0.4
    // in future versions, to refactor it such that the gas meter operation itself checks for gas exhaustion and aborts
    if gas_meter
        .total_gas_used()
        .saturating_add(amt_unstaked_bytes_cost)
        > state.txn_meta.gas_limit
    {
        // manually deduct to full exhuastion
        gas_meter.manually_charge_gas(amt_unstaked_bytes_cost);
        return abort_if_gas_exhausted(state);
    }

    // 4. set the unstaked amount to return_value
    match state.txn_meta.version {
        TxnVersion::V1 => {
            gas_meter.command_output_set_return_value(amt_unstaked_bytes);
        }
        TxnVersion::V2 => {
            gas_meter.command_output_set_amount_unstaked(amount_unstaked);
        }
    }

    // technically redundant but still leaving for consistency
    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Helpers Command ↓↓↓ */

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
