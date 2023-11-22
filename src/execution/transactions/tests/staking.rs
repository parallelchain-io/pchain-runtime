/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::{
    blockchain::{Command, CommandReceiptV2, ExitCodeV1, ExitCodeV2},
    runtime::{
        CreateDepositInput, CreatePoolInput, StakeDepositInput, UnstakeDepositInput,
        WithdrawDepositInput,
    },
};
use pchain_world_state::{NetworkAccount, Pool, Stake, StakeValue};

use crate::{
    execution::{
        execute_commands::{execute_commands_v1, execute_commands_v2},
        execute_next_epoch::execute_next_epoch_v1,
    },
    TransitionError,
};

use super::test_utils::*;

// Prepare: pool (account a) in world state
// Prepare: deposits (account b) to pool (account a)
// Commands (account b): Stake Deposit
// Exception:
// - Deposit not exist
// - Reach limit (Deposit amount)
// - Pool not exist
#[test]
fn test_stake_deposit_delegated_stakes() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
    deposit.set_balance(20_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let commands = vec![
        Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 20_000 + 1,
        }), // stake more than deposit
    ];
    let stake_deposit_inclusion_cost_v1 = set_tx(&mut state, ACCOUNT_B, 0, &commands);
    assert_eq!(stake_deposit_inclusion_cost_v1, 133530);

    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        20_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 382740);

    let mut state = create_state_v1(Some(ret.new_state));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 120_000);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&ACCOUNT_B).unwrap();
    assert_eq!(delegated_stake.power, 20_000);

    ///// Exceptions: /////
    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));

    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 20_000,
    })];
    set_tx(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    assert_eq!(extract_gas_used(&ret), 2620);

    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 1,
    })];
    set_tx(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
    assert_eq!(extract_gas_used(&ret), 16920);

    // Delete Pool first
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::DeletePool];
    set_tx(&mut state, ACCOUNT_A, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, None);
    assert_eq!(extract_gas_used(&ret), 0);

    // and then stake deposit
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 20_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 2, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
    assert_eq!(extract_gas_used(&ret), 7620);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
    deposit.set_balance(20_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![
        Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 20_000 + 1,
        }), // stake more than deposit
    ];
    let stake_deposit_inclusion_cost_v2 = set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    assert_eq!(stake_deposit_inclusion_cost_v2, 133800);

    let ret = execute_commands_v2(state, commands);
    assert!(ret.error.is_none());

    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 20_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        stake_deposit_inclusion_cost_v2 + 342740,
        342740,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 120_000);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&ACCOUNT_B).unwrap();
    assert_eq!(delegated_stake.power, 20_000);

    ///// Exceptions: /////
    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));

    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 20_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v2(state, commands);

    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        135780,
        1980,
        ExitCodeV2::Error,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 1,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v2(state, commands);
    assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        146880,
        13080,
        ExitCodeV2::Error,
        0
    ));

    // // Delete Pool first
    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::DeletePool];
    set_tx_v2(&mut state, ACCOUNT_A, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert_eq!(ret.error, None);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        132360,
        0,
        ExitCodeV2::Ok,
        0
    ));

    // and then stake deposit
    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 20_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 2, &commands);
    let ret = execute_commands_v2(state, commands);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        139500,
        5700,
        ExitCodeV2::Error,
        0
    ));

    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
}

