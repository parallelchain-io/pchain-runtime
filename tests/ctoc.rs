use borsh::{BorshDeserialize, BorshSerialize};
use pchain_types::blockchain::{ExitStatus, Transaction};

use crate::common::{
    compute_contract_address, ArgsBuilder, CallResult, SimulateWorldState, TestData,
};

mod common;

/// Simulate test to call smart contract which invokes method from another contract.
#[test]
fn test_ctoc_api() {
    let (mut sws, contract_addr_1, contract_addr_2) =
        deploy_two_contracts("all_features", true, "all_features", true);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();

    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 200_000_000;

    // Set data in the First Contract.
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().add(12345_i32).make_call(
                Some(0),
                contract_addr_1,
                "set_data_only",
            )],
            nonce: 0,
            ..base_tx
        },
        bd.clone(),
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // make contract call from Second Contract to call get_data_only from First Contract.
    let function_args = Vec::<Vec<u8>>::new().try_to_vec().unwrap();
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add(contract_addr_1.clone()) // contract address
                .add("get_data_only".to_string()) // function name
                .add(function_args)
                .add(0u64) // value
                .add(1usize)
                .make_call(Some(0), contract_addr_2, "call_other_contract")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check result of "call_other_contract" -> "get_data_only"
    let return_bs: Vec<u8> =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap(); // CallResult structure made from returning statement of actions(), and bytes made from returning from call_other_contract() in Second Contract
    let get_data_value: i32 = BorshDeserialize::deserialize(&mut return_bs.as_slice()).unwrap(); // deserialize from bytes serialized from First Contract
    assert_eq!(12345_i32, get_data_value);

    // Make contract call from Second Contract to call set_data_only to First Contract.
    let mut function_args: Vec<u8> = vec![];
    // Construct data to function set_data() in First Contract.
    let mut set_data_value_bs: Vec<u8> = vec![];
    let set_data_value: i32 = 54321;
    set_data_value.serialize(&mut set_data_value_bs).unwrap();
    let function_args_bs: Vec<Vec<u8>> = vec![set_data_value_bs];
    BorshSerialize::serialize(&function_args_bs, &mut function_args).unwrap();

    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add(contract_addr_1.clone()) // contract address
                .add("set_data_only".to_string()) // function name
                .add(function_args)
                .add(0u64) // value
                .add(1usize) // count
                .make_call(Some(0), contract_addr_2, "call_other_contract")],
            nonce: 2,
            ..base_tx
        },
        bd.clone(),
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    assert_eq!(
        sws.get_storage_data(contract_addr_1, vec![0u8]),
        Some(54321_i32.to_le_bytes().to_vec())
    );
}

/// Simulate test to call smart contract which invokes another contract with multiple entrypoints by use_contract.
#[test]
fn test_ctoc_use_contract() {
    let (mut sws, _, contract_addr_2) =
        deploy_two_contracts("basic_contract", false, "all_features", true);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();
    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 200_000_000;

    // Call the Second contract to make cross contract call to First contract
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().add(0u64).make_call(
                Some(0),
                contract_addr_2,
                "call_other_contract_using_macro",
            )],
            nonce: 0,
            ..base_tx
        },
        bd.clone(),
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check result of "call_other_contract_using_macro" -> "hello".
    let return_bs: Vec<u8> =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert!(return_bs.is_empty()); // None returned from hello contract and then return vec![] from "call_other_contract_using_macro"

    // Check if hello() is executed as expected.
    assert!(receipt
        .last()
        .unwrap()
        .logs
        .iter()
        .find(|e| {
            e.topic == format!("topic: basic").as_bytes()
                && e.value == format!("Hello, Contract").as_bytes()
        })
        .is_some());

    // Call the Second contract to make cross contract call to First contract by using macro.
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add("testing name".to_string()) // input
                .add(0u64)
                .make_call(
                    Some(0),
                    contract_addr_2,
                    "call_other_contract_using_macro_with_input",
                )],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let _: SimulateWorldState = result.new_state.into();

    // Check the result of "call_other_contract_using_macro_with_input".
    let return_value: u32 =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    let check_value = "testing name".to_string().len() as u32;
    assert_eq!(return_value, check_value);

    // Check if hello_from() is executed as expected.
    assert!(receipt
        .last()
        .unwrap()
        .logs
        .iter()
        .find(|e| {
            e.topic == format!("topic: Hello From").as_bytes()
                && e.value == format!("Hello, Contract. From: testing name").as_bytes()
        })
        .is_some());
}

// Simulate test to call smart contract which invokes another contract with insufficient gas limit
#[test]
fn test_ctoc_with_insufficient_gas_limit() {
    let (mut sws, contract_addr_1, contract_addr_2) =
        deploy_two_contracts("all_features", true, "all_features", true);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();

    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 7_000_000;

    // make contract call from Second Contract to call get_data_only from First Contract
    let function_args = Vec::<Vec<u8>>::new().try_to_vec().unwrap();
    let tx = Transaction {
        commands: vec![ArgsBuilder::new()
            .add(contract_addr_1.clone()) // contract address
            .add("get_data_only".to_string()) // function name
            .add(function_args)
            .add(0u64) // value
            .add(20usize)
            .make_call(Some(0), contract_addr_2, "call_other_contract")],
        nonce: 0,
        ..base_tx
    };
    let tx_base_cost = pchain_runtime::gas::tx_inclusion_cost(
        pchain_types::serialization::Serializable::serialize(&tx).len(),
        tx.commands.len(),
    );
    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(
        receipt.last().unwrap().exit_status,
        ExitStatus::GasExhausted
    );
    assert_eq!(
        receipt.last().unwrap().gas_used,
        base_tx.gas_limit - tx_base_cost
    ); // tx.gas_limit - tx_base_cost
}

fn deploy_two_contracts(
    contract_name_1: &str,
    call_init_1: bool,
    contract_name_2: &str,
    call_init_2: bool,
) -> (SimulateWorldState, [u8; 32], [u8; 32]) {
    let contract_code_1 = TestData::get_test_contract_code(contract_name_1);
    let contract_code_2 = TestData::get_test_contract_code(contract_name_2);
    let origin_address = TestData::get_origin_address();
    let contract_address_1 = compute_contract_address(origin_address, 0)
        .try_into()
        .unwrap();
    let contract_address_2 = compute_contract_address(origin_address, 1)
        .try_into()
        .unwrap();

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    let deploy_1 = if call_init_1 {
        ArgsBuilder::new()
            .empty_args()
            .make_deploy(contract_code_1, 0)
    } else {
        ArgsBuilder::new().make_deploy(contract_code_1, 0)
    };
    // 0a. deploy first contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.commands = vec![deploy_1];
    tx.gas_limit = 300_000_000;

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    let deploy_2 = if call_init_2 {
        ArgsBuilder::new()
            .empty_args()
            .make_deploy(contract_code_2, 0)
    } else {
        ArgsBuilder::new().make_deploy(contract_code_2, 0)
    };
    // 0b. deploy second contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.commands = vec![deploy_2];
    tx.nonce = 1;
    tx.gas_limit = 300_000_000;

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    (sws, contract_address_1, contract_address_2)
}
