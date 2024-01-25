use pchain_runtime::BlockchainParams;
use pchain_types::{
    blockchain::{Command, CommandReceiptV2, ExitCodeV1, ExitCodeV2, TransactionV1, TransactionV2},
    cryptography::{contract_address_v1, contract_address_v2, PublicAddress},
    runtime::{StakeDepositInput, UnstakeDepositInput, WithdrawDepositInput},
};
use pchain_world_state::{NetworkAccount, Stake, StakeValue, VersionProvider, V1, V2};

use crate::common::{
    gas::{extract_gas_used, verify_receipt_content_v2},
    ArgsBuilder, CallResult, SimulateWorldState, SimulateWorldStateStorage, TestData,
};

mod common;

//
//
//
//
//
// ↓↓↓ Version 1 ↓↓↓ //
//
//
//
//
//
/// When executing a StakeDeposit Command, the return value should NOT be written to the CommandReceipt in the case of insufficient gas.
#[test]
fn test_short_circuit_insufficient_gas_stake_deposit_ret_val() {
    let storage = SimulateWorldStateStorage::default();
    let (depositor_addr, bd, mut sws) = init_ws::<V1>(&storage);
    let operator_addr: PublicAddress = [2u8; 32];

    let nonce: u64 = 0;
    let starting_operator_power = 100_000;
    let stake_amount = 20_000;

    //
    // 0. set up a Pool and Operator
    //
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    pool.set_operator(operator_addr);
    pool.set_power(starting_operator_power);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    deposit.set_balance(stake_amount);
    deposit.set_auto_stake_rewards(false);

    let sws_first_run: SimulateWorldState<'_, V1> = sws.into();
    let sws_second_run: SimulateWorldState<'_, V1> = sws_first_run.clone();

    // expected gas costs
    let tx_inclusion_cost_v1 = 133530;
    let cmd_cost = 382740;

    //
    // 1. issue a StakeDeposit command with sufficient gas to stake
    //
    let tx = TransactionV1 {
        commands: vec![Command::StakeDeposit(StakeDepositInput {
            operator: operator_addr,
            max_amount: stake_amount,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v1 + cmd_cost,
        ..TestData::transaction_v1()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v1(sws_first_run.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), cmd_cost);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    assert_eq!(
        CallResult::parse::<u64>(receipt.last().unwrap().return_values.clone()),
        Some(stake_amount)
    );
    let mut sws: SimulateWorldState<'_, V1> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(
        pool.power().unwrap(),
        starting_operator_power + stake_amount
    );
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr).unwrap();
    assert_eq!(delegated_stake.power, stake_amount);

    //
    // 2. issue a StakeDeposit command with insufficient gas
    //
    let tx = TransactionV1 {
        commands: vec![Command::StakeDeposit(StakeDepositInput {
            operator: operator_addr,
            max_amount: stake_amount,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v1 + cmd_cost - 1,
        ..TestData::transaction_v1()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v1(sws_second_run.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), cmd_cost - 1);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(
        CallResult::parse::<u64>(receipt.last().unwrap().return_values.clone()),
        None
    );
    let mut sws: SimulateWorldState<'_, V1> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(pool.power().unwrap(), starting_operator_power);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr);
    assert!(delegated_stake.is_none());
}

/// When executing a StakeDeposit Command, the logs should NOT be written to the CommandReceipt in the case of insufficient gas.
#[test]
fn test_short_circuit_insufficient_gas_unstake_deposit_ret_val() {
    let storage = SimulateWorldStateStorage::default();
    let (depositor_addr, bd, mut sws) = init_ws::<V1>(&storage);
    let operator_addr: PublicAddress = [2u8; 32];

    let nonce: u64 = 0;
    let total_pool_power = 100_000;
    let initial_deposit_amt = 80_000;
    let depositor_stake_amt = 50_000;
    let unstake_amt = 40_000;
    //
    // 0. set up a Pool and Operator
    //
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    pool.set_operator(operator_addr);
    pool.set_power(total_pool_power);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    pool.delegated_stakes()
        .insert(StakeValue::new(Stake {
            owner: depositor_addr,
            power: depositor_stake_amt,
        }))
        .unwrap();
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    deposit.set_balance(initial_deposit_amt);
    deposit.set_auto_stake_rewards(false);

    let sws_first_run: SimulateWorldState<'_, V1> = sws.into();
    let sws_second_run: SimulateWorldState<'_, V1> = sws_first_run.clone();

    // expected gas costs
    let tx_inclusion_cost_v1 = 133_530;
    let cmd_cost = 311_320;

    //
    // 1. issue an UnstakeDeposit command with sufficient gas to stake
    //
    let tx = TransactionV1 {
        commands: vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: operator_addr,
            max_amount: unstake_amt,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v1 + cmd_cost,
        ..TestData::transaction_v1()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v1(sws_first_run.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), cmd_cost);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    assert_eq!(
        CallResult::parse::<u64>(receipt.last().unwrap().return_values.clone()),
        Some(unstake_amt)
    );
    let mut sws: SimulateWorldState<'_, V1> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(pool.power().unwrap(), total_pool_power - unstake_amt);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr).unwrap();
    assert_eq!(delegated_stake.power, depositor_stake_amt - unstake_amt);

    //
    // 2. issue an UnstakeDeposit command with insufficient gas
    //
    let tx = TransactionV1 {
        commands: vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: operator_addr,
            max_amount: unstake_amt,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v1 + cmd_cost - 1,
        ..TestData::transaction_v1()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v1(sws_second_run.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), cmd_cost - 1);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(
        CallResult::parse::<u64>(receipt.last().unwrap().return_values.clone()),
        None
    );
    let mut sws: SimulateWorldState<'_, V1> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(pool.power().unwrap(), total_pool_power);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr).unwrap();
    assert_eq!(delegated_stake.power, depositor_stake_amt);
}

/// When executing a WithdrawDeposit Command, the logs should NOT be written to the CommandReceipt in the case of insufficient gas.
#[test]
fn test_short_circuit_insufficient_gas_for_withdraw_dep_ret_val() {
    let storage = SimulateWorldStateStorage::default();
    let (depositor_addr, bd, mut sws) = init_ws::<V1>(&storage);
    let operator_addr: PublicAddress = [2u8; 32];

    let total_pool_power = 100_000;
    let total_deposit_amt = 100_000;
    let withdraw_amt = 40_000;

    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    pool.set_operator(operator_addr);
    pool.set_power(total_pool_power);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    deposit.set_balance(total_deposit_amt);
    deposit.set_auto_stake_rewards(false);
    NetworkAccount::pools(&mut sws, operator_addr)
        .delegated_stakes()
        .insert(StakeValue::new(Stake {
            owner: depositor_addr,
            power: total_deposit_amt,
        }))
        .unwrap();

    let sws_first_run: SimulateWorldState<'_, V1> = sws.into();
    let sws_second_run: SimulateWorldState<'_, V1> = sws_first_run.clone();

    // expected gas costs
    let tx_inclusion_cost_v1 = 133_530;
    let cmd_cost = 362_780;

    //
    // 1. issue a WithdrawDeposit command with sufficient gas
    //
    let tx = TransactionV1 {
        commands: vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: operator_addr,
            max_amount: withdraw_amt,
        })],
        nonce: 0,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v1 + cmd_cost,
        ..TestData::transaction_v1()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v1(sws_first_run.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), cmd_cost);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::Success);
    assert_eq!(
        CallResult::parse::<u64>(receipt.last().unwrap().return_values.clone()),
        Some(withdraw_amt)
    );
    let mut sws: SimulateWorldState<'_, V1> = result.new_state.into();
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    assert_eq!(deposit.balance().unwrap(), total_deposit_amt - withdraw_amt);

    let delegated_stake = NetworkAccount::pools(&mut sws, operator_addr)
        .delegated_stakes()
        .get_by(&depositor_addr)
        .unwrap();
    assert_eq!(delegated_stake.owner, depositor_addr);
    assert_eq!(delegated_stake.power, total_deposit_amt - withdraw_amt);

    //
    // 2. issue a WithdrawDeposit command with insufficient gas
    //
    let tx = TransactionV1 {
        commands: vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: operator_addr,
            max_amount: withdraw_amt,
        })],
        nonce: 0,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v1 + cmd_cost - 1,
        ..TestData::transaction_v1()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v1(sws_second_run.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), cmd_cost - 1);
    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(
        CallResult::parse::<u64>(receipt.last().unwrap().return_values.clone()),
        None
    );

    let mut sws: SimulateWorldState<'_, V1> = result.new_state.into();
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    assert_eq!(deposit.balance().unwrap(), total_deposit_amt);
    let delegated_stake = NetworkAccount::pools(&mut sws, operator_addr)
        .delegated_stakes()
        .get_by(&depositor_addr)
        .unwrap();
    assert_eq!(delegated_stake.owner, depositor_addr);
    assert_eq!(delegated_stake.power, total_deposit_amt);
}

