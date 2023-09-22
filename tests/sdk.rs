use std::path::Path;

use borsh::BorshDeserialize;
use pchain_runtime::Cache;
use pchain_types::{
    blockchain::{Command, ExitStatus, Transaction},
    runtime::{
        CreateDepositInput, SetDepositSettingsInput, StakeDepositInput, TopUpDepositInput,
        UnstakeDepositInput, WithdrawDepositInput,
    },
};
use pchain_world_state::network::constants::NETWORK_ADDRESS;

use crate::common::{
    compute_contract_address, gas::extract_gas_used, ArgsBuilder, CallResult, SimulateWorldState,
    TestData, CONTRACT_CACHE_FOLDER,
};

mod common;

/// Deploy a smart contract and simulate test to the basic_contract with `contract` macro.
/// Verify the data is correctly read/write from/to world state with different getter and setter.
#[test]
fn test_success_etoc_tx_with_different_setters_getters() {
    let contract_code = TestData::get_test_contract_code("basic_contract");
    let origin_address = [1u8; 32];
    let contract_address = compute_contract_address(origin_address, 0);

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.gas_limit = 400_000_000;
    tx.commands = vec![ArgsBuilder::new().make_deploy(contract_code, 0)];

    let runtime = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(Cache::new(Path::new(CONTRACT_CACHE_FOLDER)));

    let result = runtime.transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 121925230);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // prepare inputs
    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 100_000_000;

    // 1. set data using the self setter.
    let result = runtime.transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().add(1234_u64).make_call(
                Some(0),
                contract_address,
                "set_state_with_self",
            )],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // Verify the state changes
    assert_eq!(
        sws.get_storage_data(contract_address, vec![0u8]),
        Some(1234_u64.to_le_bytes().to_vec())
    );

    // 2. set data without using the self getter.
    let result = runtime.transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().add(5678_u64).make_call(
                Some(0),
                contract_address,
                "set_state_without_self",
            )],
            nonce: 2,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1279914);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // Verify the state changes
    assert_eq!(
        sws.get_storage_data(contract_address, vec![1u8]),
        Some(5678_u64.to_le_bytes().to_vec())
    );

    // 3. get data using the self getter.
    let result = runtime.transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().make_call(
                Some(0),
                contract_address,
                "get_state_with_self",
            )],
            nonce: 3,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1324005);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check the result of "get_state_with_self".
    let return_value: u64 =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert_eq!(return_value, 1234_u64);

    // 4. get data without using the self getter.
    let result = runtime.transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().make_call(
                Some(0),
                contract_address,
                "get_state_without_self",
            )],
            nonce: 4,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1259335);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);

    // Check the result of "get_state_without_self".
    let return_value: u64 =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert_eq!(return_value, 5678_u64);

    // Clear sc cache folders.
    if std::path::Path::new(CONTRACT_CACHE_FOLDER).exists() {
        std::fs::remove_dir_all(CONTRACT_CACHE_FOLDER).unwrap();
    }
}

