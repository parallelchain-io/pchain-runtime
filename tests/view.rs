use pchain_runtime::TransitionError;
use pchain_types::{
    blockchain::{CommandReceiptV2, ExitCodeV1, ExitCodeV2},
    cryptography::contract_address_v1,
};
use pchain_world_state::{V1, V2};

use crate::common::{
    ArgsBuilder, CallResult, SimulateWorldState, SimulateWorldStateStorage, TestData,
    CONTRACT_CACHE_FOLDER,
};

mod common;

/// Test calling view from runtime, cases:
/// 1. success case: call contract from world state
/// 2. success case: call contract from cache
/// 3. fail case: call non-exist contract
#[test]
fn test_view() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let method_args = "arg".to_string();
    let contract_address = contract_address_v1(&[123u8; 32], 0);

    // initialize world state
    let storage = SimulateWorldStateStorage::default();
    let mut sws: SimulateWorldState<'_, V1> = SimulateWorldState::new(&storage);
    sws.add_contract(contract_address, wasm_bytes, pchain_runtime::cbi_version());

    // 1. call contract from world state
    let (receipt, error) = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(pchain_runtime::Cache::new(std::path::Path::new(
            CONTRACT_CACHE_FOLDER,
        )))
        .view_v1(
            sws.world_state.clone(),
            u64::MAX,
            contract_address,
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );
    assert!(error.is_none());
    let gas_used = receipt.gas_used;
    // check return value from the called method
    let result_value: u32 = CallResult::parse(receipt.return_values).unwrap();
    assert_eq!(result_value as usize, method_args.len());

    // check event is emitted
    assert!(receipt
        .logs
        .iter()
        .find(|e| {
            String::from_utf8(e.topic.clone()).unwrap() == "topic: Hello From".to_string()
                && String::from_utf8(e.value.clone()).unwrap()
                    == format!("Hello, Contract. From: {}", method_args).to_string()
        })
        .is_some());

    // 2. retry with use of smart contract
    let (receipt, error) = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(pchain_runtime::Cache::new(std::path::Path::new(
            CONTRACT_CACHE_FOLDER,
        )))
        .view_v1(
            sws.world_state.clone(),
            u64::MAX,
            contract_address,
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );
    assert!(error.is_none());
    assert_eq!(receipt.gas_used, gas_used);

    // 3. call a non-exist contract
    let (receipt, error) = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(pchain_runtime::Cache::new(std::path::Path::new(
            CONTRACT_CACHE_FOLDER,
        )))
        .view_v1(
            sws.world_state,
            u64::MAX,
            [123u8; 32],
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );
    assert_eq!(receipt.exit_code, ExitCodeV1::Failed);
    assert_eq!(error, Some(TransitionError::InvalidCBI));

    // Clear sc cache folders.
    if std::path::Path::new(CONTRACT_CACHE_FOLDER).exists() {
        std::fs::remove_dir_all(CONTRACT_CACHE_FOLDER).unwrap();
    }

    /* Version 2 */
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let method_args = "arg".to_string();
    let contract_address = contract_address_v1(&[123u8; 32], 0);

    // initialize world state
    let storage = SimulateWorldStateStorage::default();
    let mut sws: SimulateWorldState<'_, V2> = SimulateWorldState::new(&storage);
    sws.add_contract(contract_address, wasm_bytes, pchain_runtime::cbi_version());

    // 1. call contract from world state
    let (command_receipt, error) = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(pchain_runtime::Cache::new(std::path::Path::new(
            CONTRACT_CACHE_FOLDER,
        )))
        .view_v2(
            sws.world_state.clone(),
            u64::MAX,
            contract_address,
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );

    assert!(error.is_none());
    if let CommandReceiptV2::Call(cr) = &command_receipt {
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
        let ret = CallResult::parse::<u32>(cr.return_value.clone()).unwrap();
        assert_eq!(ret, method_args.len() as u32);
        assert!(cr
            .logs
            .iter()
            .find(|e| {
                String::from_utf8(e.topic.clone()).unwrap() == "topic: Hello From".to_string()
                    && String::from_utf8(e.value.clone()).unwrap()
                        == format!("Hello, Contract. From: {}", method_args).to_string()
            })
            .is_some());
    } else {
        panic!("Call command receipt expected");
    }

    let expected_gas_used = match &command_receipt {
        CommandReceiptV2::Call(cr) => cr.gas_used,
        _ => panic!("Call command receipt expected"),
    };

    // 2. retry with use of smart contract
    let (commmand_receipt, error) = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(pchain_runtime::Cache::new(std::path::Path::new(
            CONTRACT_CACHE_FOLDER,
        )))
        .view_v2(
            sws.world_state.clone(),
            u64::MAX,
            contract_address,
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );
    assert!(error.is_none());
    if let CommandReceiptV2::Call(cr) = commmand_receipt {
        assert_eq!(cr.gas_used, expected_gas_used);
        assert_eq!(cr.exit_code, ExitCodeV2::Ok);
    } else {
        panic!("Call command receipt expected");
    }

    // 3. call a non-exist contract
    let (command_receipt, error) = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(pchain_runtime::Cache::new(std::path::Path::new(
            CONTRACT_CACHE_FOLDER,
        )))
        .view_v2(
            sws.world_state,
            u64::MAX,
            [123u8; 32],
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );

    assert_eq!(error, Some(TransitionError::InvalidCBI));
    if let CommandReceiptV2::Call(cr) = command_receipt {
        assert_eq!(cr.exit_code, ExitCodeV2::Error);
    } else {
        panic!("Call command receipt expected");
    }

    // Clear sc cache folders.
    if std::path::Path::new(CONTRACT_CACHE_FOLDER).exists() {
        std::fs::remove_dir_all(CONTRACT_CACHE_FOLDER).unwrap();
    }
}