//
//
//
// Call Command
//
//
//
/// Return values should NOT be written to CommandReceipts if doing so breaches the gas limit.
///
/// If there is insufficient gas to complete the return value operation, gas should be consumed to the point of full exhaustion,
/// but without writing the return value to the CommandReceipt.
/// Return values written prior to gas exhaustion should be preserved in the CommandReceipt.
#[test]
fn test_short_circuit_insufficient_gas_for_return_value() {
    let storage = SimulateWorldStateStorage::default();
    let (signer_address, bd, sws) = init_ws::<V1>(&storage);
    //
    // 0. set up and deploy contract
    //
    let mut nonce: u64 = 0;
    let contract_code = TestData::get_test_contract_code("basic_contract");
    let contract_address = contract_address_v1(&signer_address, 0);
    let mut tx = TestData::transaction_v1();
    tx.gas_limit = 400_000_000;
    tx.commands = vec![ArgsBuilder::new().make_deploy(contract_code, nonce as u32)];

    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 121925230);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_code,
        ExitCodeV1::Success
    );
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

    // expected gas costs
    let pre_calc_inclusion_cost = 134520;
    let exact_cmd_gas_write_ret_val = 1258152; // total cmd cost - gas incurred after ret val cmd
    let just_enough_gas_to_write_ret = pre_calc_inclusion_cost + exact_cmd_gas_write_ret_val;

    //
    // 1. call with just enough gas to return the value
    // even though insufficient sufficient to complete the full command
    //
    nonce += 1;
    let tx = TransactionV1 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "get_init_state_without_self",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_write_ret,
        ..TestData::transaction_v1()
    };
    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());

    // sufficient gas to return value, but not to complete full command
    assert_eq!(extract_gas_used(&result), exact_cmd_gas_write_ret_val);

    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(
        CallResult::parse::<i32>(receipt.last().unwrap().return_values.clone()),
        Some(0)
    );
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

    //
    // 2. call with 1 gas less than the gas required to return the value
    //
    nonce += 1;
    let tx = TransactionV1 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "get_init_state_without_self",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_write_ret - 1,
        ..TestData::transaction_v1()
    };

    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), exact_cmd_gas_write_ret_val - 1);

    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(
        CallResult::parse::<i32>(receipt.last().unwrap().return_values.clone()),
        None
    );
}