/// The following test showcase a few things
/// 1.   Simulate test to call smart contract with different user-defined entrypoints
///      Verify entrypoint can be enterred correctly
/// 2.   Test data consistent(mvcc) when concurrently read–write the same state in execution.
///      The test set and get world state with several transactions in same block.
///      (TX 1-4 for concurrent read-write AND TX 7-9 for empty and previously modified state read)
/// 3.   TX 1 and TX 6 for event emission
#[test]
fn test_success_etoc_multiple_methods() {
    let contract_code = TestData::get_test_contract_code("basic_contract");
    let origin_address = [1u8; 32];
    let contract_address = compute_contract_address(origin_address, 0);

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.gas_limit = 400_000_000;
    tx.commands = vec![ArgsBuilder::new().make_deploy(contract_code, 0)];

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 121925230);

    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // prepare inputs
    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 100_000_000;

    // 1. get data from contract storage (should be default value).
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().make_call(
                Some(0),
                contract_address,
                "get_init_state_without_self",
            )],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1258440);

    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check result of "set_data" by "init";
    let return_value: i32 =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert!(return_value == 0 as i32);

    // 2. set data to contract storage.
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().add(i32::MAX).make_call(
                Some(0),
                contract_address,
                "set_init_state_without_self",
            )],
            nonce: 2,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1279345);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // Check if the value is really written into world state.
    assert_eq!(
        sws.get_storage_data(contract_address, vec![2u8]), // MyContract/data
        Some(i32::MAX.to_le_bytes().to_vec())
    );

    // 3. get data from contract storage (should be latest updated value).
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().make_call(
                Some(0),
                contract_address,
                "get_init_state_without_self",
            )],
            nonce: 3,
            ..base_tx
        },
        bd.clone(),
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check result of "set_data" by the second call to "set_data_to_storage";
    let return_value: i32 =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert!(return_value == i32::MAX);

    // 4. enter entrypoint with multiple arguments.
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add(i32::MIN)
                .add("argument to multiple_inputs".to_string())
                .add(vec![1u8, 0, 255])
                .make_call(Some(0), contract_address, "multiple_inputs")],
            nonce: 4,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1275515);

    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check result of "multiple_inputs".
    let return_string: String =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    let check_value = format!(
        "1: {} 2: {} 3: {:?}",
        i32::MIN,
        "argument to multiple_inputs".to_string(),
        vec![1u8, 0u8, 255u8]
    );
    assert_eq!(return_string, check_value);

    // 5. check print event
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().make_call(Some(0), contract_address, "print_event")],
            nonce: 5,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1259021);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check return value from print_event is None.
    assert!(receipt.last().unwrap().return_values.is_empty());

    // Check the method print_event was executed correctly.
    assert!(receipt
        .last()
        .unwrap()
        .logs
        .iter()
        .find(|e| {
            e.topic == format!("print_event topic").as_bytes()
                && e.value == format!("print_event value").as_bytes()
        })
        .is_some());

    // 6. test for non-existing keys.
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().add([0u8, 1].to_vec()).make_call(
                Some(0),
                contract_address,
                "raw_get",
            )],
            nonce: 6,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1260899);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check result of "raw_get" should be None for non-existing key.
    let return_value: Option<Vec<u8>> =
        CallResult::parse(receipt.last().unwrap().return_values.clone());
    assert!(return_value == None);

    // 7. test for key with empty vec (using field `arr` in contract storage).
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            commands: vec![ArgsBuilder::new().add([2u8].to_vec()).make_call(
                Some(0),
                contract_address,
                "raw_get",
            )],
            nonce: 7,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 1261720);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);

    // Check result of "raw_get" should be the init state variable.
    let return_value: Option<Vec<u8>> =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    let return_value: i32 =
        BorshDeserialize::deserialize(&mut return_value.unwrap().as_slice()).unwrap();
    assert!(return_value == i32::MAX);
}

