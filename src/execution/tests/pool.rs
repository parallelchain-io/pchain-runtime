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
    state.txn_meta.nonce = 1;
    let ret = execute_commands_v1(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    assert_eq!(ret.error, Some(TransitionError::PoolAlreadyExists));
    assert_eq!(extract_gas_used(&ret), 1980);

    let mut state = create_state_v1(Some(ret.new_state));
    state.txn_meta.nonce = 2;
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
    state.txn_meta.signer = ACCOUNT_B;
    let ret = execute_commands_v1(
        state,
        vec![Command::SetPoolSettings(SetPoolSettingsInput {
            commission_rate: 3,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));

    assert_eq!(extract_gas_used(&ret), 1980);

    let mut state = create_state_v1(Some(ret.new_state));
    state.txn_meta.signer = ACCOUNT_A;
    state.txn_meta.nonce = 1;
    let ret = execute_commands_v1(
        state,
        vec![Command::SetPoolSettings(SetPoolSettingsInput {
            commission_rate: 101,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));

    assert_eq!(extract_gas_used(&ret), 0);

    let mut state = create_state_v1(Some(ret.new_state));
    state.txn_meta.nonce = 2;
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
    state.txn_meta.signer = ACCOUNT_B;
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
    set_tx_v1(&mut state, ACCOUNT_B, 0, &commands);
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
    state.txn_meta.nonce = 1;
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
    set_tx_v1(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v1(state, commands);
    assert_eq!(ret.error, Some(TransitionError::DepositsAlreadyExists));
    assert_eq!(extract_gas_used(&ret), 4600);

    let mut state = create_state_v1(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000_000,
        auto_stake_rewards: false,
    })];
    set_tx_v1(&mut state, ACCOUNT_C, 0, &commands);
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
    set_tx_v1(&mut state, ACCOUNT_B, 0, &commands);
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
    set_tx_v1(&mut state, ACCOUNT_B, 1, &commands);
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
    set_tx_v1(&mut state, ACCOUNT_B, 0, &commands);
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
    set_tx_v1(&mut state, ACCOUNT_C, 0, &commands);
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

    state.txn_meta.nonce = 1;

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

    state.txn_meta.nonce = 2;
    let state = execute_next_epoch_test_v1(state);

    let mut state = create_state_v1(Some(state.ctx.into_ws_cache().ws));
    state.txn_meta.signer = ACCOUNT_B;
    state.txn_meta.nonce = 0;
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

    state.txn_meta.nonce = 3;
    execute_next_epoch_test_v1(state);
}

//
//
//
//
//
// ↓↓↓ Version 2 ↓↓↓ //
//
//
//
//
//

// Commands: Create Pool
// Exception:
// - Create Pool again
// - Pool commission rate > 100
#[test]
fn test_create_pool_v2() {
    let fixture = TestFixture::new();
    let state = create_state_v2(Some(fixture.ws()));
    let ret = execute_commands_v2(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        431150,
        299090,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
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
    state.txn_meta.nonce = 1;
    let ret = execute_commands_v2(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        137730,
        5670,
        ExitCodeV2::Error,
        0
    ));
    assert_eq!(ret.error, Some(TransitionError::PoolAlreadyExists));

    let mut state = create_state_v2(Some(ret.new_state));
    state.txn_meta.nonce = 2;
    let ret = execute_commands_v2(
        state,
        vec![Command::CreatePool(CreatePoolInput {
            commission_rate: 101,
        })],
    );
    assert!(verify_receipt_content_v2(
        rcp,
        137730,
        5670,
        ExitCodeV2::Error,
        0
    ));
    assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));
}

// Commands: Create Pool, Set Pool Settings
// Exception:
// - Pool Not exist
// - Pool commission rate > 100
// - Same commission rate
#[test]
fn test_create_pool_set_policy_v2() {
    let fixture = TestFixture::new();
    let state = create_state_v2(Some(fixture.ws()));
    let ret = execute_commands_v2(
        state,
        vec![
            Command::CreatePool(CreatePoolInput { commission_rate: 1 }),
            Command::SetPoolSettings(SetPoolSettingsInput { commission_rate: 2 }),
        ],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        445230,
        313170,
        ExitCodeV2::Ok,
        0
    ));
    let mut state = create_state_v2(Some(ret.new_state));
    assert_eq!(
        NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A)
            .commission_rate()
            .unwrap(),
        2
    );

    ///// Exceptions: /////
    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.txn_meta.signer = ACCOUNT_B;
    let ret = execute_commands_v2(
        state,
        vec![Command::SetPoolSettings(SetPoolSettingsInput {
            commission_rate: 3,
        })],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        133400,
        1340,
        ExitCodeV2::Error,
        0
    ));
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));

    let mut state = create_state_v2(Some(ret.new_state));
    state.txn_meta.signer = ACCOUNT_A;
    state.txn_meta.nonce = 1;
    let ret = execute_commands_v2(
        state,
        vec![Command::SetPoolSettings(SetPoolSettingsInput {
            commission_rate: 101,
        })],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        132060,
        0,
        ExitCodeV2::Error,
        0
    ));
    assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));

    let mut state = create_state_v2(Some(ret.new_state));
    state.txn_meta.nonce = 2;
    let ret = execute_commands_v2(
        state,
        vec![Command::SetPoolSettings(SetPoolSettingsInput {
            commission_rate: 2,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::InvalidPoolPolicy));
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        134790,
        2730,
        ExitCodeV2::Error,
        0
    ));
}

