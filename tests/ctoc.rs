use borsh::{BorshDeserialize, BorshSerialize};
use pchain_runtime::{gas::tx_inclusion_cost_v1, types::CommandKind};
use pchain_types::{
    blockchain::{CommandReceiptV2, ExitCodeV1, ExitCodeV2, TransactionV1, TransactionV2},
    cryptography::{contract_address_v1, contract_address_v2},
};
use pchain_world_state::{WorldState, V1, V2};

use crate::common::{
    gas::{extract_gas_used, verify_receipt_content_v2},
    ArgsBuilder, CallResult, SimulateWorldState, SimulateWorldStateStorage, TestData,
};

mod common;

/// Simulate test to call smart contract which invokes method from another contract.
#[test]
fn test_ctoc_api() {
    let storage = SimulateWorldStateStorage::default();
    let (mut sws, contract_addr_1, contract_addr_2) =
        deploy_two_contracts("all_features", true, "all_features", true, &storage);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();

    let mut base_tx = TestData::transaction_v1();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 200_000_000;

    // Set data in the First Contract.
    let result = pchain_runtime::Runtime::new().transition_v1(
        sws.world_state,
        TransactionV1 {
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
    assert_eq!(extract_gas_used(&result), 2262500);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

    // make contract call from Second Contract to call get_data_only from First Contract.
    let function_args = Vec::<Vec<u8>>::new().try_to_vec().unwrap();
    let result = pchain_runtime::Runtime::new().transition_v1(
        sws.world_state,
        TransactionV1 {
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
    assert_eq!(extract_gas_used(&result), 4467014);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

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

    let result = pchain_runtime::Runtime::new().transition_v1(
        sws.world_state,
        TransactionV1 {
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
    assert_eq!(extract_gas_used(&result), 4483108);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let mut sws: SimulateWorldState<'_, V1> = result.new_state.into();

    assert_eq!(
        sws.get_storage_data(contract_addr_1, vec![0u8]),
        Some(54321_i32.to_le_bytes().to_vec())
    );

    /* Version 2 */
    let storage = SimulateWorldStateStorage::default();
    let (mut sws, contract_addr_1, contract_addr_2) =
        deploy_two_contracts_v2("all_features", true, "all_features", true, &storage);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();

    let mut base_tx = TestData::transaction_v2();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 200_000_000;

    // Set data in the First Contract.
    let result = pchain_runtime::Runtime::new().transition_v2(
        sws.world_state,
        TransactionV2 {
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
    assert!(result.error.is_none());
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        2392430,
        2257700,
        ExitCodeV2::Ok,
        0
    ));
    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    // make contract call from Second Contract to call get_data_only from First Contract.
    let function_args = Vec::<Vec<u8>>::new().try_to_vec().unwrap();
    let result = pchain_runtime::Runtime::new().transition_v2(
        sws.world_state,
        TransactionV2 {
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
    assert!(result.error.is_none());
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        4603834,
        4466374,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::Call(cr)) =
        result.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        let ret = CallResult::parse::<Vec<u8>>(cr.return_value.clone()).unwrap();
        assert_eq!(
            // deserialize from bytes serialized from First Contract
            <i32 as BorshDeserialize>::deserialize(&mut ret.as_slice()).unwrap(),
            12345
        )
    } else {
        panic!("Call command receipt expected");
    }
    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    // Make contract call from Second Contract to call set_data_only to First Contract.
    let mut function_args: Vec<u8> = vec![];
    // Construct data to function set_data() in First Contract.
    let mut set_data_value_bs: Vec<u8> = vec![];
    let set_data_value: i32 = 54321;
    set_data_value.serialize(&mut set_data_value_bs).unwrap();
    let function_args_bs: Vec<Vec<u8>> = vec![set_data_value_bs];
    BorshSerialize::serialize(&function_args_bs, &mut function_args).unwrap();

    let result = pchain_runtime::Runtime::new().transition_v2(
        sws.world_state,
        TransactionV2 {
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

    assert!(result.error.is_none());
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        4616008,
        4478308,
        ExitCodeV2::Ok,
        0
    ));
    let mut sws: SimulateWorldState<'_, V2> = result.new_state.into();

    assert_eq!(
        sws.get_storage_data(contract_addr_1, vec![0u8]),
        Some(54321_i32.to_le_bytes().to_vec())
    );
}

/// Simulate test to call smart contract which invokes another contract with multiple entrypoints by use_contract.
#[test]
fn test_ctoc_use_contract() {
    let storage = SimulateWorldStateStorage::default();
    let (mut sws, _, contract_addr_2) =
        deploy_two_contracts("basic_contract", false, "all_features", true, &storage);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();
    let mut base_tx = TestData::transaction_v1();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 200_000_000;

    // Call the Second contract to make cross contract call to First contract
    let result = pchain_runtime::Runtime::new().transition_v1(
        sws.world_state,
        TransactionV1 {
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
    assert_eq!(extract_gas_used(&result), 3473496);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

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
    let result = pchain_runtime::Runtime::new().transition_v1(
        sws.world_state,
        TransactionV1 {
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
    assert_eq!(extract_gas_used(&result), 3489103);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);

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

    /* Version 2 */
    // note here we build in WorldState V1 because the "all_features" contract test fixture Wasm
    // includes a hardcoded contract V1 address from the "use_contract" macro
    let storage = SimulateWorldStateStorage::default();
    let (mut sws, _, contract_addr_2) =
        deploy_two_contracts("basic_contract", false, "all_features", true, &storage);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    // then upgrade to WS v2 and continue the test
    let ws_v2 = WorldState::<SimulateWorldStateStorage, V1>::upgrade(sws.world_state).unwrap();

    let bd = TestData::block_params();
    let mut base_tx = TestData::transaction_v2();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 200_000_000;

    // Call the Second contract to make cross contract call to First contract
    let result = pchain_runtime::Runtime::new().transition_v2(
        ws_v2,
        TransactionV2 {
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
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        3608886,
        3473496,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::Call(cr)) =
        result.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        let ret = CallResult::parse::<Vec<u8>>(cr.return_value.clone()).unwrap();
        assert_eq!(
            // None returned from hello contract and then return vec![] from "call_other_contract_using_macro"
            ret,
            vec![]
        );
        assert!(cr
            .logs
            .iter()
            .find(|e| {
                e.topic == format!("topic: basic").as_bytes()
                    && e.value == format!("Hello, Contract").as_bytes()
            })
            .is_some())
    } else {
        panic!("Call command receipt expected");
    }

    let sws: SimulateWorldState<'_, V2> = result.new_state.into();
    // Call the Second contract to make cross contract call to First contract by using macro.
    let result = pchain_runtime::Runtime::new().transition_v2(
        sws.world_state,
        TransactionV2 {
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
    assert!(result.error.is_none());
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        3625423,
        3489103,
        ExitCodeV2::Ok,
        0
    ));
    if let Some(CommandReceiptV2::Call(cr)) =
        result.receipt.as_ref().unwrap().command_receipts.last()
    {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        let ret = CallResult::parse::<u32>(cr.return_value.clone()).unwrap();
        assert_eq!("testing_name".to_string().len() as u32, ret);
        assert!(cr
            .logs
            .iter()
            .find(|e| {
                e.topic == format!("topic: Hello From").as_bytes()
                    && e.value == format!("Hello, Contract. From: testing name").as_bytes()
            })
            .is_some());
    } else {
        panic!("Call command receipt expected");
    }
}

// Simulate test to call smart contract which invokes another contract with insufficient gas limit
#[test]
fn test_ctoc_with_insufficient_gas_limit() {
    let storage = SimulateWorldStateStorage::default();
    let (mut sws, contract_addr_1, contract_addr_2) =
        deploy_two_contracts("all_features", true, "all_features", true, &storage);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();

    let mut base_tx = TestData::transaction_v1();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 7_000_000;

    // make contract call from Second Contract to call get_data_only from First Contract
    let function_args = Vec::<Vec<u8>>::new().try_to_vec().unwrap();
    let tx = TransactionV1 {
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
    let tx_base_cost = tx_inclusion_cost_v1(
        pchain_types::serialization::Serializable::serialize(&tx).len(),
        &tx.commands.iter().map(CommandKind::from).collect(),
    );
    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 6862810);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(
        receipt.last().unwrap().gas_used,
        base_tx.gas_limit - tx_base_cost
    ); // tx.gas_limit - tx_base_cost

    /* Version 2 */
    let storage = SimulateWorldStateStorage::default();
    let (mut sws, contract_addr_1, contract_addr_2) =
        deploy_two_contracts_v2("all_features", true, "all_features", true, &storage);
    let origin_address = [2u8; 32];
    sws.set_balance(origin_address, 300_000_000);

    let bd = TestData::block_params();

    let mut base_tx = TestData::transaction_v2();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 7_000_000;

    // make contract call from Second Contract to call get_data_only from First Contract
    let function_args = Vec::<Vec<u8>>::new().try_to_vec().unwrap();
    let tx = TransactionV2 {
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
    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    assert!(result.error.is_some());
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        7000000,
        6862540,
        ExitCodeV2::GasExhausted,
        0
    ));
}

fn deploy_two_contracts<'a>(
    contract_name_1: &str,
    call_init_1: bool,
    contract_name_2: &str,
    call_init_2: bool,
    storage: &'a SimulateWorldStateStorage,
) -> (SimulateWorldState<'a, V1>, [u8; 32], [u8; 32]) {
    let contract_code_1 = TestData::get_test_contract_code(contract_name_1);
    let contract_code_2 = TestData::get_test_contract_code(contract_name_2);
    let origin_address = TestData::get_origin_address();
    let contract_address_1 = contract_address_v1(&origin_address, 0);
    let contract_address_2 = contract_address_v1(&origin_address, 1);

    let bd = TestData::block_params();

    let mut sws: SimulateWorldState<'_, V1> = SimulateWorldState::new(storage);
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
    let mut tx = TestData::transaction_v1();
    tx.signer = origin_address;
    tx.commands = vec![deploy_1];
    tx.gas_limit = 300_000_000;

    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

    let deploy_2 = if call_init_2 {
        ArgsBuilder::new()
            .empty_args()
            .make_deploy(contract_code_2, 0)
    } else {
        ArgsBuilder::new().make_deploy(contract_code_2, 0)
    };
    // 0b. deploy second contract
    let mut tx = TestData::transaction_v1();
    tx.signer = origin_address;
    tx.commands = vec![deploy_2];
    tx.nonce = 1;
    tx.gas_limit = 300_000_000;

    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 220290230);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

    (sws, contract_address_1, contract_address_2)
}

fn deploy_two_contracts_v2<'a>(
    contract_name_1: &str,
    call_init_1: bool,
    contract_name_2: &str,
    call_init_2: bool,
    storage: &'a SimulateWorldStateStorage,
) -> (SimulateWorldState<'a, V2>, [u8; 32], [u8; 32]) {
    let contract_code_1 = TestData::get_test_contract_code(contract_name_1);
    let contract_code_2 = TestData::get_test_contract_code(contract_name_2);
    let origin_address = TestData::get_origin_address();
    let contract_address_1 = contract_address_v2(&origin_address, 0, 0);
    let contract_address_2 = contract_address_v2(&origin_address, 1, 0);

    let bd = TestData::block_params();

    let mut sws: SimulateWorldState<'_, V2> = SimulateWorldState::new(storage);
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
    let mut tx = TestData::transaction_v2();
    tx.signer = origin_address;
    tx.commands = vec![deploy_1];
    tx.gas_limit = 300_000_000;

    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    assert!(result.error.is_none());
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        223066070,
        220290230,
        ExitCodeV2::Ok,
        0
    ));

    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    let deploy_2 = if call_init_2 {
        ArgsBuilder::new()
            .empty_args()
            .make_deploy(contract_code_2, 0)
    } else {
        ArgsBuilder::new().make_deploy(contract_code_2, 0)
    };
    // 0b. deploy second contract
    let mut tx = TestData::transaction_v2();
    tx.signer = origin_address;
    tx.commands = vec![deploy_2];
    tx.nonce = 1;
    tx.gas_limit = 300_000_000;

    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    assert!(result.error.is_none());
    assert!(verify_receipt_content_v2(
        result.receipt.as_ref().expect("Receipt expected"),
        223066070,
        220290230,
        ExitCodeV2::Ok,
        0
    ));
    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    (sws, contract_address_1, contract_address_2)
}
