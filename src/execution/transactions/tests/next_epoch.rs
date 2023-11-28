/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
use std::collections::HashMap;

use pchain_world_state::{NetworkAccount, Pool, Stake};

use crate::commands::protocol;

use super::test_utils::*;

// Note: The next epoch functions tested here are non-chargeable
// ctx.gas_meter is used only to prepare testing state

// Prepare: no pool in world state
// Prepare: empty pvp and vp.
// Commands (account a): Next Epoch
#[test]
fn test_next_epoch_no_pool() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    NetworkAccount::new(&mut state.ctx.gas_meter).set_current_epoch(0);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v1(Some(ws));

    let mut state = execute_next_epoch_test_v1(state);
    assert_eq!(
        NetworkAccount::new(&mut state.ctx.gas_meter).current_epoch(),
        1
    );

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    NetworkAccount::new(&mut state.ctx.gas_meter).set_current_epoch(0);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v2(Some(ws));
    let mut state = execute_next_epoch_test_v2(state);
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
    let fixture = TestFixture::new();
    let ws = {
        let mut state = create_state_v1(Some(fixture.ws()));
        setup_pool(
            &mut state, ACCOUNT_A, 10_000, ACCOUNT_B, 90_000, false, false,
        );
        state.ctx.into_ws_cache().commit_to_world_state()
    };
    let state = create_state_v1(Some(ws));
    let mut state = execute_next_epoch_test_v1(state);

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
    let mut nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
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
    let mut delegated_stakes = pool.delegated_stakes();
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

    /* Version 2 */
    let fixture = TestFixture::new();
    let ws = {
        let mut state = create_state_v2(Some(fixture.ws()));
        setup_pool(
            &mut state, ACCOUNT_A, 10_000, ACCOUNT_B, 90_000, false, false,
        );
        state.ctx.into_ws_cache().commit_to_world_state()
    };
    let state = create_state_v2(Some(ws));
    let mut state = execute_next_epoch_test_v2(state);

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
    let mut nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
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
    let mut delegated_stakes = pool.delegated_stakes();
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
    let fixture = TestFixture::new();
    let ws = {
        let mut state = create_state_v1(Some(fixture.ws()));
        setup_pool(
            &mut state, ACCOUNT_A, 10_000, ACCOUNT_B, 90_000, false, false,
        );
        state.ctx.into_ws_cache().commit_to_world_state()
    };
    let mut state = create_state_v1(Some(ws));
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // prepare data by executing first epoch, assume test result is correct from test_next_epoch_single_pool
    let mut state = execute_next_epoch_test_v1(state);
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // second epoch
    state.tx.nonce = 1;
    let mut state = execute_next_epoch_test_v1(state);

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
    let mut nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
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

    /* Version 2 */
    let fixture = TestFixture::new();
    let ws = {
        let mut state = create_state_v2(Some(fixture.ws()));
        setup_pool(
            &mut state, ACCOUNT_A, 10_000, ACCOUNT_B, 90_000, false, false,
        );
        state.ctx.into_ws_cache().commit_to_world_state()
    };
    let mut state = create_state_v2(Some(ws));
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // prepare data by executing first epoch, assume test result is correct from test_next_epoch_single_pool
    let mut state = execute_next_epoch_test_v2(state);
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // second epoch
    state.tx.nonce = 1;
    let mut state = execute_next_epoch_test_v2(state);

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
    let mut nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
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
    let fixture = TestFixture::new();
    let ws = {
        let mut state = create_state_v1(Some(fixture.ws()));
        setup_pool(&mut state, ACCOUNT_A, 10_000, ACCOUNT_B, 90_000, true, true);
        state.ctx.into_ws_cache().commit_to_world_state()
    };
    let mut state = create_state_v1(Some(ws));
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // prepare data by executing first epoch, assume test result is correct from test_next_epoch_single_pool
    let mut state = execute_next_epoch_test_v1(state);
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // second epoch
    state.tx.nonce = 1;
    let mut state = execute_next_epoch_test_v1(state);

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
    let mut nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
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

    /* Version 2 */
    let fixture = TestFixture::new();
    let ws = {
        let mut state = create_state_v2(Some(fixture.ws()));
        setup_pool(&mut state, ACCOUNT_A, 10_000, ACCOUNT_B, 90_000, true, true);
        state.ctx.into_ws_cache().commit_to_world_state()
    };
    let mut state = create_state_v2(Some(ws));
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // prepare data by executing first epoch, assume test result is correct from test_next_epoch_single_pool
    let mut state = execute_next_epoch_test_v2(state);
    state.bd.validator_performance = Some(single_node_performance(ACCOUNT_A, 1));
    // second epoch
    state.tx.nonce = 1;
    let mut state = execute_next_epoch_test_v2(state);

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
    let mut nvp = NetworkAccount::nvp(&mut state.ctx.gas_meter);
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));

    prepare_accounts_balance(&mut state.ctx.inner_ws_cache_mut().ws);

    create_full_nvp_pool_stakes_deposits(&mut state, false, false, false);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));

    // First Epoch
    state.bd.validator_performance = Some(all_nodes_performance());
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v1(state);
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
        let mut network_acct_state = protocol::NetworkAccountWorldState::new(&mut state);

        // Pool power of vp and nvp are equal
        let l = NetworkAccount::vp(&mut network_acct_state).length();
        for i in 0..l {
            let vp: Pool = NetworkAccount::vp(&mut network_acct_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_acct_state)
                .get_by(&vp.operator)
                .unwrap();
            assert_eq!(vp.power, nvp.power);
        }

        // Stakes in VP and Deposits are not rewarded
        let mut pool_operator_stakes = HashMap::new();
        for i in 0..l {
            let mut vp_dict = NetworkAccount::vp(&mut network_acct_state);
            let mut vp = vp_dict.pool_at(i).unwrap();
            let vp_operator = vp.operator().unwrap();
            let vp_power = vp.power().unwrap();
            let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
            let mut sum = 0;
            for j in 0..TEST_MAX_STAKES_PER_POOL {
                let (address, power) = init_setup_stake_of_owner(j);
                let stake = NetworkAccount::vp(&mut network_acct_state)
                    .pool(vp_operator)
                    .unwrap()
                    .delegated_stakes()
                    .get_by(&address)
                    .unwrap();
                assert_eq!(stake.power, power);
                sum += stake.power;
                let deposit =
                    NetworkAccount::deposits(&mut network_acct_state, vp_operator, address)
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
            assert!(
                NetworkAccount::deposits(&mut network_acct_state, operator, operator)
                    .balance()
                    .is_none()
            );
        }
    }

    // Second Epoch
    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.bd.validator_performance = Some(all_nodes_performance());
    state.tx.nonce = 1;
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v1(state);
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
        let mut network_account_state = protocol::NetworkAccountWorldState::new(&mut state);

        // Pool power of pvp, vp and nvp are equal
        let l = NetworkAccount::vp(&mut network_account_state).length();
        for i in 0..l {
            let pvp: Pool = NetworkAccount::pvp(&mut network_account_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_account_state)
                .get_by(&pvp.operator)
                .unwrap();
            assert_eq!(pvp.power, nvp.power);

            let vp: Pool = NetworkAccount::vp(&mut network_account_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_account_state)
                .get_by(&vp.operator)
                .unwrap();
            assert_eq!(vp.power, nvp.power);
        }

        // Stakes are not rewarded, Desposits are rewarded
        let mut pool_operator_stakes = HashMap::new();
        for i in 0..l {
            let mut vp_dict = NetworkAccount::vp(&mut network_account_state);
            let mut vp = vp_dict.pool_at(i).unwrap();
            let vp_operator = vp.operator().unwrap();
            let vp_power = vp.power().unwrap();
            let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
            let mut sum = 0;
            for j in 0..TEST_MAX_STAKES_PER_POOL {
                let (address, power) = init_setup_stake_of_owner(j);
                let stake = NetworkAccount::vp(&mut network_account_state)
                    .pool(vp_operator)
                    .unwrap()
                    .delegated_stakes()
                    .get_by(&address)
                    .unwrap();
                sum += stake.power;
                assert_eq!(stake.power, power);
                let deposit =
                    NetworkAccount::deposits(&mut network_account_state, vp_operator, address)
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
                NetworkAccount::deposits(&mut network_account_state, operator, operator).balance()
                    > Some(0)
            );
        }
    }

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));

    prepare_accounts_balance(&mut state.ctx.inner_ws_cache_mut().ws);

    create_full_nvp_pool_stakes_deposits(&mut state, false, false, false);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v2(Some(ws));

    // First Epoch
    state.bd.validator_performance = Some(all_nodes_performance());
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v2(state);
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
        let mut network_acct_state = protocol::NetworkAccountWorldState::new(&mut state);

        // Pool power of vp and nvp are equal
        let l = NetworkAccount::vp(&mut network_acct_state).length();
        for i in 0..l {
            let vp: Pool = NetworkAccount::vp(&mut network_acct_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_acct_state)
                .get_by(&vp.operator)
                .unwrap();
            assert_eq!(vp.power, nvp.power);
        }

        // Stakes in VP and Deposits are not rewarded
        let mut pool_operator_stakes = HashMap::new();
        for i in 0..l {
            let mut vp_dict = NetworkAccount::vp(&mut network_acct_state);
            let mut vp = vp_dict.pool_at(i).unwrap();
            let vp_operator = vp.operator().unwrap();
            let vp_power = vp.power().unwrap();
            let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
            let mut sum = 0;
            for j in 0..TEST_MAX_STAKES_PER_POOL {
                let (address, power) = init_setup_stake_of_owner(j);
                let stake = NetworkAccount::vp(&mut network_acct_state)
                    .pool(vp_operator)
                    .unwrap()
                    .delegated_stakes()
                    .get_by(&address)
                    .unwrap();
                assert_eq!(stake.power, power);
                sum += stake.power;
                let deposit =
                    NetworkAccount::deposits(&mut network_acct_state, vp_operator, address)
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
            assert!(
                NetworkAccount::deposits(&mut network_acct_state, operator, operator)
                    .balance()
                    .is_none()
            );
        }
    }

    // Second Epoch
    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.bd.validator_performance = Some(all_nodes_performance());
    state.tx.nonce = 1;
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v2(state);
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
        let mut network_account_state = protocol::NetworkAccountWorldState::new(&mut state);

        // Pool power of pvp, vp and nvp are equal
        let l = NetworkAccount::vp(&mut network_account_state).length();
        for i in 0..l {
            let pvp: Pool = NetworkAccount::pvp(&mut network_account_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_account_state)
                .get_by(&pvp.operator)
                .unwrap();
            assert_eq!(pvp.power, nvp.power);

            let vp: Pool = NetworkAccount::vp(&mut network_account_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_account_state)
                .get_by(&vp.operator)
                .unwrap();
            assert_eq!(vp.power, nvp.power);
        }

        // Stakes are not rewarded, Desposits are rewarded
        let mut pool_operator_stakes = HashMap::new();
        for i in 0..l {
            let mut vp_dict = NetworkAccount::vp(&mut network_account_state);
            let mut vp = vp_dict.pool_at(i).unwrap();
            let vp_operator = vp.operator().unwrap();
            let vp_power = vp.power().unwrap();
            let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
            let mut sum = 0;
            for j in 0..TEST_MAX_STAKES_PER_POOL {
                let (address, power) = init_setup_stake_of_owner(j);
                let stake = NetworkAccount::vp(&mut network_account_state)
                    .pool(vp_operator)
                    .unwrap()
                    .delegated_stakes()
                    .get_by(&address)
                    .unwrap();
                sum += stake.power;
                assert_eq!(stake.power, power);
                let deposit =
                    NetworkAccount::deposits(&mut network_account_state, vp_operator, address)
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
                NetworkAccount::deposits(&mut network_account_state, operator, operator).balance()
                    > Some(0)
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));

    prepare_accounts_balance(&mut state.ctx.inner_ws_cache_mut().ws);

    create_full_nvp_pool_stakes_deposits(&mut state, true, true, true);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));

    // First Epoch
    state.bd.validator_performance = Some(all_nodes_performance());
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v1(state);
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
        let mut network_acct_state = protocol::NetworkAccountWorldState::new(&mut state);

        // Pool power of vp and nvp are equal
        let l = NetworkAccount::vp(&mut network_acct_state).length();
        for i in 0..l {
            let vp: Pool = NetworkAccount::vp(&mut network_acct_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_acct_state)
                .get_by(&vp.operator)
                .unwrap();
            assert_eq!(vp.power, nvp.power);
        }

        // Stakes in VP and Deposits are not rewarded
        let mut pool_operator_stakes = HashMap::new();
        for i in 0..l {
            let mut vp_dict = NetworkAccount::vp(&mut network_acct_state);
            let mut vp = vp_dict.pool_at(i).unwrap();
            let vp_operator = vp.operator().unwrap();
            let vp_power = vp.power().unwrap();
            let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
            let mut sum = 0;
            for j in 0..TEST_MAX_STAKES_PER_POOL {
                let (address, power) = init_setup_stake_of_owner(j);
                let stake = NetworkAccount::vp(&mut network_acct_state)
                    .pool(vp_operator)
                    .unwrap()
                    .delegated_stakes()
                    .get_by(&address)
                    .unwrap();
                assert_eq!(stake.power, power);
                sum += stake.power;
                let deposit =
                    NetworkAccount::deposits(&mut network_acct_state, vp_operator, address)
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
                NetworkAccount::deposits(&mut network_acct_state, operator, operator).balance(),
                Some(power)
            );
        }
    }

    // Second Epoch
    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.bd.validator_performance = Some(all_nodes_performance());
    state.tx.nonce = 1;
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v1(state);
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
        let mut state = protocol::NetworkAccountWorldState::new(&mut state);

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
            let mut vp = vp_dict.pool_at(i).unwrap();
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

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));

    prepare_accounts_balance(&mut state.ctx.inner_ws_cache_mut().ws);

    create_full_nvp_pool_stakes_deposits(&mut state, true, true, true);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));

    // First Epoch
    state.bd.validator_performance = Some(all_nodes_performance());
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v2(state);
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
        let mut network_acct_state = protocol::NetworkAccountWorldState::new(&mut state);

        // Pool power of vp and nvp are equal
        let l = NetworkAccount::vp(&mut network_acct_state).length();
        for i in 0..l {
            let vp: Pool = NetworkAccount::vp(&mut network_acct_state)
                .pool_at(i)
                .unwrap()
                .try_into()
                .unwrap();
            let nvp = NetworkAccount::nvp(&mut network_acct_state)
                .get_by(&vp.operator)
                .unwrap();
            assert_eq!(vp.power, nvp.power);
        }

        // Stakes in VP and Deposits are not rewarded
        let mut pool_operator_stakes = HashMap::new();
        for i in 0..l {
            let mut vp_dict = NetworkAccount::vp(&mut network_acct_state);
            let mut vp = vp_dict.pool_at(i).unwrap();
            let vp_operator = vp.operator().unwrap();
            let vp_power = vp.power().unwrap();
            let vp_operator_stake_power = vp.operator_stake().unwrap().unwrap().power;
            let mut sum = 0;
            for j in 0..TEST_MAX_STAKES_PER_POOL {
                let (address, power) = init_setup_stake_of_owner(j);
                let stake = NetworkAccount::vp(&mut network_acct_state)
                    .pool(vp_operator)
                    .unwrap()
                    .delegated_stakes()
                    .get_by(&address)
                    .unwrap();
                assert_eq!(stake.power, power);
                sum += stake.power;
                let deposit =
                    NetworkAccount::deposits(&mut network_acct_state, vp_operator, address)
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
                NetworkAccount::deposits(&mut network_acct_state, operator, operator).balance(),
                Some(power)
            );
        }
    }

    // Second Epoch
    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.bd.validator_performance = Some(all_nodes_performance());
    state.tx.nonce = 1;
    let t = std::time::Instant::now();
    let mut state = execute_next_epoch_test_v2(state);
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
        let mut state = protocol::NetworkAccountWorldState::new(&mut state);

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
            let mut vp = vp_dict.pool_at(i).unwrap();
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
