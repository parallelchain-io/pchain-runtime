/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Execution logics of Network Commands.

use pchain_types::{PublicAddress, Stake, Command};
use pchain_world_state::{
    network::{network_account::{NetworkAccount, NetworkAccountStorage}, pool::{PoolKey}, stake::StakeValue}, 
    storage::WorldStateStorage
};

use crate::{
    transition::{StateChangesResult}, 
    TransitionError
};

use super::{
    phase::{self, StateInTransit}, execute::TryExecuteResult
};

/// Execution Logic for Network Commands. Err If the Command is not Network Command.
pub(crate) fn try_execute<S>(
    state: StateInTransit<S>, 
    command: &Command
) -> TryExecuteResult<S> 
    where S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static
{
    let ret = match command {
        Command::CreatePool { commission_rate } => 
            create_pool(state, *commission_rate),
        Command::SetPoolSettings { commission_rate } => 
            set_pool_settings(state, *commission_rate),
        Command::DeletePool => 
            delete_pool(state),
        Command::CreateDeposit { operator, balance, auto_stake_rewards } => 
            create_deposit(state, *operator, *balance, *auto_stake_rewards),
        Command::SetDepositSettings { operator, auto_stake_rewards } => 
            set_deposit_settings(state, *operator, *auto_stake_rewards),
        Command::TopUpDeposit { operator, amount } => 
            topup_deposit(state, *operator, *amount),
        Command::WithdrawDeposit { operator, max_amount } => 
            withdraw_deposit(state, *operator, *max_amount),
        Command::StakeDeposit { operator, max_amount } => 
            stake_deposit(state, *operator, *max_amount),
        Command::UnstakeDeposit { operator, max_amount } => 
            unstake_deposit(state, *operator, *max_amount),
        _=> return TryExecuteResult::Err(state)
    };

    TryExecuteResult::Ok(ret)
}

/// Execution of Work step for [pchain_types::Command::CreatePool]
pub(crate) fn create_pool<S>(
    mut state: StateInTransit<S>,
    commission_rate: u8,
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let operator = state.tx.signer;

    if commission_rate > 100 {
        return Err(phase::abort(state, TransitionError::InvalidPoolPolicy))
    }

    // Create Pool
    let mut pool = NetworkAccount::pools(&mut state, operator);
    if pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolAlreadyExists))
    }
    pool.set_operator(operator);
    pool.set_power(0);
    pool.set_commission_rate(commission_rate);
    pool.set_operator_stake(None);

    // Update NVP
    let _ = NetworkAccount::nvp(&mut state).insert_extract(PoolKey { operator, power: 0});

    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::SetPoolSettings]
pub(crate) fn set_pool_settings<S>(
    mut state: StateInTransit<S>,
    new_commission_rate: u8
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let operator = state.tx.signer;

    if new_commission_rate > 100 {
        return Err(phase::abort(state, TransitionError::InvalidPoolPolicy))
    }

    // Update Pool
    let mut pool = NetworkAccount::pools(&mut state, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists))
    }

    if pool.commission_rate() == Some(new_commission_rate) {
        return Err(phase::abort(state, TransitionError::InvalidPoolPolicy))
    }

    pool.set_commission_rate(new_commission_rate);

    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::DeletePool]
pub(crate) fn delete_pool<S>(
    mut state: StateInTransit<S>,
) -> Result<StateInTransit<S>, StateChangesResult<S>> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    let operator = state.tx.signer;
    let pool = NetworkAccount::pools(&mut state, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists))
    }

    NetworkAccount::nvp(&mut state).remove_item(&operator);

    NetworkAccount::pools(&mut state, operator).delete();

    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::CreateDeposit]
pub(crate) fn create_deposit<S>(
    mut state: StateInTransit<S>, 
    operator: PublicAddress,
    balance: u64,
    auto_stake_rewards: bool,
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let owner = state.tx.signer;

    if !NetworkAccount::pools(&mut state, operator).exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists))
    }
    
    if NetworkAccount::deposits(&mut state, operator, owner).exists() {
        return Err(phase::abort(state, TransitionError::DepositsAlreadyExists))
    }

    let (owner_balance, _) = state.balance(owner);
    if owner_balance < balance {
        return Err(phase::abort(state, TransitionError::NotEnoughBalanceForTransfer))
    }
    state.set_balance(owner, owner_balance - balance);
    
    let mut deposits = NetworkAccount::deposits(&mut state, operator, owner);
    deposits.set_balance(balance);
    deposits.set_auto_stake_rewards(auto_stake_rewards);
    
    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::SetDepositSettings]
pub(crate) fn set_deposit_settings<S>(
    mut state: StateInTransit<S>, 
    operator: PublicAddress,
    new_auto_stake_rewards: bool,
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let owner = state.tx.signer;

    let mut deposits = NetworkAccount::deposits(&mut state, operator, owner);
    if !deposits.exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists))
    }

    if deposits.auto_stake_rewards() == Some(new_auto_stake_rewards) {
        return Err(phase::abort(state, TransitionError::InvalidDepositPolicy))
    }

    deposits.set_auto_stake_rewards(new_auto_stake_rewards);

    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::TopUpDeposit]
