use pchain_runtime::{
    formulas::{TOTAL_BASE_FEE, TREASURY_CUT_OF_BASE_FEE},
    gas::tx_inclusion_cost,
    TransitionError,
};
use pchain_types::{
    blockchain::{Command, ExitStatus, Transaction},
    cryptography::PublicAddress,
    runtime::TransferInput,
    serialization::Serializable,
};

use crate::common::{
    compute_contract_address, ArgsBuilder, CallResult, SimulateWorldState, TestData,
    EXPECTED_CBI_VERSION,
};

mod common;

#[test]
fn version() {
    assert_eq!(pchain_runtime::cbi_version(), EXPECTED_CBI_VERSION);
}

/// Transfer tokens from external account to external account
#[test]
fn test_etoe() {
    let transfer_value = 1u64;
    let target = [2u8; 32];
    let mut tx = TestData::transaction();
    tx.commands = vec![Command::Transfer(TransferInput {
        recipient: target,
        amount: transfer_value,
    })];
    let priority_fee_per_gas = tx.priority_fee_per_gas;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());

    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = tx.signer;
    let to_address = target;
    let init_from_balance = 100_000_000;
    sws.set_balance(from_address, init_from_balance);

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let sws: SimulateWorldState = result.new_state.into();

    // check from_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(from_address.clone());
    assert_eq!(
        new_from_balance,
        init_from_balance
            - transfer_value
            - base_fee_per_gas * gas_used
            - priority_fee_per_gas * gas_used
    );
    assert_eq!(sws.get_nonce(from_address), 1);

    // check to_address balance
    let new_to_balance = sws.get_balance(to_address.clone());
    assert_eq!(new_to_balance, transfer_value);
    assert_eq!(sws.get_nonce(to_address), 0);
}

/// Contract Call from external account
#[test]
fn test_etoc() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let method_args = "arg".to_string();
    let method_name = "emit_event_with_return";
    let target = [2u8; 32];
    let mut tx = TestData::transaction();
    tx.gas_limit = 10_000_000;
    tx.commands =
        vec![ArgsBuilder::new()
            .add(method_args.clone())
            .make_call(Some(0), target, method_name)];
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());
    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = tx.signer;
    let to_address = target;
    let init_from_balance = 100_000_000;
    sws.set_balance(from_address, init_from_balance);
    sws.add_contract(to_address, wasm_bytes, pchain_runtime::cbi_version());

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);

    let sws: SimulateWorldState = result.new_state.into();

    // check from_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * gas_used
    );
    assert_eq!(sws.get_nonce(from_address), 1);

    // check to_address balance
    let new_to_balance = sws.get_balance(to_address);
    assert_eq!(new_to_balance, 0);
    assert_eq!(sws.get_nonce(to_address), 0);

    // check return value from the called method
    let result_value: u32 =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert_eq!(result_value as usize, method_args.len());

    // check event is emitted
    assert!(receipt
        .last()
        .unwrap()
        .logs
        .iter()
        .find(|e| {
            String::from_utf8(e.topic.clone()).unwrap() == "topic: Hello From".to_string()
                && String::from_utf8(e.value.clone()).unwrap()
                    == format!("Hello, Contract. From: {}", method_args).to_string()
        })
        .is_some());
}