// // Prepare: set maximum number of pools in world state, pool (account a) has the minimum power.
// // Prepare: deposits (account b) to pool (account a)
// // Commands (account b): Stake Deposit (to increase the power of pool (account a))
#[test]
fn test_stake_deposit_delegated_stakes_nvp_change_key() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 100_000);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
    deposit.set_balance(6_300_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 6_300_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        6_300_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 1308410);

    let mut state = create_state_v1(Some(ret.new_state));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
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
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1
        ]
    );
    assert_eq!(
        NetworkAccount::nvp(&mut state.ctx.gas_meter)
            .get(0)
            .unwrap()
            .power,
        200_000
    );

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 100_000);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_B);
    deposit.set_balance(6_300_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 6_300_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);

    assert_eq!(ret.error, None);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        1289250,
        1155450,
        ExitCodeV2::Ok,
        0
    ));

    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 6_300_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
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
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_B);
    pool.set_operator(ACCOUNT_B);
    pool.set_commission_rate(1);
    pool.set_power(0);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_B, ACCOUNT_C);
    deposit.set_balance(6_500_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_B,
        max_amount: 6_500_000,
    })];
    set_tx(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        6_500_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 1247750);
    let mut state = create_state_v1(Some(ret.new_state));

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
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1
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

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_B);
    pool.set_operator(ACCOUNT_B);
    pool.set_commission_rate(1);
    pool.set_power(0);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_B, ACCOUNT_C);
    deposit.set_balance(6_500_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_B,
        max_amount: 6_500_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v2(state, commands);

    assert_eq!(ret.error, None);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        1277870,
        1144070,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 6_500_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));
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
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    create_full_stakes_in_pool(&mut state, ACCOUNT_A);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_C);
    deposit.set_balance(250_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let prev_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
        .power()
        .unwrap();
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 250_000,
    })];
    set_tx(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        250_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 2811240);

    let mut state = create_state_v1(Some(ret.new_state));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    let cur_pool_power = pool.power().unwrap();
    assert_eq!(cur_pool_power, prev_pool_power + 50_000);
    let mut delegated_stakes = pool.delegated_stakes();
    assert_eq!(delegated_stakes.get(0).unwrap().power, 250_000);
    assert_eq!(delegated_stakes.get(0).unwrap().owner, ACCOUNT_C);

    ///// Exceptions: /////

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    // create deposit first (too low to join deledated stake )
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 100_000,
        auto_stake_rewards: false,
    })];
    set_tx(&mut state, ACCOUNT_D, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 82810);
    // and then stake deposit
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 100_000,
    })];
    set_tx(&mut state, ACCOUNT_D, 1, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
    assert_eq!(extract_gas_used(&ret), 18920);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_stakes_in_pool(&mut state, ACCOUNT_A);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_C);
    deposit.set_balance(250_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let prev_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
        .power()
        .unwrap();
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 250_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v2(state, commands);

    assert_eq!(ret.error, None);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        2675600,
        2541800,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 250_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    let cur_pool_power = pool.power().unwrap();
    assert_eq!(cur_pool_power, prev_pool_power + 50_000);
    let mut delegated_stakes = pool.delegated_stakes();
    assert_eq!(delegated_stakes.get(0).unwrap().power, 250_000);
    assert_eq!(delegated_stakes.get(0).unwrap().owner, ACCOUNT_C);

    ///// Exceptions: /////

    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    // create deposit first (too low to join delegated stake )
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 100_000,
        auto_stake_rewards: false,
    })];
    set_tx_v2(&mut state, ACCOUNT_D, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert_eq!(ret.error, None);
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        205520,
        71930,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 100_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_D, 1, &commands);
    let ret = execute_commands_v2(state, commands);
    assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        148240,
        14440,
        ExitCodeV2::Error,
        0
    ));
}

// Prepare: pool (account c), with maximum number of stakes in world state, stakes (account b) is the minimum value.
// Prepare: deposits (account b) to pool (account c)
// Commands (account b): Stake Deposit (to be included in delegated stakes, but not the minimum one)
#[test]
fn test_stake_deposit_delegated_stakes_change_key() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    create_full_stakes_in_pool(&mut state, ACCOUNT_C);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_C, ACCOUNT_B);
    deposit.set_balance(310_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let prev_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C)
        .power()
        .unwrap();
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 110_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        110_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 542720);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C);
    let cur_pool_power = pool.power().unwrap();
    assert_eq!(cur_pool_power, prev_pool_power + 110_000);
    let min_stake = pool.delegated_stakes().get(0).unwrap();
    assert_eq!(min_stake.power, 300_000);
    assert_eq!(
        min_stake.owner,
        [
            3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2
        ]
    );

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_stakes_in_pool(&mut state, ACCOUNT_C);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_C, ACCOUNT_B);
    deposit.set_balance(310_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let prev_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C)
        .power()
        .unwrap();
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 110_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        613800,
        480000,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 110_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C);
    let cur_pool_power = pool.power().unwrap();
    assert_eq!(cur_pool_power, prev_pool_power + 110_000);
    let min_stake = pool.delegated_stakes().get(0).unwrap();
    assert_eq!(min_stake.power, 300_000);
    assert_eq!(
        min_stake.owner,
        [
            3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2
        ]
    );
}