pub(crate) fn topup_deposit<S>(
    mut state: StateInTransit<S>, 
    operator: PublicAddress,
    amount: u64
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let owner = state.tx.signer;

    if !NetworkAccount::deposits(&mut state, operator, owner).exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists))
    }

    let (owner_balance, _) = state.balance(owner);
    if owner_balance < amount {
        return Err(phase::abort(state, TransitionError::NotEnoughBalanceForTransfer))
    }
    state.set_balance(owner, owner_balance - amount);

    let mut deposits = NetworkAccount::deposits(&mut state, operator, owner);
    let deposit_balance = deposits.balance().unwrap();
    deposits.set_balance(deposit_balance + amount);

    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::WithdrawDeposit]
pub(crate) fn withdraw_deposit<S>(
    mut state: StateInTransit<S>, 
    operator: PublicAddress,
    max_amount: u64,
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let owner = state.tx.signer;

    // 1. Check if there is any deposit to withdraw
    let deposits = NetworkAccount::deposits(&mut state, operator, owner);
    if !deposits.exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists))
    }
    let deposit_balance = deposits.balance().unwrap();

    // 2. Compute withdrawal Amount
    let prev_epoch_locked_power = NetworkAccount::pvp(&mut state)
        .pool(operator).map_or(0, |mut pool|{
            if operator == owner {
                pool.operator_stake().map_or(0, |stake| stake.map_or(0, |s| s.power))
            } else {
                pool.delegated_stakes().get_by(&owner).map_or(0,|stake| stake.power )
            }
        }
    );
    let cur_epoch_locked_power = NetworkAccount::vp(&mut state)
        .pool(operator).map_or(0, |mut pool|{
            if operator == owner {
                pool.operator_stake().map_or(0, |stake| stake.map_or(0, |s| s.power))
            } else {
                pool.delegated_stakes().get_by(&owner).map_or(0,|stake| stake.power )
            }
        }
    );
    let locked_power = std::cmp::max(prev_epoch_locked_power, cur_epoch_locked_power);
    let new_deposit_balance = std::cmp::max(deposit_balance.saturating_sub(max_amount), locked_power);

    // 3. Abort if there is no amount currently available to withdraw.
    if new_deposit_balance == deposit_balance { // e.g. max_amount = 0  or deposit_balance == locked_power
        return Err(phase::abort(state, TransitionError::InvalidStakeAmount))
    }

    // 4. Update the deposit's balance to reflect the withdrawal.
    NetworkAccount::deposits(&mut state, operator, owner).set_balance(new_deposit_balance);
    let (owner_balance, _) = state.balance(owner);
    state.set_balance(owner, owner_balance + deposit_balance - new_deposit_balance);

    // 5. If the deposit's new balance is now to small to support its Stake in the next Epoch, cap the Stake's power at the new balance.
    if let Ok(stake_power) = stake_of_pool(&mut state, operator, owner) {
        if new_deposit_balance < stake_power {
            if let Some(prev_pool_power) = NetworkAccount::pools(&mut state, operator).power(){
                reduce_stake_power(&mut state, operator, prev_pool_power, owner, stake_power, stake_power - new_deposit_balance);
            }
        }
    }

    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::StakeDeposit]
pub(crate) fn stake_deposit<S>(
    mut state: StateInTransit<S>, 
    operator: PublicAddress,
    max_amount: u64,
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let owner = state.tx.signer;

    // 1. Check if there is a Deposit to stake
    if !NetworkAccount::deposits(&mut state, operator, owner).exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists))
    }
    let deposit_balance = NetworkAccount::deposits(&mut state, operator, owner).balance().unwrap();

    // 2. Check if there is a Pool to stake to.
    let pool = NetworkAccount::pools(&mut state, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists))
    }
    let prev_pool_power = pool.power().unwrap();

    // We use this to update the Pool's power after the power of one of its stakes get increased.
    let stake_power =
    if operator == owner {
        match pool.operator_stake() {
            Some(Some(operator_stake)) => Some(operator_stake.power),
            _ => None
        }
    } else {
        let mut pool = pool;
        pool.delegated_stakes().get_by(&owner).map(|stake| stake.power )
    };
    let increase_amount = std::cmp::min(max_amount, deposit_balance.saturating_sub(stake_power.unwrap_or(0)));
    if increase_amount == 0 {
        return Err(phase::abort(state, TransitionError::InvalidStakeAmount))
    }

    // Update Stakes and the Pool's power and its position in the Next Validator Set.
    match increase_stake_power(&mut state, operator, prev_pool_power, owner, stake_power, increase_amount, true) {
        Ok(_) => {},
        Err(_) => return Err(phase::abort(state, TransitionError::InvalidStakeAmount))
    };

    phase::finalize_gas_consumption(state)
}