/// Multiple Contract Calls in a Transaction
#[test]
fn test_etoc_multiple() {
    let wasm_bytes_1 = TestData::get_test_contract_code("all_features");
    let contract_address_1 = [22u8; 32];
    let method_args_1 = 123_i32;
    let method_name_1 = "set_data_only";
    let command_1 =
        ArgsBuilder::new()
            .add(method_args_1)
            .make_call(Some(1), contract_address_1, method_name_1);

    let wasm_bytes_2 = TestData::get_test_contract_code("basic_contract");
    let contract_address_2 = [2u8; 32];
    let method_args_2 = "arg".to_string();
    let method_name_2 = "emit_event_with_return";
    let command_2 = ArgsBuilder::new().add(method_args_2.clone()).make_call(
        Some(2),
        contract_address_2,
        method_name_2,
    );

    let method_name_3 = "get_data_only";
    let command_3 = ArgsBuilder::new().make_call(Some(3), contract_address_1, method_name_3);

    let mut tx = TestData::transaction();
    tx.gas_limit = 20_000_000;
    let proposer_address = [4u8; 32];
    let treasury_address = [100u8; 32];
    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = tx.signer;
    let init_from_balance = 500_000_000;
    sws.set_balance(from_address, init_from_balance);
    sws.add_contract(
        contract_address_1,
        wasm_bytes_1,
        pchain_runtime::cbi_version(),
    );
    sws.add_contract(
        contract_address_2,
        wasm_bytes_2,
        pchain_runtime::cbi_version(),
    );

    // test for two commands (dry run)
    tx.commands = vec![command_1.clone(), command_3.clone()];
    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state.clone(), tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.len(), tx.commands.len());
    assert!(!receipt.iter().any(|r| r.exit_status != ExitStatus::Success));
    let gas_consumed_1 = receipt[0].gas_used;
    let gas_consumed_3 = receipt[1].gas_used;

    // test for three commands (insert a command in the middle)
    tx.commands = vec![command_1.clone(), command_2.clone(), command_3.clone()];
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());
    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.len(), tx.commands.len());
    assert!(!receipt.iter().any(|r| r.exit_status != ExitStatus::Success));
    let gas_consumed_2 = receipt[1].gas_used;
    // check gas consumption and receipt is independent on the inserted command
    assert_eq!(gas_consumed_1, receipt[0].gas_used);
    assert_ne!(gas_consumed_1, gas_consumed_2);
    assert_eq!(gas_consumed_3, receipt[2].gas_used);
    assert!(receipt[2].logs.is_empty());
    let get_data_value: i32 = CallResult::parse(receipt[2].return_values.clone()).unwrap();
    assert_eq!(method_args_1, get_data_value);

    let sws: SimulateWorldState = result.new_state.into();

    // check state change from contract
    let data = sws.get_storage_data(contract_address_1, vec![0u8]);
    assert_eq!(data, Some(123_i32.to_le_bytes().to_vec()));

    // check from_address balance
    let total_gas_used = gas_consumed_1 + gas_consumed_2 + gas_consumed_3 + tx_base_cost;
    let new_from_balance = sws.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - 6 - base_fee_per_gas * total_gas_used
    ); // 6 = balance transferred to contract
    assert_eq!(sws.get_nonce(from_address), 1);

    // check contracts' nonce and balance
    assert_eq!(sws.get_balance(contract_address_1), 1 + 3);
    assert_eq!(sws.get_balance(contract_address_2), 2);
    assert_eq!(sws.get_nonce(contract_address_1), 0);
    assert_eq!(sws.get_nonce(contract_address_2), 0);

    // Proposer balance is unchanged because priority_fee_per_gas is set to 0
    assert_eq!(sws.get_balance(proposer_address), 0);

    // Treasury will get a cut of the base fee
    let treasury_balance = sws.get_balance(treasury_address);
    let base_fee_to_treasury =
        (total_gas_used * base_fee_per_gas * TREASURY_CUT_OF_BASE_FEE) / TOTAL_BASE_FEE;
    assert_eq!(treasury_balance, base_fee_to_treasury);
}