// Prepare: pool (account a) in world state, with delegated stakes of account b
// Prepare: deposits (account b) to pool (account a)
// Commands (account b): Stake Deposit (to increase the stake in the delegated stakes)
#[test]
fn test_stake_deposit_delegated_stakes_existing() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 40_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        40_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 314340);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 140_000);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&ACCOUNT_B).unwrap();
    assert_eq!(delegated_stake.power, 90_000);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 40_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert!(ret.error.is_none());
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        411660,
        277860,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 40_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 140_000);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&ACCOUNT_B).unwrap();
    assert_eq!(delegated_stake.power, 90_000);
}

// Prepare: pool (account a) in world state
// Prepare: deposits (account a) to pool (account a)
// Commands (account a): Stake Deposit
#[test]
fn test_stake_deposit_same_owner() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
    deposit.set_balance(150_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v1(Some(ws));
    let ret = execute_commands_v1(
        state,
        vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 20_000,
        })],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        20_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 323880);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    let operator_state = pool.operator_stake().unwrap().unwrap();
    assert_eq!(operator_state.power, 20_000);
    assert_eq!(pool.power().unwrap(), 120_000);
    let mut delegated_stakes = pool.delegated_stakes();
    assert_eq!(delegated_stakes.length(), 0);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
    deposit.set_balance(150_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v2(Some(ws));
    let ret = execute_commands_v2(
        state,
        vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 20_000,
        })],
    );
    assert!(ret.error.is_none());
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        426820,
        294760,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 20_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    let operator_state = pool.operator_stake().unwrap().unwrap();
    assert_eq!(operator_state.power, 20_000);
    assert_eq!(pool.power().unwrap(), 120_000);
    let mut delegated_stakes = pool.delegated_stakes();
    assert_eq!(delegated_stakes.length(), 0);
}

// Prepare: set maximum number of pools in world state, pool (account a) has the minimum power.
// Prepare: deposits (account a) to pool (account a)
// Commands (account a): Stake Deposit (to increase the power of pool (account a))
#[test]
fn test_stake_deposit_same_owner_nvp_change_key() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 100_000);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
    deposit.set_balance(210_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 110_000,
    })];
    set_tx(&mut state, ACCOUNT_A, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        110_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 420710);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
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
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1
        ]
    );
    assert_eq!(
        NetworkAccount::nvp(&mut state.ctx.gas_meter)
            .get(0)
            .unwrap()
            .power,
        200_000
    );

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 100_000);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_A, ACCOUNT_A);
    deposit.set_balance(210_000);
    deposit.set_auto_stake_rewards(false);

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 110_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_A, 0, &commands);
    let ret = execute_commands_v2(state, commands);

    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        503310,
        369510,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 110_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }
    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
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
            2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 150_000,
    })];
    set_tx(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        150_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 2279890);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C);
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

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::StakeDeposit(StakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 150_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        2199290,
        2065490,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 150_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_C);
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v1(Some(ws));
    let ret = execute_commands_v1(
        state,
        vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 10_000,
        })],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        10_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 277880);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    let operator_state = pool.operator_stake().unwrap().unwrap();
    assert_eq!(operator_state.power, 90_000);
    assert_eq!(pool.power().unwrap(), 110_000);
    let mut delegated_stake = pool.delegated_stakes();
    assert_eq!(delegated_stake.length(), 0);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v2(Some(ws));
    let ret = execute_commands_v2(
        state,
        vec![Command::StakeDeposit(StakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 10_000,
        })],
    );
    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        380820,
        248760,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::StakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_staked, 10_000);
    } else {
        panic!("Stake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    let operator_state = pool.operator_stake().unwrap().unwrap();
    assert_eq!(operator_state.power, 90_000);
    assert_eq!(pool.power().unwrap(), 110_000);
    let mut delegated_stake = pool.delegated_stakes();
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 40_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        40_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 311320);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 60_000);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&ACCOUNT_B).unwrap();
    assert_eq!(delegated_stake.power, 10_000);

    ///// Exceptions: /////
    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 40_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    assert_eq!(extract_gas_used(&ret), 2620);
    // create Pool and deposit first
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })];
    set_tx(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 516870);
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_C,
        balance: 10_000,
        auto_stake_rewards: false,
    })];
    set_tx(&mut state, ACCOUNT_B, 2, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 82810);
    // and then UnstakeDeposit
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 10_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 3, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::PoolHasNoStakes));
    assert_eq!(extract_gas_used(&ret), 9620);
    // delete pool first
    let state = create_state_v1(Some(ret.new_state));
    let ret = execute_commands_v1(state, vec![Command::DeletePool]);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 0);
    // then UnstakeDeposit
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 10_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 4, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
    assert_eq!(extract_gas_used(&ret), 4600);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 40_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        409280,
        275480,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::UnstakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_unstaked, 40_000);
    } else {
        panic!("Unstake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 60_000);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&ACCOUNT_B).unwrap();
    assert_eq!(delegated_stake.power, 10_000);

    ///// Exceptions: /////
    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 40_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v2(state, commands);
    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        135780,
        1980,
        ExitCodeV2::Error,
        0
    ));

    // create Pool and deposit first
    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })];
    set_tx_v2(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert_eq!(ret.error, None);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        592620,
        460230,
        ExitCodeV2::Ok,
        0
    ));
    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_C,
        balance: 10_000,
        auto_stake_rewards: false,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 2, &commands);
    let ret = execute_commands_v2(state, commands);
    println!("{:?}", ret.receipt);
    assert_eq!(ret.error, None);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        205520,
        71930,
        ExitCodeV2::Ok,
        0
    ));
    // and then UnstakeDeposit
    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_C,
        max_amount: 10_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 3, &commands);
    let ret = execute_commands_v2(state, commands);

    println!("{:?}", ret.receipt);
    assert_eq!(ret.error, Some(TransitionError::PoolHasNoStakes));
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        140860,
        7060,
        ExitCodeV2::Error,
        0
    ));

    // // delete pool first
    let state = create_state_v2(Some(ret.new_state));
    let ret = execute_commands_v2(state, vec![Command::DeletePool]);
    println!("{:?}", ret.receipt);
    assert_eq!(ret.error, None);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        132060,
        0,
        ExitCodeV2::Ok,
        0
    ));
    // then UnstakeDeposit
    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: 10_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 4, &commands);
    let ret = execute_commands_v2(state, commands);
    println!("{:?}", ret.receipt);
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        137120,
        3320,
        ExitCodeV2::Error,
        0
    ));
}