/// Simulate test to call smart contract with nested struct as fields in contract storage
/// Moreover, compare than last test, it verify that nested state can be correctly setted.
/// Verify key of the fields in nested struct is correctly loaded/written
#[test]
fn test_success_etoc_set_all_contract_fields() {
    let contract_code = TestData::get_test_contract_code("all_features");
    let origin_address = [1u8; 32];
    let contract_address = compute_contract_address(origin_address, 0)
        .try_into()
        .unwrap();

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.gas_limit = 400_000_000;
    tx.commands = vec![ArgsBuilder::new()
        .empty_args()
        .make_deploy(contract_code, 0)];

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 220290230);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // 1. Call entrypoint to mutate data in contract storage.
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            signer: origin_address,
            commands: vec![ArgsBuilder::new().make_call(
                Some(0),
                contract_address,
                "set_all_fields",
            )],
            gas_limit: 67_500_000,
            priority_fee_per_gas: 0,
            max_base_fee_per_gas: 1,
            nonce: 1,
            hash: [0u8; 32],
            signature: [0u8; 64],
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2495040);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // Verify the fields altered by the execution of entrypoint.
    // key and the borsh-serialized bytes of expected value.
    let all_key_value: Vec<(Vec<u8>, Vec<u8>)> = vec![
        ([0].to_vec(), [131, 3, 0, 0].to_vec()), // MyContract/input: 899 i32
        (
            [1].to_vec(),
            [
                13, 0, 0, 0, 99, 111, 110, 116, 114, 97, 99, 116, 32, 110, 97, 109, 101,
            ]
            .to_vec(),
        ), // MyContract/name: "contract name" String
        ([2].to_vec(), [3, 0, 0, 0, 9, 0, 1].to_vec()), // MyContract/arr: [9,0,1] Vec<u8>
        ([3, 0].to_vec(), [16, 39, 0, 0, 0, 0, 0, 0].to_vec()), // MyContract/mf/field_1: 10000 u64
        (
            [3, 1, 0].to_vec(),
            [7, 0, 0, 0, 97, 108, 116, 101, 114, 101, 100].to_vec(),
        ), // MyContract/mf/df/deeper: "altered" String
    ];

    for (key, expected_value) in all_key_value {
        assert_eq!(
            sws.get_storage_data(contract_address, key),
            Some(expected_value)
        );
    }
}