/// Execution of Work step for [pchain_types::Command::UnstakeDeposit]
pub(crate) fn unstake_deposit<S>(
    mut state: StateInTransit<S>, 
    operator: PublicAddress, 
    max_amount: u64
) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone
{
    let owner = state.tx.signer;

    // 1. Check if there is a Deposit to unstake with.
    if !NetworkAccount::deposits(&mut state, operator, owner).exists() {
        return Err(phase::abort(state, TransitionError::DepositsNotExists))
    }

    // 2. If there is no Pool, then there is no Stake to unstake.
    let pool = NetworkAccount::pools(&mut state, operator);
    if !pool.exists() {
        return Err(phase::abort(state, TransitionError::PoolNotExists))
    }
    let prev_pool_power = pool.power().unwrap();

    let stake_power =  {
        if operator == owner {
            match pool.operator_stake() {
                Some(Some(stake)) => stake.power,
                _ => return Err(phase::abort(state, TransitionError::PoolHasNoStakes))
            }
        } else {
            let mut pool = pool;
            match pool.delegated_stakes().get_by(&owner) {
                Some(stake) => stake.power,
                None => return Err(phase::abort(state, TransitionError::PoolHasNoStakes))
            }
        }
    };

    // 3. Reduce the Stake's power.
    reduce_stake_power(&mut state, operator, prev_pool_power, owner, stake_power, max_amount);

    phase::finalize_gas_consumption(state)
}

/// return owner's stake from operator's pool (NVS)
pub(crate) fn stake_of_pool<T>(
    state: &mut T,
    operator: PublicAddress,
    owner: PublicAddress
) -> Result<u64, ()>
    where T: NetworkAccountStorage
{
    let stake_power =  {
        if operator == owner {
            match NetworkAccount::pools(state, operator).operator_stake() {
                Some(Some(stake)) => stake.power,
                _ => return Err(())
            }
        } else {
            match NetworkAccount::pools(state, operator).delegated_stakes().get_by(&owner) {
                Some(stake) => stake.power,
                None => return Err(())
            }
        }
    };
    Ok(stake_power)
}

/// Reduce stake power and update Pool position in Next validator set.
pub(crate) fn reduce_stake_power<T>(
    state: &mut T, 
    operator: PublicAddress, 
    pool_power: u64,
    owner: PublicAddress, 
    stake_power: u64,
    reduce_amount: u64
)
    where T: NetworkAccountStorage
{
    // Reduce the Stake's power.
    let new_pool_power = 
    if stake_power <= reduce_amount {
        // If the Stake's power is less than the amount to be reduced, remove the Stake.
        if operator == owner {
            NetworkAccount::pools(state, operator).set_operator_stake(None);
        } else {
            NetworkAccount::pools(state, operator).delegated_stakes().remove_item(&owner);
        }
        pool_power.saturating_sub(stake_power)
    } else {
        // Otherwise, reduce the Stake's power.
        let new_state = Stake { owner, power: stake_power - reduce_amount};
        if operator == owner {
            NetworkAccount::pools(state, operator).set_operator_stake(Some(new_state));
        } else {
            NetworkAccount::pools(state, operator).delegated_stakes().change_key(StakeValue::new(new_state));
        }
        pool_power.saturating_sub(reduce_amount)
    };

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
        },
        None => {
            if new_pool_power > 0 {
                let _ = NetworkAccount::nvp(state).insert_extract(PoolKey { operator, power: new_pool_power });
            }
        }
    }
}

/// increase_stake_power increases stake power and also update the NVP.
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
    increase_amount: u64,
    exit_on_insert_fail: bool
) -> Result<(), ()>
    where T: NetworkAccountStorage
{
    let mut pool = NetworkAccount::pools(state, operator);
    
    let power_to_add = 
    if operator == owner {
        let stake_power = stake_power.unwrap_or(0);
        pool.set_operator_stake(Some(Stake { owner: operator, power: stake_power + increase_amount }));
        increase_amount
    } else {
        let mut delegated_stakes = pool.delegated_stakes();
        match stake_power {
            Some(stake_power) => {
                delegated_stakes.change_key(StakeValue::new(Stake { owner, power: stake_power + increase_amount }));
                increase_amount
            },
            None => {
                match delegated_stakes.insert_extract(StakeValue::new(Stake { owner, power: increase_amount })) {
                    Ok(Some(replaced_stake)) => increase_amount - replaced_stake.power,
                    Ok(None) => increase_amount,
                    Err(_) => {
                        if exit_on_insert_fail { return Err(()) }
                        increase_amount
                    }
                }
            }
        }
    };
    
    let new_pool_power = pool_power + power_to_add;
    pool.set_power(new_pool_power);
    match NetworkAccount::nvp(state).get_by(&operator) {
        Some(mut pool_key) => {
            pool_key.power = new_pool_power;
            NetworkAccount::nvp(state).change_key(pool_key);
        },
        None => {
            let _ = NetworkAccount::nvp(state).insert_extract(PoolKey { operator, power: new_pool_power });
        }
    }
    Ok(())
}