// Prepare: pool (account a) in world state, with delegated stakes of account X, X has the biggest stake
// Prepare: deposits (account X) to pool (account a)
// Commands (account X): Unstake Deposit
#[test]
fn test_unstake_deposit_delegated_stakes_remove() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    create_full_deposits_in_pool(&mut state, ACCOUNT_A, false);
    create_full_stakes_in_pool(&mut state, ACCOUNT_A);
    let biggest = [
        129u8, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
        2, 2, 2,
    ];
    state.ctx.gas_meter.ws_set_balance(biggest, 500_000_000);
    let origin_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
        .power()
        .unwrap();
    let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
        .delegated_stakes()
        .get_by(&biggest)
        .unwrap();

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: stake.power,
    })];
    set_tx(&mut state, biggest, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        stake.power.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 0);
    let mut state = create_state_v1(Some(ret.new_state));

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

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_deposits_in_pool(&mut state, ACCOUNT_A, false);
    create_full_stakes_in_pool(&mut state, ACCOUNT_A);
    let biggest = [
        129u8, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
        2, 2, 2,
    ];
    state.ctx.gas_meter.ws_set_balance(biggest, 500_000_000);
    let origin_pool_power = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
        .power()
        .unwrap();
    let stake = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
        .delegated_stakes()
        .get_by(&biggest)
        .unwrap();

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_A,
        max_amount: stake.power,
    })];
    set_tx_v2(&mut state, biggest, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        133800,
        0,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::UnstakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_unstaked, 12_900_000);
    } else {
        panic!("Unstake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));
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
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![
        Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_T,
            max_amount: 150_000 + 1,
        }), // unstake more than staked
    ];
    set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        150_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 42590);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
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

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![
        Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_T,
            max_amount: 150_000 + 1,
        }), // unstake more than staked
    ];
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);

    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        198150,
        64350,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::UnstakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_unstaked, 150_000);
    } else {
        panic!("Unstake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
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
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_T,
        max_amount: 200_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        200_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 423900);
    let mut state = create_state_v1(Some(ret.new_state));

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

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v2(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_T,
        max_amount: 200_000,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);

    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        572740,
        438940,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::UnstakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_unstaked, 200_000);
    } else {
        panic!("Unstake deposit command receipt expected");
    }
    let mut state = create_state_v2(Some(ret.new_state));

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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v1(Some(ws));
    let ret = execute_commands_v1(
        state,
        vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 100_000,
        })],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        100_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 6630);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 0);
    assert!(pool.operator_stake().unwrap().is_none());

    ///// Exceptions: /////

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.tx.nonce = 1;
    let ret = execute_commands_v1(
        state,
        vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 50_000,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::PoolHasNoStakes));
    assert_eq!(extract_gas_used(&ret), 9010);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v2(Some(ws));
    let ret = execute_commands_v2(
        state,
        vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 100_000,
        })],
    );
    assert!(ret.error.is_none());
    println!("{:?}", ret.receipt);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        132060,
        0,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::UnstakeDeposit(cr)) =
        ret.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        assert_eq!(cr.amount_unstaked, 100_000);
    } else {
        panic!("Unstake deposit command receipt expected");
    }

    let mut state = create_state_v2(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    assert_eq!(pool.power().unwrap(), 0);
    assert!(pool.operator_stake().unwrap().is_none());

    ///// Exceptions: /////

    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.tx.nonce = 1;
    let ret = execute_commands_v2(
        state,
        vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 50_000,
        })],
    );

    println!("{:?}", ret.receipt);
    assert_eq!(ret.error, Some(TransitionError::PoolHasNoStakes));
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        138510,
        6450,
        ExitCodeV2::Error,
        0
    ));
}