// Commands: Create Pool, Delete Pool
// Exception:
// - Pool Not exist
#[test]
fn test_create_delete_pool_v2() {
    let fixture = TestFixture::new();
    let state = create_state_v2(Some(fixture.ws()));
    let ret = execute_commands_v2(
        state,
        vec![
            Command::CreatePool(CreatePoolInput { commission_rate: 1 }),
            Command::DeletePool,
        ],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        431150,
        299090,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
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

    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.txn_meta.signer = ACCOUNT_B;
    let ret = execute_commands_v2(state, vec![Command::DeletePool]);
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        133400,
        1340,
        ExitCodeV2::Error,
        0
    ));
}

// Command 1 (account a): Create Pool
// Command 2 (account b): Create Deposit
// Exception:
// - Pool Not exist
// - Deposit already exists
// - Not enough balance
#[test]
fn test_create_pool_create_deposit_v2() {
    let fixture = TestFixture::new();
    let state = create_state_v2(Some(fixture.ws()));
    let ret = execute_commands_v2(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        431150,
        299090,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000,
        auto_stake_rewards: false,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        205520,
        71930,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
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

    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.txn_meta.nonce = 1;
    let ret = execute_commands_v2(
        state,
        vec![Command::CreateDeposit(CreateDepositInput {
            operator: ACCOUNT_B,
            balance: 500_000,
            auto_stake_rewards: false,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::PoolNotExists));
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        133400,
        1340,
        ExitCodeV2::Error,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000,
        auto_stake_rewards: false,
    })];
    set_tx_v2(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v2(state, commands);
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        136910,
        3320,
        ExitCodeV2::Error,
        0
    ));
    assert_eq!(ret.error, Some(TransitionError::DepositsAlreadyExists));

    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000_000,
        auto_stake_rewards: false,
    })];
    set_tx_v2(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        137970,
        4380,
        ExitCodeV2::Error,
        0
    ));
    assert_eq!(
        ret.error,
        Some(TransitionError::NotEnoughBalanceForTransfer)
    );
}

// Prepare: pool (account a) in world state
// Commands (account b): Create Deposit, Set Deposit Settings
// Exception:
// - Deposit not exist
// - same deposit policy
#[test]
fn test_create_deposit_set_policy_v2() {
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v2(Some(ws));
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
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        226970,
        71930 + 20160,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
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

    let state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    let ret = execute_commands_v2(
        state,
        vec![Command::SetDepositSettings(SetDepositSettingsInput {
            operator: ACCOUNT_B,
            auto_stake_rewards: true,
        })],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        134040,
        1980,
        ExitCodeV2::Error,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![
        Command::SetDepositSettings(SetDepositSettingsInput {
            operator: ACCOUNT_A,
            auto_stake_rewards: true,
        }), // Same deposit plocy
    ];
    set_tx_v2(&mut state, ACCOUNT_B, 1, &commands);
    let ret = execute_commands_v2(state, commands);
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        137360,
        4010,
        ExitCodeV2::Error,
        0
    ));
    assert_eq!(ret.error, Some(TransitionError::InvalidDepositPolicy));
}

// Prepare: pool (account a) in world state
// Commands (account b): Create Deposit, Topup Deposit
// Exception:
// - Deposit not exist
// - Not enough balance
#[test]
fn test_create_deposit_topupdeposit_v2() {
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, ACCOUNT_A);
    pool.set_operator(ACCOUNT_A);
    pool.set_power(100_000);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();

    let mut state = create_state_v2(Some(ws));
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
    set_tx_v2(&mut state, ACCOUNT_B, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        253_040,
        71_930 + 46_020,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
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
    let state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    let ret = execute_commands_v2(
        state,
        vec![Command::TopUpDeposit(TopUpDepositInput {
            operator: ACCOUNT_A,
            amount: 100,
        })],
    );
    assert_eq!(ret.error, Some(TransitionError::DepositsNotExists));
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        134_040,
        1980,
        ExitCodeV2::Error,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    let commands = vec![Command::CreateDeposit(CreateDepositInput {
        operator: ACCOUNT_A,
        balance: 500_000_000,
        auto_stake_rewards: false,
    })];
    set_tx_v2(&mut state, ACCOUNT_C, 0, &commands);
    let ret = execute_commands_v2(state, commands);
    assert_eq!(
        ret.error,
        Some(TransitionError::NotEnoughBalanceForTransfer)
    );

    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        137_970,
        4380,
        ExitCodeV2::Error,
        0
    ));
}

// Prepare: add max. number of pools in world state, included in nvp.
// Prepare: empty pvp and vp.
// Commands: Next Epoch, Delete Pool (account a), Next Epoch, Create Pool (account b), Next Epoch
#[test]
fn test_update_pool_epoch_change_validator_v2() {
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    create_full_pools_in_nvp(&mut state, false, false);
    let ws = state.ctx.into_ws_cache().commit_to_world_state();
    let state = create_state_v2(Some(ws));
    let mut state = execute_next_epoch_test_v2(state);

    state.txn_meta.nonce = 1;

    let ret = execute_commands_v2(state, vec![Command::DeletePool]);
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        560990,
        428930,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));
    state.txn_meta.nonce = 2;
    let state = execute_next_epoch_test_v2(state);

    let mut state = create_state_v2(Some(state.ctx.into_ws_cache().ws));
    state.txn_meta.signer = ACCOUNT_B;
    state.txn_meta.nonce = 0;
    let ret = execute_commands_v2(
        state,
        vec![Command::CreatePool(CreatePoolInput { commission_rate: 1 })],
    );
    let rcp = ret.receipt.as_ref().expect("Receipt expected");
    assert!(verify_receipt_content_v2(
        rcp,
        1405090,
        1273030,
        ExitCodeV2::Ok,
        0
    ));

    let mut state = create_state_v2(Some(ret.new_state));

    state.txn_meta.nonce = 3;
    execute_next_epoch_test_v2(state);
}