/// Multiple Contract Calls in a Transaction with insufficient gas
#[test]
fn test_etoc_multiple_insufficient_gas() {
    let wasm_bytes_1 = TestData::get_test_contract_code("all_features");
    let contract_address_1 = [22u8; 32];
    let command_1 =
        ArgsBuilder::new()
            .add(123_i32)
            .make_call(Some(1), contract_address_1, "set_data_only");

    let wasm_bytes_2 = TestData::get_test_contract_code("basic_contract");
    let contract_address_2 = [2u8; 32];
    let command_2 = ArgsBuilder::new().add("arg".to_string()).make_call(
        Some(2),
        contract_address_2,
        "emit_event_with_return",
    );

    let command_3 = ArgsBuilder::new().make_call(Some(3), contract_address_1, "get_data_only");

    let mut tx = TestData::transaction();
    tx.gas_limit = 20_000_000;
    let treasury_address = [100u8; 32];
    let mut bd = TestData::block_params();
    bd.proposer_address = [4u8; 32];
    bd.treasury_address = treasury_address;
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = tx.signer;
    let init_from_balance = 500_000_000;
    sws.set_balance(from_address, init_from_balance);
    sws.add_contract(
        contract_address_1,
        wasm_bytes_1,
        pchain_runtime::cbi_version(),
    );
    sws.add_contract(
        contract_address_2,
        wasm_bytes_2,
        pchain_runtime::cbi_version(),
    );

    // all commands success
    tx.commands = vec![command_1.clone(), command_2.clone(), command_3.clone()];
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());
    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state.clone(), tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.len(), tx.commands.len());
    assert!(!receipt.iter().any(|r| r.exit_status != ExitStatus::Success));
    let gas_consumed_1 = receipt[0].gas_used;
    let gas_consumed_2 = receipt[1].gas_used;
    let gas_consumed_3 = receipt[2].gas_used;

    // 1. Exhausted at first command
    tx.gas_limit = tx_base_cost;
    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state.clone(), tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.len(), 1);
    assert_eq!(
        receipt.iter().last().unwrap().exit_status,
        ExitStatus::GasExhausted
    );
    assert_eq!(receipt.iter().last().unwrap().gas_used, 0);
    let sws_1: SimulateWorldState = result.new_state.into();

    // check state unchange from contract
    let data = sws.get_storage_data(contract_address_1, vec![0u8]);
    assert_eq!(data, None);
    // check from_address and treasury balance
    let total_gas_used = tx.gas_limit;
    let new_from_balance = sws_1.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * total_gas_used
    );
    // Treasury will get a cut of the base fee
    let treasury_balance = sws_1.get_balance(treasury_address);
    let base_fee_to_treasury =
        (total_gas_used * base_fee_per_gas * TREASURY_CUT_OF_BASE_FEE) / TOTAL_BASE_FEE;
    assert_eq!(treasury_balance, base_fee_to_treasury);

    // 2. Exhausted at second command
    tx.gas_limit = gas_consumed_1 + tx_base_cost;
    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state.clone(), tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.len(), 2);
    assert_eq!(
        receipt.iter().last().unwrap().exit_status,
        ExitStatus::GasExhausted
    );
    assert_eq!(receipt.iter().last().unwrap().gas_used, 0);
    let sum_of_gas: u64 = receipt.iter().map(|r| r.gas_used).sum();
    assert_eq!(sum_of_gas, gas_consumed_1);
    let sws_2: SimulateWorldState = result.new_state.into();

    // check state unchange from contract
    let data = sws.get_storage_data(contract_address_1, vec![0u8]);
    assert_eq!(data, None);
    // check from_address and treasury balance
    let total_gas_used = tx.gas_limit;
    let new_from_balance = sws_2.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * total_gas_used
    );
    // Treasury will get a cut of the base fee
    let treasury_balance = sws_2.get_balance(treasury_address);
    let base_fee_to_treasury =
        (total_gas_used * base_fee_per_gas * TREASURY_CUT_OF_BASE_FEE) / TOTAL_BASE_FEE;
    assert_eq!(treasury_balance, base_fee_to_treasury);

    // 3. Exhausted at third command
    tx.gas_limit = gas_consumed_1 + gas_consumed_2 + tx_base_cost;
    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state.clone(), tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.len(), 3);
    assert_eq!(
        receipt.iter().last().unwrap().exit_status,
        ExitStatus::GasExhausted
    );
    assert_eq!(receipt.iter().last().unwrap().gas_used, 0);
    let sum_of_gas: u64 = receipt.iter().map(|r| r.gas_used).sum();
    assert_eq!(sum_of_gas, gas_consumed_1 + gas_consumed_2);
    let sws_3: SimulateWorldState = result.new_state.into();

    // check state unchange from contract
    let data = sws.get_storage_data(contract_address_1, vec![0u8]);
    assert_eq!(data, None);
    // check from_address and treasury balance
    let total_gas_used = tx.gas_limit;
    let new_from_balance = sws_3.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * total_gas_used
    );
    // Treasury will get a cut of the base fee
    let treasury_balance = sws_3.get_balance(treasury_address);
    let base_fee_to_treasury =
        (total_gas_used * base_fee_per_gas * TREASURY_CUT_OF_BASE_FEE) / TOTAL_BASE_FEE;
    assert_eq!(treasury_balance, base_fee_to_treasury);

    // 4. Exhausted at third command (1 Gas difference)
    tx.gas_limit = gas_consumed_1 + gas_consumed_2 + gas_consumed_3 + tx_base_cost - 1;
    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state.clone(), tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.len(), 3);
    assert_eq!(
        receipt.iter().last().unwrap().exit_status,
        ExitStatus::GasExhausted
    );
    assert_eq!(receipt.iter().last().unwrap().gas_used, gas_consumed_3 - 1);
    let sum_of_gas: u64 = receipt.iter().map(|r| r.gas_used).sum();
    assert_eq!(
        sum_of_gas,
        gas_consumed_1 + gas_consumed_2 + gas_consumed_3 - 1
    );
    let sws_4: SimulateWorldState = result.new_state.into();

    // check state unchange from contract
    let data = sws.get_storage_data(contract_address_1, vec![0u8]);
    assert_eq!(data, None);
    // check from_address and treasury balance
    let total_gas_used = tx.gas_limit;
    let new_from_balance = sws_4.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * total_gas_used
    );
    // Treasury will get a cut of the base fee
    let treasury_balance = sws_4.get_balance(treasury_address);
    let base_fee_to_treasury =
        (total_gas_used * base_fee_per_gas * TREASURY_CUT_OF_BASE_FEE) / TOTAL_BASE_FEE;
    assert_eq!(treasury_balance, base_fee_to_treasury);
}