// TODO stop here

// Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with non-zero value of Operator Stake
// Prepare: deposits (account t) to pool (account t)
// Commands (account t): Unstake Deposit (to decrease the power of pool (account t))
#[test]
fn test_unstake_deposit_same_owner_nvp_change_key() {
    const ACCOUNT_T: [u8; 32] = [
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    state
        .ctx
        .inner_ws_cache_mut()
        .ws
        .account_trie_mut()
        .set_balance(&ACCOUNT_T, 500_000_000)
        .unwrap();
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_T,
        max_amount: 190_000,
    })];
    set_tx(&mut state, ACCOUNT_T, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        190_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 388730);
    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
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
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    state
        .ctx
        .inner_ws_cache_mut()
        .ws
        .account_trie_mut()
        .set_balance(&ACCOUNT_T, 500_000_000)
        .unwrap();
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let commands = vec![Command::UnstakeDeposit(UnstakeDepositInput {
        operator: ACCOUNT_T,
        max_amount: 200_000,
    })];
    set_tx(&mut state, ACCOUNT_T, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        200_000_u64.to_le_bytes().to_vec()
    );
    assert_eq!(extract_gas_used(&ret), 670040);

    let mut state = create_state_v1(Some(ret.new_state));

    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T);
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_A,
        max_amount: 40_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        40_000_u64.to_le_bytes().to_vec()
    );
    let gas_used = extract_gas_used(&ret);
    assert_eq!(gas_used, 362780);

    let mut state = create_state_v1(Some(ret.new_state));
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
    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();

    assert_eq!(
        owner_balance_before,
        owner_balance_after + gas_used + tx_base_cost - 40_000
    );

    ///// Exceptions: /////

    let state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    let ret = execute_commands_v1(
        state,
        vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 40_000,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    assert_eq!(extract_gas_used(&ret), 2620);

    // First proceed next epoch
    let mut state = create_state_v1(Some(ret.new_state));
    state.tx.nonce = 1;
    let ret = execute_next_epoch_v1(state, vec![Command::NextEpoch]);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 0);
    // Then unstake
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![
        Command::UnstakeDeposit(UnstakeDepositInput {
            operator: ACCOUNT_A,
            max_amount: 10_000,
        }), // 60_000 - 10_000
    ];
    set_tx(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 242150);
    // pvp: 0, vp: 60_000, nvp: 50_000, deposit: 60_000, Try withdraw
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_A,
        max_amount: 10_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 2, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
    assert_eq!(extract_gas_used(&ret), 19780);

    // Proceed next epoch
    let mut state = create_state_v1(Some(ret.new_state));
    state.tx.nonce = 2;
    state.bd.validator_performance = Some(single_node_performance(
        ACCOUNT_A,
        TEST_MAX_VALIDATOR_SET_SIZE as u32,
    ));
    let ret = execute_next_epoch_v1(state, vec![Command::NextEpoch]);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 0);
    // pvp: 60_000, vp: 50_000, nvp: 50_000, deposit: 60_013, Deduce deposit to 60_000
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![
        Command::WithdrawDeposit(WithdrawDepositInput {
            operator: ACCOUNT_A,
            max_amount: 13,
        }), // reduce deposit to 60_000
    ];
    set_tx(&mut state, ACCOUNT_B, 3, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 83580);
    // pvp: 60_000, vp: 50_000, nvp: 50_000, deposit: 60_000, Try Withdraw
    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_A,
        max_amount: 10_000,
    })];
    set_tx(&mut state, ACCOUNT_B, 4, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::InvalidStakeAmount));
    assert_eq!(extract_gas_used(&ret), 29960);
}