/// Logs should NOT be written to CommandReceipt if doing so breaches the gas limit.
///
/// If there is insufficient gas to complete the Log operation, gas should be consumed to the point of full exhaustion,
/// but withiout writing the log to the CommandReceipt.
/// Logs written prior to gas exhaustion should be preserved in the CommandReceipt.
#[test]
fn test_short_circuit_insufficient_gas_for_logs() {
    let storage = SimulateWorldStateStorage::default();
    let (signer_address, bd, sws) = init_ws::<V1>(&storage);
    //
    // 0. set up and deploy contract
    //
    let mut nonce: u64 = 0;
    let contract_code = TestData::get_test_contract_code("basic_contract");
    let contract_address = contract_address_v1(&signer_address, 0);
    let mut tx = TestData::transaction_v1();
    tx.gas_limit = 400_000_000;
    tx.commands = vec![ArgsBuilder::new().make_deploy(contract_code, nonce as u32)];

    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), 121925230);
    assert_eq!(
        result.receipt.unwrap().last().unwrap().exit_code,
        ExitCodeV1::Success
    );
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

    // expected gas costs
    let pre_calc_inclusion_cost = 134460;
    let exact_gas_cmd_write_log = 1259201; // cost of executing right up to the log operation, there are still some Wasm opcodes after the host call...
    let just_enough_gas_to_log_v1 = pre_calc_inclusion_cost + exact_gas_cmd_write_log;

    //
    // 1. call with just enough gas to log
    // even though insufficient sufficient to complete the full command
    //
    println!("Calling with sufficient gas to write log...");
    nonce += 1;
    let tx = TransactionV1 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "emit_event_without_return",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_log_v1,
        ..TestData::transaction_v1()
    };
    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), exact_gas_cmd_write_log);

    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(receipt.last().unwrap().logs.len(), 1);
    assert!(receipt.last().unwrap().logs.iter().any(|e| {
        e.topic == format!("topic: basic").as_bytes()
            && e.value == format!("Hello, Contract").as_bytes()
    }));
    let sws: SimulateWorldState<'_, V1> = result.new_state.into();

    //
    // 2. call with 1 gas less than the gas required to return the value
    //
    println!("Calling with sufficient gas to write log...");
    nonce += 1;
    let tx = TransactionV1 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "emit_event_without_return",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_log_v1 - 1,
        ..TestData::transaction_v1()
    };
    let result = pchain_runtime::Runtime::new().transition_v1(sws.world_state, tx, bd.clone());
    assert_eq!(extract_gas_used(&result), exact_gas_cmd_write_log - 1);

    let receipt = result.receipt.unwrap();
    assert_eq!(receipt.last().unwrap().exit_code, ExitCodeV1::GasExhausted);
    assert_eq!(receipt.last().unwrap().logs.len(), 0);
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
/// When executing a StakeDeposit Command, the return value should NOT be written to the CommandReceipt in the case of insufficient gas.
#[test]
fn test_short_circuit_insufficient_gas_stake_deposit_ret_val_v2() {
    let storage = SimulateWorldStateStorage::default();
    let (depositor_addr, bd, mut sws) = init_ws::<V2>(&storage);
    let operator_addr: PublicAddress = [2u8; 32];

    let nonce: u64 = 0;
    let starting_operator_power = 100_000;
    let stake_amount = 20_000;

    //
    // 0. set up a Pool and Operator
    //
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    pool.set_operator(operator_addr);
    pool.set_power(starting_operator_power);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    deposit.set_balance(stake_amount);
    deposit.set_auto_stake_rewards(false);

    let sws_first_run: SimulateWorldState<'_, V2> = sws.into();
    let sws_second_run: SimulateWorldState<'_, V2> = sws_first_run.clone();

    // expected gas costs
    let stake_deposit_cmd_cost = 342_370;
    let total_gas_cost = 476_170;

    //
    // 1. issue a StakeDeposit command with sufficient gas to stake
    //
    let tx = TransactionV2 {
        commands: vec![Command::StakeDeposit(StakeDepositInput {
            operator: operator_addr,
            max_amount: stake_amount,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: total_gas_cost,
        ..TestData::transaction_v2()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v2(sws_first_run.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        total_gas_cost,
        stake_deposit_cmd_cost,
        ExitCodeV2::Ok,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::StakeDeposit(cr) => cr,
        _ => panic!("Expected StakeDeposit command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::Ok);
    assert_eq!(cr.amount_staked, stake_amount);
    let mut sws: SimulateWorldState<'_, V2> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(
        pool.power().unwrap(),
        starting_operator_power + stake_amount
    );
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr).unwrap();
    assert_eq!(delegated_stake.power, stake_amount);

    //
    // 2. issue a StakeDeposit command with insufficient gas
    //
    let tx = TransactionV2 {
        commands: vec![Command::StakeDeposit(StakeDepositInput {
            operator: operator_addr,
            max_amount: stake_amount,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: total_gas_cost - 1,
        ..TestData::transaction_v2()
    };
    let result = pchain_runtime::Runtime::new().transition_v2(sws_second_run.world_state, tx, bd);
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        total_gas_cost - 1,
        stake_deposit_cmd_cost - 1,
        ExitCodeV2::GasExhausted,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::StakeDeposit(cr) => cr,
        _ => panic!("Expected StakeDeposit command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    assert_eq!(cr.amount_staked, 0);
    let mut sws: SimulateWorldState<'_, V2> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(pool.power().unwrap(), starting_operator_power);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr);
    assert!(delegated_stake.is_none());
}

/// When executing a StakeDeposit Command, the logs should NOT be written to the CommandReceipt in the case of insufficient gas.
#[test]
fn test_short_circuit_insufficient_gas_unstake_deposit_ret_val_v2() {
    let storage = SimulateWorldStateStorage::default();
    let (depositor_addr, bd, mut sws) = init_ws::<V2>(&storage);
    let operator_addr: PublicAddress = [2u8; 32];

    let nonce: u64 = 0;
    let total_pool_power = 100_000;
    let initial_deposit_amt = 80_000;
    let depositor_stake_amt = 50_000;
    let unstake_amt = 40_000;
    //
    // 0. set up a Pool and Operator
    //
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    pool.set_operator(operator_addr);
    pool.set_power(total_pool_power);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    pool.delegated_stakes()
        .insert(StakeValue::new(Stake {
            owner: depositor_addr,
            power: depositor_stake_amt,
        }))
        .unwrap();
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    deposit.set_balance(initial_deposit_amt);
    deposit.set_auto_stake_rewards(false);

    let sws_first_run: SimulateWorldState<'_, V2> = sws.into();
    let sws_second_run: SimulateWorldState<'_, V2> = sws_first_run.clone();

    // expected gas costs
    let tx_inclusion_cost_v2 = 133_800;
    let cmd_cost = 275_464;

    //
    // 1. issue an UnstakeDeposit command with sufficient gas to stake
    //
    let tx = TransactionV2 {
        commands: vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: operator_addr,
            max_amount: unstake_amt,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v2 + cmd_cost,
        ..TestData::transaction_v2()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v2(sws_first_run.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        tx_inclusion_cost_v2 + cmd_cost,
        cmd_cost,
        ExitCodeV2::Ok,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::UnstakeDeposit(cr) => cr,
        _ => panic!("Expected UnstakeDeposit command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::Ok);
    assert_eq!(cr.amount_unstaked, unstake_amt);
    let mut sws: SimulateWorldState<'_, V2> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(pool.power().unwrap(), total_pool_power - unstake_amt);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr).unwrap();
    assert_eq!(delegated_stake.power, depositor_stake_amt - unstake_amt);

    //
    // 2. issue a UnstakeDeposit command with insufficient gas
    //
    let tx = TransactionV2 {
        commands: vec![Command::UnstakeDeposit(UnstakeDepositInput {
            operator: operator_addr,
            max_amount: unstake_amt,
        })],
        nonce,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v2 + cmd_cost - 1,
        ..TestData::transaction_v2()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v2(sws_second_run.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        tx_inclusion_cost_v2 + cmd_cost - 1,
        cmd_cost - 1,
        ExitCodeV2::GasExhausted,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::UnstakeDeposit(cr) => cr,
        _ => panic!("Expected UnstakeDeposit command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    assert_eq!(cr.amount_unstaked, 0);
    let mut sws: SimulateWorldState<'_, V2> = result.new_state.into();
    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    assert_eq!(pool.power().unwrap(), total_pool_power);
    let mut delegated_stakes = pool.delegated_stakes();
    let delegated_stake = delegated_stakes.get_by(&depositor_addr).unwrap();
    assert_eq!(delegated_stake.power, depositor_stake_amt);
}

/// When executing a WithdrawDeposit Command, the logs should NOT be written to the CommandReceipt in the case of insufficient gas.
#[test]
fn test_short_circuit_insufficient_gas_for_withdraw_dep_ret_val_v2() {
    let storage = SimulateWorldStateStorage::default();
    let (depositor_addr, bd, mut sws) = init_ws::<V2>(&storage);
    let operator_addr: PublicAddress = [2u8; 32];

    let total_pool_power = 100_000;
    let total_deposit_amt = 100_000;
    let withdraw_amt = 40_000;

    let mut pool = NetworkAccount::pools(&mut sws, operator_addr);
    pool.set_operator(operator_addr);
    pool.set_power(total_pool_power);
    pool.set_commission_rate(1);
    pool.set_operator_stake(None);
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    deposit.set_balance(total_deposit_amt);
    deposit.set_auto_stake_rewards(false);
    NetworkAccount::pools(&mut sws, operator_addr)
        .delegated_stakes()
        .insert(StakeValue::new(Stake {
            owner: depositor_addr,
            power: total_deposit_amt,
        }))
        .unwrap();

    let sws_first_run: SimulateWorldState<'_, V2> = sws.into();
    let sws_second_run: SimulateWorldState<'_, V2> = sws_first_run.clone();

    // expected gas costs
    let tx_inclusion_cost_v2 = 133_800;
    let cmd_cost = 317_680;

    //
    // 1. issue a WithdrawDeposit command with sufficient gas
    //
    let tx = TransactionV2 {
        commands: vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: operator_addr,
            max_amount: withdraw_amt,
        })],
        nonce: 0,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v2 + cmd_cost,
        ..TestData::transaction_v2()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v2(sws_first_run.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        tx_inclusion_cost_v2 + cmd_cost,
        cmd_cost,
        ExitCodeV2::Ok,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::WithdrawDeposit(cr) => cr,
        _ => panic!("Expected WithdrawDeposit command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::Ok);
    assert_eq!(cr.amount_withdrawn, withdraw_amt);
    let mut sws: SimulateWorldState<'_, V2> = result.new_state.into();
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    assert_eq!(deposit.balance().unwrap(), total_deposit_amt - withdraw_amt);
    let delegated_stake = NetworkAccount::pools(&mut sws, operator_addr)
        .delegated_stakes()
        .get_by(&depositor_addr)
        .unwrap();
    assert_eq!(delegated_stake.owner, depositor_addr);
    assert_eq!(delegated_stake.power, total_deposit_amt - withdraw_amt);

    //
    // 2. issue a WithdrawDeposit command with insufficient gas
    //
    let tx = TransactionV2 {
        commands: vec![Command::WithdrawDeposit(WithdrawDepositInput {
            operator: operator_addr,
            max_amount: withdraw_amt,
        })],
        nonce: 0,
        signer: depositor_addr,
        gas_limit: tx_inclusion_cost_v2 + cmd_cost - 1,
        ..TestData::transaction_v2()
    };
    let result =
        pchain_runtime::Runtime::new().transition_v2(sws_second_run.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        tx_inclusion_cost_v2 + cmd_cost - 1,
        cmd_cost - 1,
        ExitCodeV2::GasExhausted,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::WithdrawDeposit(cr) => cr,
        _ => panic!("Expected WithdrawDeposit command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    assert_eq!(cr.amount_withdrawn, 0);
    let mut sws: SimulateWorldState<'_, V2> = result.new_state.into();
    let mut deposit = NetworkAccount::deposits(&mut sws, operator_addr, depositor_addr);
    assert_eq!(deposit.balance().unwrap(), total_deposit_amt);
    let delegated_stake = NetworkAccount::pools(&mut sws, operator_addr)
        .delegated_stakes()
        .get_by(&depositor_addr)
        .unwrap();
    assert_eq!(delegated_stake.owner, depositor_addr);
    assert_eq!(delegated_stake.power, total_deposit_amt);
}

//
//
//
// Call Command
//
//
//
/// Return values should NOT be written to CommandReceipts if doing so breaches the gas limit.
///
/// If there is insufficient gas to complete the return value operation, gas should be consumed to the point of full exhaustion,
/// but without writing the return value to the CommandReceipt.
/// Return values written prior to gas exhaustion should be preserved in the CommandReceipt.
#[test]
fn test_short_circuit_insufficient_gas_for_return_value_v2() {
    let storage = SimulateWorldStateStorage::default();
    let (signer_address, bd, sws) = init_ws::<V2>(&storage);

    let mut nonce: u64 = 0;
    let contract_code = TestData::get_test_contract_code("basic_contract");
    let contract_address = contract_address_v2(&signer_address, 0, 0);
    let tx = TransactionV2 {
        gas_limit: 400_000_000,
        commands: vec![ArgsBuilder::new().make_deploy(contract_code, nonce as u32)],
        ..TestData::transaction_v2()
    };
    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    assert!(result.error.is_none());
    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    // exected gas costs
    let just_enough_gas_to_write_ret = 1392302;
    let exact_cmd_gas_write_ret_val = 1257512; // total cmd cost - gas incurred after ret val cmd

    //
    // 1. call with just enough gas to return the value
    // even though insufficient sufficient to complete the full command
    //
    println!("Calling with sufficient gas to write return value...");
    nonce += 1;
    let tx = TransactionV2 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "get_init_state_without_self",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_write_ret,
        ..TestData::transaction_v2()
    };
    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        just_enough_gas_to_write_ret,
        exact_cmd_gas_write_ret_val,
        ExitCodeV2::GasExhausted,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::Call(cr) => cr,
        _ => panic!("Expected Call command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    assert_eq!(CallResult::parse::<i32>(cr.return_value.clone()), Some(0));
    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    //
    // 2. call with 1 gas less than the gas required to return the value
    //
    println!("Calling with insufficient (previous - 1) gas to write return value...");
    nonce += 1;
    let tx = TransactionV2 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "get_init_state_without_self",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_write_ret - 1,
        ..TestData::transaction_v2()
    };

    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        just_enough_gas_to_write_ret - 1,
        exact_cmd_gas_write_ret_val - 1,
        ExitCodeV2::GasExhausted,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::Call(cr) => cr,
        _ => panic!("Expected Call command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    assert_eq!(CallResult::parse::<i32>(cr.return_value.clone()), None);
}

/// Logs should NOT be written to CommandReceipt if doing so breaches the gas limit.
///
/// If there is insufficient gas to complete the Log operation, gas should be consumed to the point of full exhaustion,
/// but withiout writing the log to the CommandReceipt.
/// Logs written prior to gas exhaustion should be preserved in the CommandReceipt.
#[test]
fn test_short_circuit_insufficient_gas_for_logs_v2() {
    let storage = SimulateWorldStateStorage::default();
    let (signer_address, bd, sws) = init_ws::<V2>(&storage);

    let contract_code = TestData::get_test_contract_code("basic_contract");
    let contract_address = contract_address_v2(&signer_address, 0, 0);
    let mut nonce: u64 = 0;
    let tx = TransactionV2 {
        gas_limit: 400_000_000,
        commands: vec![ArgsBuilder::new().make_deploy(contract_code, nonce as u32)],
        ..TestData::transaction_v2()
    };
    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    assert!(result.error.is_none());
    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    // expected gas costs of being able to emit log, excluding cost of any Wasm opcodes after
    let just_enough_gas_to_log_v2 = 1393931;
    let exact_gas_cmd_write_log = 1259201; // cost of executing right up to the log operation, there are still some Wasm opcodes after the host call...

    //
    // 1. call with just enough gas to return the value
    // even though insufficient sufficient to complete the full command
    //
    println!("Calling with sufficient gas to write return value...");
    nonce += 1;
    let tx = TransactionV2 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "emit_event_without_return",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_log_v2,
        ..TestData::transaction_v2()
    };
    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        just_enough_gas_to_log_v2,
        exact_gas_cmd_write_log,
        ExitCodeV2::GasExhausted,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::Call(cr) => cr,
        _ => panic!("Expected Call command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    assert_eq!(cr.logs.len(), 1);
    assert!(cr.logs.iter().any(|e| {
        e.topic == format!("topic: basic").as_bytes()
            && e.value == format!("Hello, Contract").as_bytes()
    }));
    let sws: SimulateWorldState<'_, V2> = result.new_state.into();

    //
    // 2. call with 1 gas less than the gas required to return the value
    //
    println!("Calling with insufficient (previous - 1) gas to write return value...");
    nonce += 1;
    let tx = TransactionV2 {
        commands: vec![ArgsBuilder::new().make_call(
            Some(0),
            contract_address,
            "emit_event_without_return",
        )],
        nonce,
        signer: signer_address,
        gas_limit: just_enough_gas_to_log_v2 - 1,
        ..TestData::transaction_v2()
    };
    let result = pchain_runtime::Runtime::new().transition_v2(sws.world_state, tx, bd.clone());
    let rcp = result.receipt.as_ref().expect("rcp should exist");
    assert!(verify_receipt_content_v2(
        rcp,
        just_enough_gas_to_log_v2 - 1,
        exact_gas_cmd_write_log - 1,
        ExitCodeV2::GasExhausted,
        0
    ));
    let cr = match rcp.command_receipts.last().expect("cr should exist") {
        CommandReceiptV2::Call(cr) => cr,
        _ => panic!("Expected Call command receipt"),
    };
    assert_eq!(cr.exit_code, ExitCodeV2::GasExhausted);
    assert_eq!(cr.logs.len(), 0);
}

fn init_ws<'a, V: VersionProvider + Send + Sync + Clone>(
    storage: &'a SimulateWorldStateStorage,
) -> (PublicAddress, BlockchainParams, SimulateWorldState<'a, V>) {
    let signer_address = [1u8; 32];
    let bd = TestData::block_params();
    let mut sws: SimulateWorldState<'_, V> = SimulateWorldState::new(storage);
    let init_from_balance = 500_000_000_000;
    sws.set_balance(signer_address, init_from_balance);
    (signer_address, bd, sws)
}