#[test]
fn test_etoc_panic() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let method_args = true; // incorrect argument
    let method_name = "set_init_state_without_self";
    let target = [2u8; 32];
    let mut tx = TestData::transaction();
    tx.commands =
        vec![ArgsBuilder::new()
            .add(method_args.clone())
            .make_call(None, target, method_name)];
    tx.gas_limit = 10_000_000;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());
    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = tx.signer;
    let to_address = target;
    let init_from_balance = 100_000_000;
    sws.set_balance(from_address, init_from_balance);
    sws.add_contract(to_address, wasm_bytes, pchain_runtime::cbi_version());

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    assert_eq!(result.error, Some(TransitionError::RuntimeError));
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Failed);

    let sws: SimulateWorldState = result.new_state.into();

    // check from_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * gas_used
    );
    assert_eq!(sws.get_nonce(from_address), 1);

    // check to_address balance
    let new_to_balance = sws.get_balance(to_address);
    assert_eq!(new_to_balance, 0);
    assert_eq!(sws.get_nonce(to_address), 0);
}

#[test]
fn test_etoc_insufficient_gas() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let method_args = "arg".to_string();
    let method_name = "emit_event_with_return";
    let method_call_success_gas_consumption = 2_000_000;
    let target = [2u8; 32];
    let mut success_tx = TestData::transaction();
    success_tx.commands =
        vec![ArgsBuilder::new()
            .add(method_args.clone())
            .make_call(Some(1), target, method_name)];
    success_tx.gas_limit = method_call_success_gas_consumption;
    let tx_base_cost = tx_inclusion_cost(success_tx.serialize().len(), success_tx.commands.len());
    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = success_tx.signer;
    let to_address = target;
    let init_from_balance = (method_call_success_gas_consumption + 1) * bd.this_base_fee;
    sws.set_balance(from_address, init_from_balance);
    sws.add_contract(to_address, wasm_bytes, pchain_runtime::cbi_version());

    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state, success_tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let mut sws: SimulateWorldState = result.new_state.into();

    // Obtain the gas_used and reset setup.
    let method_call_theoretical_gas_consumption = receipt.last().unwrap().gas_used + tx_base_cost;
    let init_from_balance = method_call_theoretical_gas_consumption * bd.this_base_fee;
    sws.set_balance(from_address, init_from_balance);
    sws.set_balance(to_address, 0);

    let tx = Transaction {
        gas_limit: method_call_theoretical_gas_consumption - 1,
        nonce: success_tx.nonce + 1,
        ..success_tx
    };
    let tx_gas_limit = tx.gas_limit;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    let receipt = result.receipt.unwrap();
    assert_eq!(
        receipt.last().unwrap().exit_status,
        ExitStatus::GasExhausted
    );
    assert_eq!(
        receipt.last().unwrap().gas_used,
        tx_gas_limit - tx_base_cost
    );

    let sws: SimulateWorldState = result.new_state.into();

    // check from_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(from_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * gas_used
    );
    assert_eq!(sws.get_nonce(from_address), 2);

    // check to_address balance
    let new_to_balance = sws.get_balance(to_address);
    assert_eq!(new_to_balance, 0);
    assert_eq!(sws.get_nonce(to_address), 0);

    // check event is not empty (logs is not erased if transition exits later)
    assert!(!receipt.last().unwrap().logs.is_empty());
}