/// Test calling view from runtime, cases:
/// 1. fail case: wasm runtime failule
/// 2. fail case: gas exhausted
/// 3. panic case: invoke non-callable view method
#[test]
fn test_view_failure() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let target = [2u8; 32];

    // initialize world state
    let storage = SimulateWorldStateStorage::default();
    let mut sws: SimulateWorldState<'_, V1> = SimulateWorldState::new(&storage);
    sws.add_contract(target, wasm_bytes, pchain_runtime::cbi_version());

    // 1. wasm execution fails
    let (receipt, error) = pchain_runtime::Runtime::new().view_v1(
        sws.world_state.clone(),
        u64::MAX,
        target,
        "emit_event_with_return".to_string(),
        ArgsBuilder::new()
            .add(1u8) // incorrect method argument type.
            .args,
    );
    assert_eq!(receipt.exit_code, ExitCodeV1::Failed);
    assert_eq!(error, Some(TransitionError::RuntimeError));

    // 2. fail for gas exhausted
    let (receipt, error) = pchain_runtime::Runtime::new().view_v1(
        sws.world_state.clone(),
        1_000_000, // smaller than gas_used in success case
        target,
        "emit_event_with_return".to_string(),
        ArgsBuilder::new().add("arg".to_string()).args,
    );
    assert_eq!(receipt.exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(error, Some(TransitionError::ExecutionProperGasExhausted));

    let (receipt, error) = pchain_runtime::Runtime::new().view_v1(
        sws.world_state,
        u64::MAX,
        target,
        "set_state_without_self".to_string(),
        ArgsBuilder::new().add(1u8).args,
    );
    assert_eq!(receipt.exit_code, ExitCodeV1::Failed);
    assert_eq!(error, Some(TransitionError::RuntimeError));

    /* Version 2 */
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let target = [2u8; 32];

    // initialize world state
    let storage = SimulateWorldStateStorage::default();
    let mut sws: SimulateWorldState<'_, V2> = SimulateWorldState::new(&storage);
    sws.add_contract(target, wasm_bytes, pchain_runtime::cbi_version());

    // 1. wasm execution fails
    let (command_receipt, error) = pchain_runtime::Runtime::new().view_v2(
        sws.world_state.clone(),
        u64::MAX,
        target,
        "emit_event_with_return".to_string(),
        ArgsBuilder::new()
            .add(1u8) // incorrect method argument type.
            .args,
    );
    assert_eq!(error, Some(TransitionError::RuntimeError));
    if let CommandReceiptV2::Call(cr) = command_receipt {
        assert_eq!(cr.exit_code, ExitCodeV2::Error);
    } else {
        panic!("Call command receipt expected");
    }

    // 2. fail for gas exhausted
    let (command_receipt, error) = pchain_runtime::Runtime::new().view_v2(
        sws.world_state.clone(),
        1_000_000, // smaller than gas_used in success case
        target,
        "emit_event_with_return".to_string(),
        ArgsBuilder::new().add("arg".to_string()).args,
    );
    assert_eq!(error, Some(TransitionError::ExecutionProperGasExhausted));
    if let CommandReceiptV2::Call(cr) = command_receipt {
        assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    } else {
        panic!("Call command receipt expected");
    }

    let (command_receipt, error) = pchain_runtime::Runtime::new().view_v2(
        sws.world_state,
        u64::MAX,
        target,
        "set_state_without_self".to_string(),
        ArgsBuilder::new().add(1u8).args,
    );
    assert_eq!(receipt.exit_code, ExitCodeV1::Failed);
    assert_eq!(error, Some(TransitionError::RuntimeError));
    if let CommandReceiptV2::Call(cr) = command_receipt {
        assert_eq!(cr.exit_code, ExitCodeV2::Error);
    } else {
        panic!("Call command receipt expected");
    }
}