// Prepare: set maximum number of pools in world state, pool (account t) has power > minimum, with delegated stakes of account b
// Prepare: deposits (account b) to pool (account t)
// Commands (account b): Withdraw Deposit (to decrease the power of pool (account t))
#[test]
fn test_withdrawal_deposit_delegated_stakes_nvp_change_key() {
    const ACCOUNT_T: [u8; 32] = [
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();

    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_T,
        max_amount: 200_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
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

    let mut state = create_state_v1(Some(ret.new_state));

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
    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();
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
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_T,
        max_amount: 300_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
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

    let mut state = create_state_v1(Some(ret.new_state));

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
    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_A,
        max_amount: 45_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_A, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        45_000_u64.to_le_bytes().to_vec()
    );
    let gas_used = extract_gas_used(&ret);
    assert_eq!(gas_used, 326320);

    let mut state = create_state_v1(Some(ret.new_state));

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
    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();
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
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    state
        .ctx
        .inner_ws_cache_mut()
        .ws
        .account_trie_mut()
        .set_balance(&ACCOUNT_T, 500_000_000)
        .unwrap();
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_T)
        .unwrap();
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_T,
        max_amount: 200_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_T, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        200_000_u64.to_le_bytes().to_vec()
    );
    let gas_used = extract_gas_used(&ret);
    assert_eq!(gas_used, 11140);

    let mut state = create_state_v1(Some(ret.new_state));

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
    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_T)
        .unwrap();
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
        2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1,
    ];
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    state
        .ctx
        .inner_ws_cache_mut()
        .ws
        .account_trie_mut()
        .set_balance(&ACCOUNT_T, 500_000_000)
        .unwrap();
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_T,
        max_amount: 300_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_T, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        300_000_u64.to_le_bytes().to_vec()
    );
    let gas_used = extract_gas_used(&ret);
    assert_eq!(gas_used, 392450);

    let mut state = create_state_v1(Some(ret.new_state));

    assert_eq!(
        NetworkAccount::deposits(&mut state.ctx.gas_meter, ACCOUNT_T, ACCOUNT_T).balance(),
        None
    );
    assert!(NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_T)
        .operator_stake()
        .unwrap()
        .is_none());
    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_T)
        .unwrap();

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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();
    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_A,
        max_amount: 40_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        20_000_u64.to_le_bytes().to_vec()
    );
    let gas_used = extract_gas_used(&ret);
    assert_eq!(gas_used, 383140);

    let mut state = create_state_v1(Some(ret.new_state));

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

    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
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

    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let mut state = create_state_v1(Some(ws));
    let owner_balance_before = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();

    let commands = vec![Command::WithdrawDeposit(WithdrawDepositInput {
        operator: ACCOUNT_A,
        max_amount: 40_000,
    })];
    let tx_base_cost = set_tx(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(
        ret.receipt.as_ref().unwrap().last().unwrap().return_values,
        10_000_u64.to_le_bytes().to_vec()
    );
    let gas_used = extract_gas_used(&ret);
    assert_eq!(gas_used, 383140);

    let mut state = create_state_v1(Some(ret.new_state));

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
    let owner_balance_after = state
        .ctx
        .inner_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_B)
        .unwrap();
    assert_eq!(
        owner_balance_before,
        owner_balance_after + gas_used + tx_base_cost - 10_000
    );
}
