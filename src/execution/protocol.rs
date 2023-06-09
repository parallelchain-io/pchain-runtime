/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of executing [Protocol Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#protocol-commands).

use std::collections::HashMap;

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{
    keys::AppKey,
    network::{
        constants::NETWORK_ADDRESS,
        network_account::{NetworkAccount, NetworkAccountStorage},
        pool::Pool,
        stake::StakeValue,
    },
    states::AccountStorageState,
    storage::WorldStateStorage,
};

use crate::{
    formulas::issuance_reward, read_write_set::ReadWriteSet, BlockProposalStats, ValidatorChanges,
};

use super::state::ExecutionState;

/// Execution of [pchain_types::blockchain::Command::NextEpoch]
pub(crate) fn next_epoch<S>(mut state: ExecutionState<S>) -> (ExecutionState<S>, ValidatorChanges)
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let block_performance = state.bd.validator_performance.clone().unwrap();

    let new_validator_set = {
        let acc_state = state.ws.account_storage_state(NETWORK_ADDRESS).unwrap();
        let mut state = NetworkAccountWorldState::new(&mut state, acc_state);

        let mut pools_in_vp = Vec::new();
        let mut stakes_of_vp = HashMap::<PublicAddress, Vec<StakeValue>>::new();
        let mut auto_stakes: Vec<(PublicAddress, PublicAddress, u64)> = Vec::new();

        // 1. Reward each Stake in VS
        // 1.1 calculate total reward
        let current_epoch = NetworkAccount::new(&mut state).current_epoch();
        let pool_length = NetworkAccount::vp(&mut state).length();
        for i in 0..pool_length {
            let mut vp = NetworkAccount::vp(&mut state);
            let pool = vp.pool_at(i).unwrap();
            pools_in_vp.push(Pool {
                operator: pool.operator().unwrap(),
                commission_rate: pool.commission_rate().unwrap(),
                power: pool.power().map_or(0, |power| power),
                operator_stake: pool.operator_stake().and_then(|opt_stake| opt_stake),
            });
        }

        for pool in &pools_in_vp {
            let pool_operator = pool.operator;
            let pool_power = pool.power;
            let pool_operator_own_stake = pool.operator_stake.map_or(0, |s| s.power);
            let commission_rate = pool.commission_rate;
            let stats = block_performance
                .stats
                .get(&pool_operator)
                .map_or(BlockProposalStats::new(0), |stat| stat.clone());
            let pool_reward = pool_reward(
                current_epoch,
                pool_power,
                stats.num_of_proposed_blocks,
                block_performance.blocks_per_epoch / pool_length,
            );

            // 1.2 Calculate total stakes of this pool
            let mut total_stakes = pool_operator_own_stake;
            let mut vp_stakes = Vec::new();
            let mut vp = NetworkAccount::vp(&mut state);
            if let Some(mut vp_pool) = vp.pool(pool_operator) {
                let stakes = vp_pool.delegated_stakes();
                let stakes_length = stakes.length();
                for j in 0..stakes_length {
                    let stake = stakes.get(j).unwrap();
                    total_stakes = total_stakes.saturating_add(stake.power);
                    vp_stakes.push(stake);
                }
            }
            // 1.3 Distribute pool rewards to stakers
            let mut total_commission_fee: u64 = 0;
            let mut stakers_to_reward = Vec::new();
            if pool_reward > 0 {
                for stake in &vp_stakes {
                    let (stake_reward, commission_fee) =
                        stake_reward(pool_reward, commission_rate, stake.power, total_stakes);
                    stakers_to_reward.push((stake.owner, stake_reward));
                    total_commission_fee = total_commission_fee.saturating_add(commission_fee);
                }
            }
            stakes_of_vp.insert(pool_operator, vp_stakes);

            for (stake_owner, reward) in stakers_to_reward {
                let mut stake_owner_deposit =
                    NetworkAccount::deposits(&mut state, pool_operator, stake_owner);
                if let Some(balance) = stake_owner_deposit.balance() {
                    stake_owner_deposit.set_balance(balance.saturating_add(reward));
                }

                // auto stake rewards for stakers
                if stake_owner_deposit.auto_stake_rewards() == Some(true) {
                    auto_stakes.push((pool_operator, stake_owner, reward));
                }
            }

            // 1.4 Reward Pool's own stakes
            if pool_reward > 0 {
                let (pool_operator_stake_reward, _) =
                    stake_reward(pool_reward, 0, pool_operator_own_stake, total_stakes);
                let mut operator_deposits =
                    NetworkAccount::deposits(&mut state, pool_operator, pool_operator);
                let pool_operator_total_reward =
                    pool_operator_stake_reward.saturating_add(total_commission_fee);
                match operator_deposits.balance() {
                    Some(balance) => {
                        operator_deposits
                            .set_balance(balance.saturating_add(pool_operator_total_reward));
                    }
                    None => {
                        // create deposit if not exist
                        operator_deposits.set_balance(pool_operator_total_reward);
                        operator_deposits.set_auto_stake_rewards(false);
                    }
                }

                // auto stake rewards for operators
                if operator_deposits.auto_stake_rewards() == Some(true) {
                    auto_stakes.push((pool_operator, pool_operator, pool_operator_total_reward));
                }
            }
        }

        // Auto Stake to NVP
        for (operator, owner, increase_amount) in auto_stakes {
            let mut pool = NetworkAccount::pools(&mut state, operator);
            if !pool.exists() {
                continue;
            }
            let pool_power = pool.power().unwrap_or(0);
            let stake_power = if operator == owner {
                match pool.operator_stake() {
                    Some(Some(stake)) => Some(stake.power),
                    _ => None,
                }
            } else {
                pool.delegated_stakes()
                    .get_by(&owner)
                    .map(|stake| stake.power)
            };
            let _ = super::staking::increase_stake_power(
                &mut state,
                operator,
                pool_power,
                owner,
                stake_power,
                increase_amount,
                false,
            );
        }

        // 2. Replace PVS with VS
        NetworkAccount::pvp(&mut state).clear();
        for pool in &pools_in_vp {
            let delegated_stakes = stakes_of_vp.remove(&pool.operator).unwrap();
            let _ = NetworkAccount::pvp(&mut state).push(pool.clone(), delegated_stakes);
        }

        // 3. Replace VS with NVS
        let mut next_validator_set = Vec::new();
        NetworkAccount::vp(&mut state).clear();
        let pool_length = NetworkAccount::nvp(&mut state).length();
        for i in 0..pool_length {
            let pool = NetworkAccount::nvp(&mut state).get(i).unwrap();
            let pool_operator = pool.operator;
            let mut pool = NetworkAccount::pools(&mut state, pool_operator);

            let pool_to_vs = Pool {
                operator: pool.operator().unwrap(),
                commission_rate: pool.commission_rate().unwrap(),
                power: pool.power().unwrap(),
                operator_stake: pool.operator_stake().unwrap(),
            };
            next_validator_set.push((pool_to_vs.operator, pool_to_vs.power));

            let delegated_stakes = pool.delegated_stakes().unordered_values();

            let _ = NetworkAccount::vp(&mut state).push(pool_to_vs, delegated_stakes);
        }

        // 4. Bump up Current Epoch by 1.
        NetworkAccount::new(&mut state).set_current_epoch(current_epoch + 1);

        // 5. Update validator set
        let new_validator_set: Vec<(PublicAddress, u64)> = next_validator_set
            .iter()
            .filter_map(|(new_p, new_power)| {
                if !pools_in_vp
                    .iter()
                    .any(|old_p| old_p.operator == *new_p && old_p.power == *new_power)
                {
                    Some((*new_p, *new_power))
                } else {
                    None
                }
            })
            .collect();
        let remove_validator_set = pools_in_vp
            .iter()
            .filter_map(|old_p| {
                if !next_validator_set
                    .iter()
                    .any(|new_p| new_p.0 == old_p.operator)
                {
                    Some(old_p.operator)
                } else {
                    None
                }
            })
            .collect();

        ValidatorChanges {
            new_validator_set,
            remove_validator_set,
        }
    };

    // There is no Gas consumption as we use NetworkAccountWorldState for accessing the world state
    (state, new_validator_set)
}