/// Simulate test to check data access to Network Account Storage.
#[test]
fn test_success_etoc_network_state() {
    let contract_code = TestData::get_test_contract_code("all_features");
    let origin_address = [1u8; 32];
    let contract_address = compute_contract_address(origin_address, 0)
        .try_into()
        .unwrap();
    let network_state_app_key = vec![5u8];
    let network_state_app_value = 13579_u64.to_le_bytes().to_vec();

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_storage_data(
        NETWORK_ADDRESS,
        network_state_app_key.clone(),
        network_state_app_value.clone(),
    );
    // prepare a pool in network account (Operator, Power, Commission Rate, Operator's Own stake)
    sws.set_storage_data(
        NETWORK_ADDRESS,
        [[3u8].to_vec(), origin_address.to_vec(), [0u8].to_vec()].concat(),
        origin_address.to_vec(),
    );
    sws.set_storage_data(
        NETWORK_ADDRESS,
        [[3u8].to_vec(), origin_address.to_vec(), [1u8].to_vec()].concat(),
        0u64.to_le_bytes().to_vec(),
    );
    sws.set_storage_data(
        NETWORK_ADDRESS,
        [[3u8].to_vec(), origin_address.to_vec(), [2u8].to_vec()].concat(),
        [0u8; 1].to_vec(),
    );
    sws.set_storage_data(
        NETWORK_ADDRESS,
        [[3u8].to_vec(), origin_address.to_vec(), [3u8].to_vec()].concat(),
        [0u8; 1].to_vec(),
    );

    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.gas_limit = 400_000_000;
    tx.commands = vec![ArgsBuilder::new()
        .empty_args()
        .make_deploy(contract_code, 0)];

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 220290230);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // 1. Call entrypoint to mutate data in contract storage.
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            signer: origin_address,
            commands: vec![
                ArgsBuilder::new().add(network_state_app_key).make_call(
                    Some(10_000_000_000),
                    contract_address,
                    "get_network_state",
                ), // transfer some tokens to contract for staking
            ],
            gas_limit: 100_000_000,
            nonce: 1,
            ..TestData::transaction()
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2246202);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // Check result of "get_network_state";
    let return_value: Vec<u8> =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert_eq!(return_value, network_state_app_value);

    // 2. Issue network commands to stake
    let network_command_1 = Command::CreateDeposit(CreateDepositInput {
        operator: origin_address,
        balance: 1234,
        auto_stake_rewards: false,
    });
    let network_command_2 = Command::SetDepositSettings(SetDepositSettingsInput {
        operator: origin_address,
        auto_stake_rewards: true,
    });
    let network_command_3 = Command::TopUpDeposit(TopUpDepositInput {
        operator: origin_address,
        amount: 1,
    });
    let network_command_4 = Command::StakeDeposit(StakeDepositInput {
        operator: origin_address,
        max_amount: 1000,
    });
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            signer: origin_address,
            commands: vec![ArgsBuilder::new()
                .add(vec![
                    network_command_1,
                    network_command_2,
                    network_command_3,
                    network_command_4,
                ])
                .make_call(Some(0), contract_address, "defer_network_commands")],
            gas_limit: 100_000_000,
            nonce: 2,
            ..TestData::transaction()
        },
        bd.clone(),
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    assert_eq!(
        receipt.last().unwrap().return_values,
        1000u64.to_le_bytes().to_vec()
    );
    let sws: SimulateWorldState = result.new_state.into();

    // check if network command takes effect.
    let deposit_balance = sws
        .get_storage_data(
            NETWORK_ADDRESS,
            // WSKey for Deposit Balance
            [
                [4u8].to_vec(),
                origin_address.to_vec(),
                contract_address.to_vec(),
                [0u8].to_vec(),
            ]
            .concat(),
        )
        .unwrap();
    assert_eq!(deposit_balance, 1235u64.to_le_bytes().to_vec());

    // 3. Issue Network commands to withdraw
    let network_command_5 = Command::UnstakeDeposit(UnstakeDepositInput {
        operator: origin_address,
        max_amount: 1000,
    });
    let network_command_6 = Command::WithdrawDeposit(WithdrawDepositInput {
        operator: origin_address,
        max_amount: 2000,
    });
    let result = pchain_runtime::Runtime::new().transition(
        sws.world_state,
        Transaction {
            signer: origin_address,
            commands: vec![ArgsBuilder::new()
                .add(vec![network_command_5, network_command_6])
                .make_call(None, contract_address, "defer_network_commands")],
            gas_limit: 100_000_000,
            nonce: 3,
            ..TestData::transaction()
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2220638);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    assert_eq!(
        receipt.last().unwrap().return_values,
        1235u64.to_le_bytes().to_vec()
    );
    let sws: SimulateWorldState = result.new_state.into();

    // check if network command takes effect.
    let deposit_balance = sws.get_storage_data(
        NETWORK_ADDRESS,
        // WSKey for Deposit Balance
        [
            [4u8].to_vec(),
            origin_address.to_vec(),
            contract_address.to_vec(),
            [0u8].to_vec(),
        ]
        .concat(),
    );
    assert_eq!(deposit_balance, None);
}

/// Simulate basic_contract test with invalid data type as an argument.
/// Verify the transaction status code is FailureRuntimeError and no state has been setted.
#[test]
fn test_failure_etoc_tx_with_invalid_argument_data_type() {
    let contract_code = TestData::get_test_contract_code("basic_contract");
    let origin_address = [1u8; 32];
    let contract_address = compute_contract_address(origin_address, 0)
        .try_into()
        .unwrap();

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.gas_limit = 200_000_000;
    tx.commands = vec![ArgsBuilder::new().make_deploy(contract_code, 0)];

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 121925230);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // 1. Passing criteria: smart contract should fail to get the correct arguments (u64) as it is set to u32 for now.
    let tx = Transaction {
        signer: origin_address,
        commands: vec![ArgsBuilder::new().add(1234_u32).make_call(
            Some(0),
            contract_address,
            "set_state_with_self",
        )],
        gas_limit: 67_500_000,
        nonce: 1,
        ..TestData::transaction()
    };
    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 1264607);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Failed
    );
}