/// Contract Call to a method that invokes various host functions.
#[test]
fn test_etoc_host_functions() {
    let wasm_bytes = TestData::get_test_contract_code("all_features");
    let target = [2u8; 32];
    let mut tx = TestData::transaction();
    tx.gas_limit = 10_000_000;
    tx.commands = vec![ArgsBuilder::new().make_call(None, target, "about")];
    let bd = TestData::block_params();

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = tx.signer;
    let to_address = target;
    let init_from_balance = 100_000_000;
    sws.set_balance(from_address, init_from_balance);
    sws.add_contract(to_address, wasm_bytes, pchain_runtime::cbi_version());

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
}

#[test]
fn test_ctoc() {
    let wasm_bytes_1 = TestData::get_test_contract_code("all_features");
    let wasm_bytes_2 = TestData::get_test_contract_code("all_features");

    let contract_address_1 = [49u8; 32];
    let contract_address_2 = [50u8; 32];

    // transfer value in EtoC
    let value_to_contract_2 = 10;
    // transfer value in CtoC
    let value_to_contract_1 = 5;
    // data store to contract 1
    let data_only = 1234_i32;

    // make contract call from Second Contract to call get_data_only from First Contract.
    let function_args = borsh::BorshSerialize::try_to_vec(&Vec::<Vec<u8>>::new()).unwrap();
    let mut tx = TestData::transaction();
    tx.commands = vec![ArgsBuilder::new()
        .add(contract_address_1)
        .add("get_data_only".to_string()) // function name
        .add(function_args)
        .add(value_to_contract_1) // value
        .add(1usize)
        .make_call(
            Some(value_to_contract_2),
            contract_address_2,
            "call_other_contract",
        )];
    tx.gas_limit = 100_000_000;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());

    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let from_address = tx.signer;
    let to_address = contract_address_2;
    let init_from_balance = 1_000_000_000;
    sws.set_balance(from_address, init_from_balance);
    sws.add_contract(
        contract_address_1,
        wasm_bytes_1,
        pchain_runtime::cbi_version(),
    );
    sws.add_contract(
        contract_address_2,
        wasm_bytes_2,
        pchain_runtime::cbi_version(),
    );
    sws.set_storage_data(
        contract_address_1,
        vec![0u8],
        data_only.to_le_bytes().to_vec(),
    );

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);

    let sws: SimulateWorldState = result.new_state.into();

    // check from_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(from_address.clone());
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * gas_used - value_to_contract_2
    );
    assert_eq!(sws.get_nonce(from_address), 1);

    // check to_address balance
    let new_to_balance = sws.get_balance(to_address.clone());
    assert_eq!(new_to_balance, value_to_contract_2 - value_to_contract_1);
    assert_eq!(sws.get_nonce(to_address), 0);

    // check return value from the called method, which is the data stored inside world state.
    let result_value: Vec<u8> =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert_eq!(result_value, data_only.to_le_bytes().to_vec());
}