/// NetworkAccountWorldState is specific to accessing storage of an Account Storage State.
/// It stores account storage state and use it for subsequent Read / Writes operations.
/// Write opertions would store to read write set.
/// Different with [state::ExecutionState] which also implements Trait [NetworkAccountStorage],
/// it does not charge gas for opertaions.
pub(crate) struct NetworkAccountWorldState<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    account_storage_state: AccountStorageState<S>,
    rw_set: &'a mut ReadWriteSet<S>,
}

impl<'a, S> NetworkAccountWorldState<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    pub(crate) fn new(
        state: &'a mut ExecutionState<S>,
        account_storage_state: AccountStorageState<S>,
    ) -> Self {
        Self {
            account_storage_state,
            rw_set: &mut state.ctx.rw_set,
        }
    }
}

impl<'a, S> NetworkAccountStorage for NetworkAccountWorldState<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.rw_set.app_data_from_account_storage_state(
            &self.account_storage_state,
            AppKey::new(key.to_vec()),
        )
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.rw_set.contains_app_data_from_account_storage_state(
            &self.account_storage_state,
            AppKey::new(key.to_vec()),
        )
    }

    fn set(&mut self, key: &[u8], value: Vec<u8>) {
        let address = self.account_storage_state.address();
        self.rw_set
            .set_app_data_uncharged(address, AppKey::new(key.to_vec()), value);
    }

    fn delete(&mut self, key: &[u8]) {
        let address = self.account_storage_state.address();
        self.rw_set
            .set_app_data_uncharged(address, AppKey::new(key.to_vec()), Vec::new());
    }
}

/// Calculate reward of a pool. It is fraction of the pool power to network power, and
/// block performance. Baseline is calculated by: number of blocks per term / number of validators.
///
/// ```text
/// Issuance * PoolStake * min(NumBlocksProposed/Baseline , 1)
/// ```
fn pool_reward(
    current_epoch: u64,
    pool_power: u64,
    num_of_proposed_blocks: u32,
    baseline: u32,
) -> u64 {
    // no reward if it is not expected to propose block
    if baseline == 0 {
        return 0;
    }
    // should not over reward
    if num_of_proposed_blocks > baseline {
        // Issuance * PoolStake * 1
        let (numerator, denominator) = issuance_reward(current_epoch, pool_power);
        return (numerator / denominator) as u64;
    }
    // Issuance * PoolStake * NumBlocksProposed / Baseline
    let (numerator, denominator) = issuance_reward(current_epoch, pool_power);
    ((numerator * num_of_proposed_blocks as u128) / (denominator * baseline as u128)) as u64
}

/// return reward to the stakes and commission_fee
fn stake_reward(
    pool_reward: u64,
    commission_rate: u8,
    stake_power: u64,
    total_stakes: u64,
) -> (u64, u64) {
    // no reward if there is no stakes at all
    if total_stakes == 0 {
        return (0, 0);
    }
    let reward = (pool_reward as u128 * stake_power as u128) / (total_stakes as u128);
    let commission_fee = (commission_rate as u128 * pool_reward as u128 * stake_power as u128)
        / (100 * total_stakes as u128);
    (
        (reward.saturating_sub(commission_fee)) as u64,
        commission_fee as u64,
    )
}
