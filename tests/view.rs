use pchain_runtime::TransitionError;
use pchain_types::blockchain::ExitStatus;

use crate::common::{
    compute_contract_address, ArgsBuilder, CallResult, SimulateWorldState, TestData,
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
    let contract_address = compute_contract_address([123u8; 32], 0);

    // initialize world state
    let mut sws = SimulateWorldState::default();
    sws.add_contract(contract_address, wasm_bytes, pchain_runtime::cbi_version());

    // 1. call contract from world state
    let (receipt, error) = pchain_runtime::Runtime::new()
        .set_smart_contract_cache(pchain_runtime::Cache::new(std::path::Path::new(
            CONTRACT_CACHE_FOLDER,
        )))
        .view(
            sws.world_state.clone(),
            u64::MAX,
            contract_address,
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );
    assert!(error.is_none());
    let gas_used = receipt.gas_used;
    println!("{:?}", receipt.return_values);
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
        .view(
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
        .view(
            sws.world_state,
            u64::MAX,
            [123u8; 32],
            "emit_event_with_return".to_string(),
            ArgsBuilder::new().add(method_args.clone()).args,
        );
    assert_eq!(receipt.exit_status, ExitStatus::Failed);
    assert_eq!(error, Some(TransitionError::InvalidCBI));

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
    let mut sws = SimulateWorldState::default();
    sws.add_contract(target, wasm_bytes, pchain_runtime::cbi_version());

    // 1. wasm execution fails
    let (receipt, error) = pchain_runtime::Runtime::new().view(
        sws.world_state.clone(),
        u64::MAX,
        target,
        "emit_event_with_return".to_string(),
        ArgsBuilder::new()
            .add(1u8) // incorrect method argument type.
            .args,
    );
    assert_eq!(receipt.exit_status, ExitStatus::Failed);
    assert_eq!(error, Some(TransitionError::RuntimeError));

    // 2. fail for gas exhausted
    let (receipt, error) = pchain_runtime::Runtime::new().view(
        sws.world_state.clone(),
        1_000_000, // smaller than gas_used in success case
        target,
        "emit_event_with_return".to_string(),
        ArgsBuilder::new().add("arg".to_string()).args,
    );
    assert_eq!(receipt.exit_status, ExitStatus::GasExhausted);
    assert_eq!(error, Some(TransitionError::ExecutionProperGasExhausted));

    let (receipt, error) = pchain_runtime::Runtime::new().view(
        sws.world_state,
        u64::MAX,
        target,
        "set_state_without_self".to_string(),
        ArgsBuilder::new().add(1u8).args,
    );
    assert_eq!(receipt.exit_status, ExitStatus::Failed);
    assert_eq!(error, Some(TransitionError::RuntimeError));
}