#[test]
fn test_ctoe() {
    let wasm_bytes = TestData::get_test_contract_code("all_features");

    let origin_address = [1u8; 32];
    let init_from_balance = 100_000_000;
    let contract_addr = [2u8; 32];
    // transfer value to contract in EtoC
    let value_to_contract = 100_000;
    let receiver_address = [3u8; 32];
    // transfer value in CtoE
    let value_to_receiver: u64 = 90_000;

    let mut tx = TestData::transaction();
    tx.commands = vec![ArgsBuilder::new()
        .add(receiver_address)
        .add(value_to_receiver)
        .make_call(
            Some(value_to_contract),
            contract_addr,
            "make_balance_transfer",
        )];
    tx.gas_limit = 10_000_000;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());

    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    sws.set_balance(origin_address, init_from_balance);
    sws.add_contract(contract_addr, wasm_bytes, pchain_runtime::cbi_version());

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);

    let sws: SimulateWorldState = result.new_state.into();

    // check origin_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(origin_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * gas_used - value_to_contract
    );
    assert_eq!(sws.get_nonce(origin_address), 1);

    // check to_address balance
    let new_to_balance = sws.get_balance(contract_addr);
    assert_eq!(new_to_balance, value_to_contract - value_to_receiver);
    assert_eq!(sws.get_nonce(contract_addr), 0);

    // check receiver balance
    let new_receiver_balance = sws.get_balance(receiver_address);
    assert_eq!(new_receiver_balance, value_to_receiver);
    assert_eq!(sws.get_nonce(receiver_address), 0);

    // check return value from the called method.
    // It should return remaining balance of the contract
    let result_value: u64 =
        CallResult::parse(receipt.last().unwrap().return_values.clone()).unwrap();
    assert_eq!(result_value, value_to_contract - value_to_receiver);
}

#[test]
fn test_deploy() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");

    let origin_address = [1u8; 32];
    let contract_address: PublicAddress = compute_contract_address(origin_address, 0);

    let mut tx = TestData::transaction();
    tx.commands = vec![ArgsBuilder::new().make_deploy(wasm_bytes, 0)];
    tx.gas_limit = 400_000_000;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());

    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let init_from_balance = 5_000_000_000;
    sws.set_balance(origin_address, init_from_balance);

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);

    let sws: SimulateWorldState = result.new_state.into();

    // check origin_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(origin_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * gas_used
    );
    assert_eq!(sws.get_nonce(origin_address), 1);

    // check to_address balance
    let new_to_balance = sws.get_balance(contract_address);
    assert_eq!(new_to_balance, 0);
    assert_eq!(sws.get_nonce(contract_address), 0);

    // check if contract is stored to world state
    assert!(sws.get_contract_code(contract_address).is_some());
}

