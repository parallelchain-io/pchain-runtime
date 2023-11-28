/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
use pchain_types::{
    blockchain::{Command, ExitCodeV1, ExitCodeV2},
    runtime::{
        CreateDepositInput, CreatePoolInput, SetDepositSettingsInput, SetPoolSettingsInput,
        TopUpDepositInput,
    },
};
use pchain_world_state::NetworkAccount;

use crate::{
    execution::execute_commands::{execute_commands_v1, execute_commands_v2},
    TransitionError,
};

use super::test_utils::*;

// Commands: Create Pool
// Exception:
// - Create Pool again
// - Pool commission rate > 100
#[test]
fn test_create_pool() {
    let fixture = TestFixture::new();
    let state = create_state_v1(Some(fixture.ws()));
    let ret = execute_commands_v1(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 334610);

    let mut state = create_state_v1(Some(ret.new_state));
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

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.tx.nonce = 1;
    let ret = execute_commands_v1(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    assert_eq!(ret.error, Some(TransitionError::PoolAlreadyExists));
    assert_eq!(extract_gas_used(&ret), 1980);

    let mut state = create_state_v1(Some(ret.new_state));
    state.tx.nonce = 2;
    let ret = execute_commands_v1(
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
    let fixture = TestFixture::new();
    let state = create_state_v1(Some(fixture.ws()));
    let ret = execute_commands_v1(
        state,
        vec![
            Command::CreatePool(CreatePoolInput { commission_rate: 1 }),
            Command::SetPoolSettings(SetPoolSettingsInput { commission_rate: 2 }),
        ],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );

    assert_eq!(extract_gas_used(&ret), 354770);

    let mut state = create_state_v1(Some(ret.new_state));
    assert_eq!(
        NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .commission_rate()
            .unwrap(),
        2
    );

    ///// Exceptions: /////

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.tx.signer = ACCOUNT_B;
    let ret = execute_commands_v1(
        state,
        vec![Command::SetPoolSettings(SetPoolSettingsInput {
            commission_rate: 3,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));

    assert_eq!(extract_gas_used(&ret), 1980);

    let mut state = create_state_v1(Some(ret.new_state));
    state.tx.signer = ACCOUNT_A;
    state.tx.nonce = 1;
    let ret = execute_commands_v1(
        state,
        vec![Command::SetPoolSettings(SetPoolSettingsInput {
            commission_rate: 101,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));

    assert_eq!(extract_gas_used(&ret), 0);

    let mut state = create_state_v1(Some(ret.new_state));
    state.tx.nonce = 2;
    let ret = execute_commands_v1(
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
    let fixture = TestFixture::new();
    let state = create_state_v1(Some(fixture.ws()));
    let ret = execute_commands_v1(
        state,
        vec![
            Command::CreatePool(CreatePoolInput { commission_rate: 1 }),
            Command::DeletePool,
        ],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 334610);
    let mut state = create_state_v1(Some(ret.new_state));
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

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.tx.signer = ACCOUNT_B;
    let ret = execute_commands_v1(state, vec![Command::DeletePool]);
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
    let fixture = TestFixture::new();
    let state = create_state_v1(Some(fixture.ws()));
    let ret = execute_commands_v1(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );

    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000,
        auto_stake_rewards: false,
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
    assert_eq!(extract_gas_used(&ret), 82810);

    let mut state = create_state_v1(Some(ret.new_state));
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

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.tx.nonce = 1;
    let ret = execute_commands_v1(
        state,
        vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_B,
            balance: 500_000,
            auto_stake_rewards: false,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
    assert_eq!(extract_gas_used(&ret), 1980);

    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000,
        auto_stake_rewards: false,
    })];
    set_tx(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::DepositsAlreadyExists));
    assert_eq!(extract_gas_used(&ret), 4600);

    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000_000,
        auto_stake_rewards: false,
    })];
    set_tx(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v1(state, commands);
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
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
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 109050);

    let mut state = create_state_v1(Some(ret.new_state));
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

    let state = create_state_v1(Some(state.ctx.into_ws_cache().ws));

    let ret = execute_commands_v1(
        state,
        vec![Command::SetDepositSettings(SetDepositSettingsInput {
            operator: ACCOUNT_B,
            auto_stake_rewards: true,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    assert_eq!(extract_gas_used(&ret), 2620);

    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![
        Command::SetDepositSettings(SetDepositSettingsInput {
            operator: ACCOUNT_A,
            auto_stake_rewards: true,
        }), // Same deposit plocy
    ];
    set_tx(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v1(state, commands);
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
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v1(Some(ws));
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
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 134910);

    let mut state = create_state_v1(Some(ret.new_state));
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
    let state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    let ret = execute_commands_v1(
        state,
        vec![Command::TopUpDeposit(TopUpDepositInput {
            operator: ACCOUNT_A,
            amount: 100,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    assert_eq!(extract_gas_used(&ret), 2620);

    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000_000,
        auto_stake_rewards: false,
    })];
    set_tx(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(
        ret.error,
        Some(TransitionError::NotEnoughBalanceForTransfer)
    );
    assert_eq!(extract_gas_used(&ret), 5660);
}

// Prepare: add max. number of pools in world state, included in nvp.
// Prepare: empty pvp and vp.
// Commands: Next Epoch, Delete Pool (account a), Next Epoch, Create Pool (account b), Next Epoch
#[test]
fn test_update_pool_epoch_change_validator() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v1(Some(ws));
    let mut state = execute_next_epoch_test_v1(state);

    state.tx.nonce = 1;

    let ret = execute_commands_v1(state, vec![Command::DeletePool]);
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 357250);

    let mut state = create_state_v1(Some(ret.new_state));

    state.tx.nonce = 2;
    let state = execute_next_epoch_test_v1(state);

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.tx.signer = ACCOUNT_B;
    state.tx.nonce = 0;
    let ret = execute_commands_v1(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );
    assert_eq!(extract_gas_used(&ret), 1432070);
    let mut state = create_state_v1(Some(ret.new_state));

    state.tx.nonce = 3;
    execute_next_epoch_test_v1(state);

    /* Version 2 */
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v2(Some(ws));
    let mut state = execute_next_epoch_test_v2(state);

    state.tx.nonce = 1;

    let ret = execute_commands_v2(state, vec![Command::DeletePool]);
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        579012,
        446952,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    state.tx.nonce = 2;
    let state = execute_next_epoch_test_v2(state);

    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.tx.signer = ACCOUNT_B;
    state.tx.nonce = 0;
    let ret = execute_commands_v2(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        1409742,
        1277682,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));

    state.tx.nonce = 3;
    execute_next_epoch_test_v2(state);
}
