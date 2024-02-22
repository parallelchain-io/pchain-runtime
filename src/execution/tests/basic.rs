/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
use pchain_types::{
    blockchain::{Command, ExitCodeV1, ExitCodeV2},
    runtime::TransferInput,
};

use crate::{
    execution::execute_commands::{execute_commands_v1, execute_commands_v2},
    gas::{
        get_cost_read, get_cost_traverse, set_cost_delete_old_value, set_cost_rehash,
        set_cost_write_new_value, ACCOUNT_TRIE_KEY_LENGTH,
    },
};

use super::test_utils::*;

/// Null test on empty transaction commands
#[test]
fn test_empty_commands() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));

    let owner_balance_before = state
        .ctx
        .gas_free_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();

    let tx_base_cost_v1 = set_tx_v1(&mut state, ACCOUNT_A, 0, &vec![]);
    assert_eq!(tx_base_cost_v1, 131790);

    let ret = execute_commands_v1(state, vec![]);
    assert_eq!((&ret.error, &ret.receipt), (&None, &Some(vec![])));
    let gas_used = extract_gas_used(&ret);
    assert_eq!(gas_used, 0);

    let state = create_state_v1(Some(ret.new_state));
    let owner_balance_after = state
        .ctx
        .gas_free_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();

    assert_eq!(
        owner_balance_before,
        owner_balance_after + gas_used + tx_base_cost_v1
    );
}

#[test]
// Commands Transfer
fn test_transfer() {
    let fixture = TestFixture::new();
    let mut state = create_state_v1(Some(fixture.ws()));

    let amount = 999_999;
    let commands = vec![Command::Transfer(TransferInput {
        recipient: ACCOUNT_B,
        amount,
    })];

    let tx_base_cost_v1 = set_tx_v1(&mut state, ACCOUNT_A, 0, &commands);
    assert_eq!(tx_base_cost_v1, 133530);

    let ret = execute_commands_v1(state, commands);

    assert_eq!(
        (
            &ret.error,
            &ret.receipt.as_ref().unwrap().last().unwrap().exit_code
        ),
        (&None, &ExitCodeV1::Success)
    );

    assert_eq!(extract_gas_used(&ret), xfer_cmd_cost());
    let sender_balance_after = ret.new_state.account_trie().balance(&ACCOUNT_A).unwrap();
    assert_eq!(
        sender_balance_after,
        DEFAULT_AMOUNT - amount - tx_base_cost_v1 - extract_gas_used(&ret)
    );

    let owner_balance_after = ret.new_state.account_trie().balance(&ACCOUNT_B).unwrap();
    assert_eq!(owner_balance_after, DEFAULT_AMOUNT + amount);
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

/// Null test on empty transaction commands
#[test]
fn test_empty_commands_v2() {
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));
    let owner_balance_before = state
        .ctx
        .gas_free_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();

    let tx_base_cost_v2 = set_tx_v2(&mut state, ACCOUNT_A, 0, &vec![]);
    assert_eq!(tx_base_cost_v2, 132060);

    let ret = execute_commands_v2(state, vec![]);
    assert!(ret.error.is_none());
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        tx_base_cost_v2,
        0,
        ExitCodeV2::Ok,
        0
    ));

    let state = create_state_v2(Some(ret.new_state));
    let owner_balance_after = state
        .ctx
        .gas_free_ws_cache()
        .ws
        .account_trie()
        .balance(&ACCOUNT_A)
        .unwrap();

    // no command gas used
    assert_eq!(owner_balance_before, owner_balance_after + tx_base_cost_v2);
}

#[test]
// Commands Transfer
fn test_transfer_v2() {
    let fixture = TestFixture::new();
    let mut state = create_state_v2(Some(fixture.ws()));

    let amount = 999_999;
    let commands = vec![Command::Transfer(TransferInput {
        recipient: ACCOUNT_B,
        amount,
    })];

    let tx_base_cost_v2 = set_tx_v2(&mut state, ACCOUNT_A, 0, &commands);
    assert_eq!(tx_base_cost_v2, 133560);
    let ret = execute_commands_v2(state, commands);
    assert!(ret.error.is_none());
    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        tx_base_cost_v2 + xfer_cmd_cost(),
        xfer_cmd_cost(),
        ExitCodeV2::Ok,
        0
    ));

    let owner_balance_after = ret.new_state.account_trie().balance(&ACCOUNT_B).unwrap();
    assert_eq!(owner_balance_after, 500_000_000 + amount);
}

//
//
//
//
//
// ↓↓↓ Assertion Helpers ↓↓↓ //
//
//
//
//
//
fn xfer_cmd_cost() -> u64 {
    const U64_LEN: usize = std::mem::size_of::<u64>();

    let get_sender_init_bal_cost =
        try_i64!(get_cost_traverse(ACCOUNT_TRIE_KEY_LENGTH)) + try_i64!(get_cost_read(U64_LEN));

    // note: the set will additionally perform another get
    let set_sender_new_bal_cost = get_sender_init_bal_cost
        + try_i64!(set_cost_write_new_value(U64_LEN))
        + try_i64!(set_cost_rehash(ACCOUNT_TRIE_KEY_LENGTH))
        - try_i64!(set_cost_delete_old_value(
            ACCOUNT_TRIE_KEY_LENGTH,
            U64_LEN,
            U64_LEN
        ));

    let get_recipient_init_bal_cost = get_sender_init_bal_cost; // similar operation

    let set_recipient_new_bal_cost = set_sender_new_bal_cost; // similar operation

    u64::try_from(
        get_sender_init_bal_cost
            + set_sender_new_bal_cost
            + get_recipient_init_bal_cost
            + set_recipient_new_bal_cost,
    )
    .expect("Expect gas cost to be a non-negative int")
}