/// Simulate basic_contract test with no relevent method name returns in smart contract..
/// Verify the transaction status code is FailureRuntimeError and no state is setted.
#[test]
fn test_failure_etoc_tx_with_invalid_method_name() {
    let contract_code = TestData::get_test_contract_code("basic_contract");
    let origin_address = [1u8; 32];
    let contract_address = compute_contract_address(origin_address, 0);

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.gas_limit = 200_000_000;
    tx.commands = vec![ArgsBuilder::new().make_deploy(contract_code, 0)];

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // 1. EtoC call with non-exist method name
    let tx = Transaction {
        signer: origin_address,
        commands: vec![ArgsBuilder::new().add(1234_u64).make_call(
            Some(0),
            contract_address,
            "set_state1",
        )],
        gas_limit: 67_500_000,
        nonce: 1,
        ..TestData::transaction()
    };

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 1255007);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Failed
    );
}

/// Simulate test to call crypto functions in smart contract.
#[test]
fn test_success_etoc_crypto_functions() {
    let contract_code = TestData::get_test_contract_code("all_features");
    let origin_address = [1u8; 32];
    let contract_address = compute_contract_address(origin_address, 0);

    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    // 0. deploy contract
    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.gas_limit = 400_000_000;
    tx.commands = vec![ArgsBuilder::new().make_deploy(contract_code, 0)];

    let runtime = pchain_runtime::Runtime::new();

    let result = runtime.transition(sws.world_state, tx, bd.clone());
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_status,
        ExitStatus::Success
    );
    let sws: SimulateWorldState = result.new_state.into();

    // prepare inputs
    let mut base_tx = TestData::transaction();
    base_tx.signer = origin_address;
    base_tx.gas_limit = 100_000_000;

    // 1. Check Sha256
    let result = runtime.transition(
        sws.world_state.clone(),
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add(0u8)
                .add("1234".as_bytes().to_vec())
                .make_call(None, contract_address, "crypto_hash")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2216086);
    let receipt = result.receipt.unwrap().pop().unwrap();
    assert_eq!(receipt.exit_status, ExitStatus::Success);
    let hash: Vec<u8> = CallResult::parse(receipt.return_values).unwrap();
    assert_eq!(
        hash, // Hash of "1234" checked by online conversion tool
        [
            3u8, 172, 103, 66, 22, 243, 225, 92, 118, 30, 225, 165, 226, 85, 240, 103, 149, 54, 35,
            200, 179, 136, 180, 69, 158, 19, 249, 120, 215, 200, 70, 244
        ]
        .to_vec()
    );

    // 2. Check Keccak256
    let result = runtime.transition(
        sws.world_state.clone(),
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add(1u8)
                .add("1234".as_bytes().to_vec())
                .make_call(None, contract_address, "crypto_hash")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2216087);
    let receipt = result.receipt.unwrap().pop().unwrap();
    assert_eq!(receipt.exit_status, ExitStatus::Success);
    let hash: Vec<u8> = CallResult::parse(receipt.return_values).unwrap();
    assert_eq!(
        hash, // Hash of "1234" checked by online conversion tool
        [
            56u8, 122, 130, 51, 201, 110, 31, 192, 173, 94, 40, 67, 83, 39, 97, 119, 175, 33, 134,
            231, 175, 168, 82, 150, 241, 6, 51, 110, 55, 102, 105, 247
        ]
        .to_vec()
    );

    // 3. Check Ripemd
    let result = runtime.transition(
        sws.world_state.clone(),
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add(2u8)
                .add("1234".as_bytes().to_vec())
                .make_call(None, contract_address, "crypto_hash")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 2215625);
    let receipt = result.receipt.unwrap().pop().unwrap();
    assert_eq!(receipt.exit_status, ExitStatus::Success);
    let hash: Vec<u8> = CallResult::parse(receipt.return_values).unwrap();
    assert_eq!(
        hash, // Hash of "1234" checked by online conversion tool
        [
            205u8, 157, 55, 151, 21, 204, 204, 131, 253, 140, 140, 45, 192, 115, 12, 109, 208, 129,
            189, 53
        ]
        .to_vec()
    );

    // 4. Verify ed25519 - correct signature
    // let private = [0xA4_u8, 0x81, 0x53, 0xB8, 0x4B, 0x04, 0x4F, 0xC9, 0x51, 0x3A, 0x90, 0xE5, 0x26, 0xFB, 0xC7, 0x5C, 0x16, 0xEE, 0x0A, 0xE2, 0x98, 0x8B, 0xD4, 0x6D, 0x7B, 0x85, 0x0E, 0x10, 0x3F, 0x07, 0xD8, 0x3B];
    let public = [
        0x51_u8, 0x02, 0xE6, 0x26, 0x37, 0x2C, 0x31, 0x2B, 0x48, 0x1D, 0xA1, 0x88, 0xBB, 0x75,
        0x9F, 0xEE, 0x09, 0xCC, 0x86, 0xDF, 0x73, 0x69, 0x58, 0xA8, 0x0C, 0x4D, 0x19, 0x8B, 0x44,
        0xDD, 0xB4, 0xDA,
    ];
    let correct_signature = [
        199u8, 18, 193, 78, 69, 187, 39, 118, 98, 191, 80, 132, 96, 114, 28, 101, 207, 137, 0, 222,
        119, 150, 23, 16, 136, 27, 232, 149, 2, 128, 97, 97, 244, 84, 12, 188, 28, 155, 79, 255,
        240, 36, 133, 137, 183, 164, 148, 205, 188, 170, 91, 110, 34, 47, 183, 55, 215, 112, 12,
        80, 152, 170, 214, 9,
    ];
    let result = runtime.transition(
        sws.world_state.clone(),
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add("1234".as_bytes().to_vec())
                .add(correct_signature.to_vec())
                .add(public.to_vec())
                .make_call(None, contract_address, "crypto_verify_signature")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );

    assert_eq!(extract_gas_used(&result), 3619156);
    let receipt = result.receipt.unwrap().pop().unwrap();
    assert_eq!(receipt.exit_status, ExitStatus::Success);
    let is_correct: bool = CallResult::parse(receipt.return_values).unwrap();
    assert!(is_correct);

    // 5. Verify ed25519 - incorrect signature
    // let private = [0xA4_u8, 0x81, 0x53, 0xB8, 0x4B, 0x04, 0x4F, 0xC9, 0x51, 0x3A, 0x90, 0xE5, 0x26, 0xFB, 0xC7, 0x5C, 0x16, 0xEE, 0x0A, 0xE2, 0x98, 0x8B, 0xD4, 0x6D, 0x7B, 0x85, 0x0E, 0x10, 0x3F, 0x07, 0xD8, 0x3B];
    let public = [
        0x51_u8, 0x02, 0xE6, 0x26, 0x37, 0x2C, 0x31, 0x2B, 0x48, 0x1D, 0xA1, 0x88, 0xBB, 0x75,
        0x9F, 0xEE, 0x09, 0xCC, 0x86, 0xDF, 0x73, 0x69, 0x58, 0xA8, 0x0C, 0x4D, 0x19, 0x8B, 0x44,
        0xDD, 0xB4, 0xDA,
    ];
    let incorrect_signature = [9u8; 64];
    let result = runtime.transition(
        sws.world_state.clone(),
        Transaction {
            commands: vec![ArgsBuilder::new()
                .add("1234".as_bytes().to_vec())
                .add(incorrect_signature.to_vec())
                .add(public.to_vec())
                .make_call(None, contract_address, "crypto_verify_signature")],
            nonce: 1,
            ..base_tx
        },
        bd.clone(),
    );
    assert_eq!(extract_gas_used(&result), 3619156);
    let receipt = result.receipt.unwrap().pop().unwrap();
    assert_eq!(receipt.exit_status, ExitStatus::Success);
    let is_correct: bool = CallResult::parse(receipt.return_values).unwrap();
    assert!(!is_correct);
}