/// Simulate test to deploy an invalid entrypoint contract,
/// Note that A contract cannot be compiled for serval reasons, not only limited to invalid entrypointq
/// 1. Fail to creates a new Instance from a WebAssembly Module and a set of imports (InstantiationError).
/// Check wasmer doc for details: https://docs.rs/wasmer/latest/wasmer/enum.InstantiationError.html
/// 2. Missing the start functions "entrypoint()" in the smart contract
#[test]
fn test_deploy_invalid_entrypoint_contract() {
    let wasm_bytes = TestData::get_test_contract_code("invalid_entrypoint_contract");

    let origin_address = [1u8; 32];
    let contract_address: PublicAddress = compute_contract_address(origin_address, 0);

    let mut tx = TestData::transaction();
    tx.commands = vec![ArgsBuilder::new().make_deploy(wasm_bytes, 0)];
    tx.gas_limit = 20_500_000;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());

    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000;
    sws.set_balance(origin_address, init_from_balance);

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    assert_eq!(
        result.error,
        Some(TransitionError::NoExportedContractMethod)
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Failed);
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let sws: SimulateWorldState = result.new_state.into();

    // Verify the contract code not save to world state
    assert!(sws.get_contract_code(contract_address).is_none());

    // Check if balance is deducted for deploying invalid contract
    assert_eq!(
        sws.get_balance(origin_address),
        init_from_balance - base_fee_per_gas * gas_used
    );
}

/// Simulate test to call smart contract with an invalid opcode.
/// my_non_deterministic_contract.wasm has floating point operations.
/// Verify the transaction status for the deployed smart_contract code is FailureInvalidOpCode
#[test]
fn test_deploy_contract_with_invalid_opcode() {
    let wasm_bytes = TestData::get_test_contract_code("invalid_non_deterministic");
    let origin_address = [1u8; 32];

    let mut tx = TestData::transaction();
    tx.signer = origin_address;
    tx.commands = vec![ArgsBuilder::new().make_deploy(wasm_bytes, 0)];
    tx.gas_limit = 20_000_000;

    let bd = TestData::block_params();

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let init_from_balance = 500_000_000;
    sws.set_balance(origin_address, init_from_balance);

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    assert_eq!(result.error, Some(TransitionError::DisallowedOpcode));
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Failed);
}

#[test]
fn test_deploy_insufficient_gas() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");
    let method_call_success_gas_consumption = 200_000_000;
    let origin_address = [1u8; 32];
    let contract_address: PublicAddress = compute_contract_address(origin_address, 0);

    let mut success_tx = TestData::transaction();
    success_tx.commands = vec![ArgsBuilder::new().make_deploy(wasm_bytes, 0)];
    success_tx.gas_limit = method_call_success_gas_consumption;
    let tx_base_cost = tx_inclusion_cost(success_tx.serialize().len(), success_tx.commands.len());

    let bd = TestData::block_params();
    let base_fee_per_gas = bd.this_base_fee;

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let init_from_balance = (method_call_success_gas_consumption + 1) * bd.this_base_fee;
    sws.set_balance(origin_address, init_from_balance);

    let result =
        pchain_runtime::Runtime::new().transition(sws.world_state, success_tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);
    let mut sws: SimulateWorldState = result.new_state.into();

    // Obtain the gas_used and reset setup.
    let method_call_theoretical_gas_consumption = receipt.last().unwrap().gas_used + tx_base_cost;
    let init_from_balance = method_call_theoretical_gas_consumption * bd.this_base_fee;
    sws.set_balance(origin_address, init_from_balance);

    let tx = Transaction {
        gas_limit: method_call_theoretical_gas_consumption - 1,
        nonce: success_tx.nonce + 1,
        ..success_tx
    };
    let tx_gas_limit = tx.gas_limit;
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());

    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx, bd);
    assert_eq!(
        result.error,
        Some(TransitionError::ExecutionProperGasExhausted)
    );
    let receipt = result.receipt.unwrap();
    assert_eq!(
        receipt.last().unwrap().exit_status,
        ExitStatus::GasExhausted
    );
    assert_eq!(
        receipt.last().unwrap().gas_used,
        tx_gas_limit - tx_base_cost
    );
    let sws: SimulateWorldState = result.new_state.into();

    // check origin_address balance
    let gas_used = receipt.last().unwrap().gas_used + tx_base_cost;
    let new_from_balance = sws.get_balance(origin_address);
    assert_eq!(
        new_from_balance,
        init_from_balance - base_fee_per_gas * gas_used
    );
    assert_eq!(sws.get_nonce(origin_address), 2);

    // check to_address balance
    let new_to_balance = sws.get_balance(contract_address);
    assert_eq!(new_to_balance, 0);
    assert_eq!(sws.get_nonce(contract_address), 0);

    // check event is emitted in Init method
    assert!(receipt.last().unwrap().logs.is_empty());
}

