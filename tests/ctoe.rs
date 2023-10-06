use pchain_types::{blockchain::{ExitCodeV1, TransactionV1}, cryptography::contract_address_v1};

use crate::common::{
    gas::extract_gas_used, ArgsBuilder, SimulateWorldState, TestData,
};

mod common;

/// Simulate test to call smart contract with sufficient balance in from account.
/// 1. transfer initial balance to contract at etoc call
/// 2. make ctoe call to transfer value <= contract's balance to contract itself
/// 3. make ctoe call to transfer value <= contract's balance to another address
#[test]
fn test_success_ctoe() {
    let contract_code = TestData::get_test_contract_code("all_features");
    let origin_address = [1u8; 32];
    let contract_address = contract_address_v1(&origin_address, 0);

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.gas_limit = 300_000_000;
    tx.commands = vec![ArgsBuilder::new()
        .empty_args()
        .make_deploy(contract_code, 0)];

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState = result.new_state.into();

    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 100_000_000;

    // make transfer balance to contract itself
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        TransactionV1 {
            commands: vec![ArgsBuilder::new()
                .add(contract_address.clone())
                .add(100_000_u64)
                .make_call(Some(100_000_u64), contract_address, "make_balance_transfer")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2281166);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // check contract balance is unchanged.
    let contract_balance = sws.get_balance(contract_address);
    assert_eq!(contract_balance, 100_000_u64);

    // make transfer balance to another address itself
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        TransactionV1 {
            commands: vec![ArgsBuilder::new()
                .add([9u8; 32])
                .add(100_000_u64)
                .make_call(Some(0), contract_address, "make_balance_transfer")],
            nonce: 2,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2281166);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // check contract balance is empty.
    let contract_balance = sws.get_balance(contract_address);
    assert_eq!(contract_balance, 0_u64);
}

/// Simulate test to call smart contract with sufficient balance in from account.
/// Verify the transaction status code is FailureBalanceInsufficientInContract
/// 1. transfer initial balance to contract at etoc call
/// 2. make ctoe call to transfer value > contract's balance to contract itself
#[test]
fn test_ctoe_tx_with_insufficient_balance() {
    let contract_code = TestData::get_test_contract_code("all_features");
    let origin_address = [1u8; 32];
    let contract_address = contract_address_v1(&origin_address, 0);

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let tx = TransactionV1 {
        signer: origin_address,
        commands: vec![ArgsBuilder::new()
            .empty_args()
            .make_deploy(contract_code, 0)],
        gas_limit: 300_000_000,
        priority_fee_per_gas: 0,
        max_base_fee_per_gas: 1,
        nonce: 0,
        hash: [0u8; 32],
        signature: [0u8; 64],
    };

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 220290230);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState = result.new_state.into();

    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 100_000_000;

    // make transfer balance
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        TransactionV1 {
            commands: vec![ArgsBuilder::new()
                .add(contract_address.clone())
                .add(100_000_000_u64)
                .make_call(Some(0), contract_address, "make_balance_transfer")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2246021);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Failed);
}