#[test]
fn test_memory_limited_contract_module() {
    let wasm_bytes = TestData::get_test_contract_code("basic_contract");

    let mut tx = TestData::transaction();
    tx.commands = vec![ArgsBuilder::new().make_deploy(wasm_bytes, 0)];
    tx.gas_limit = 400_000_000;
    let bd = TestData::block_params();

    let mut sws = SimulateWorldState::default();
    sws.set_balance(tx.signer, 5_000_000_000);

    // Within Memory limit
    let runtime = pchain_runtime::Runtime::new().set_smart_contract_memory_limit(100 * 1024 * 1024);
    let result = runtime.transition(sws.world_state.clone(), tx.clone(), bd.clone());
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Success);

    // Exceed Memory limit
    let runtime = pchain_runtime::Runtime::new().set_smart_contract_memory_limit(1024);
    let result = runtime.transition(sws.world_state.clone(), tx, bd);
    assert_eq!(result.error, Some(TransitionError::CannotCompile));
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_status, ExitStatus::Failed);
}

/// Possible fail cases in PreCharge Phase:
/// - transaction gas limit is smaller than minimum required gas
/// - incorrect nonce
/// - insufficient balance
#[test]
fn test_fail_in_pre_charge() {
    let tx = TestData::transaction();
    let tx_base_cost = tx_inclusion_cost(tx.serialize().len(), tx.commands.len());
    let bd = TestData::block_params();

    // initialize world state
    let mut sws = SimulateWorldState::default();
    let init_from_balance = 100_000_000;
    sws.set_balance(tx.signer, init_from_balance);

    // 1. gas limit is smaller than minimum required gas
    let tx1 = Transaction {
        gas_limit: tx_base_cost - 1,
        ..tx.clone()
    };
    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx1, bd.clone());
    assert!(result.receipt.is_none());
    assert_eq!(result.error, Some(TransitionError::PreExecutionGasExhausted));
    let sws: SimulateWorldState = result.new_state.into();


    // 2. nonce is incorrect
    let tx2 = Transaction {
        nonce: 1,
        ..tx.clone()
    };
    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx2, bd.clone());
    assert!(result.receipt.is_none());
    assert_eq!(result.error, Some(TransitionError::WrongNonce));
    let sws: SimulateWorldState = result.new_state.into();

    // 3. balance is not enough
    let tx3 = Transaction {
        priority_fee_per_gas: u64::MAX,
        ..tx.clone()
    };
    let result = pchain_runtime::Runtime::new().transition(sws.world_state, tx3, bd.clone());
    assert!(result.receipt.is_none());
    assert_eq!(result.error, Some(TransitionError::NotEnoughBalanceForGasLimit));
    let sws: SimulateWorldState = result.new_state.into();

    // check from_address balance (unchanged)
    let new_from_balance = sws.get_balance(tx.signer);
    assert_eq!(new_from_balance, init_from_balance);
    assert_eq!(sws.get_nonce(tx.signer), 0);
}