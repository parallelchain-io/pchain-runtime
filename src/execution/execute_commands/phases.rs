/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines the behaviour of common phases during command execution.
//!
//! Used in the [command executor functions](crate::execution::execute_commands)
//!
//! The Common Phases include:
//!
//! - [Pre-Charge](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#pre-charge)
//! which validates whether transactions are eligible for inclusion in the block,
//! and charges the maximum-allowable gas fee from the transaction's signer, before actual execution.
//!
//! - [Charge](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#charge)
//! refunds any amount of remaining gas to the signer, and transfers the gas fee to the proposer and the treasury.

use pchain_world_state::{VersionProvider, DB};

use crate::{
    execution::state::ExecutionState,
    rewards_formulas::{TREASURY_CUT_OF_BASE_FEE_DENOM, TREASURY_CUT_OF_BASE_FEE_NUM},
    TransitionError,
};

/// Execute the pre-Charge phase and aborts on error.
pub(crate) fn pre_charge<S, E, V>(
    state: &mut ExecutionState<S, E, V>,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    state.ctx.gas_meter.charge_txn_pre_exec_inclusion(
        state.txn_meta.version,
        state.txn_meta.size,
        &state.txn_meta.command_kinds,
    )?;

    // note, remaining reads/ writes are performed directly on WS
    // not through GasMeter, hence not chargeable
    // because they are internal housekeeping and not part of the txn execution

    let signer = state.txn_meta.signer;
    let ws_cache = state.ctx.gas_free_ws_cache_mut();

    let origin_nonce = ws_cache.ws.account_trie().nonce(&signer).expect(&format!(
        "Account trie should get CBI version for {:?}",
        signer
    ));
    if state.txn_meta.nonce != origin_nonce {
        return Err(TransitionError::WrongNonce);
    }

    let origin_balance = ws_cache
        .ws
        .account_trie()
        .balance(&signer)
        .expect(&format!("Account trie should get balance for {:?}", signer));

    let gas_limit = state.txn_meta.gas_limit;
    let base_fee = state.bd.this_base_fee;
    let priority_fee = state.txn_meta.priority_fee_per_gas;

    // pre_charge = gas_limit * (base_fee + priority_fee)
    let pre_charge = base_fee
        .checked_add(priority_fee)
        .and_then(|fee| gas_limit.checked_mul(fee))
        .ok_or(TransitionError::NotEnoughBalanceForGasLimit)?; // Overflow check

    // pre_charged_balance = origin_balance - pre_charge
    let pre_charged_balance = origin_balance
        .checked_sub(pre_charge)
        .ok_or(TransitionError::NotEnoughBalanceForGasLimit)?; // pre_charge > origin_balance

    ws_cache
        .ws
        .account_trie_mut()
        .set_balance(&signer, pre_charged_balance)
        .expect(&format!("Account trie should set balance for {:?}", signer));

    Ok(())
}

/// Execute the Charge phase and updates relevant account balances
/// returns the final Execution state
/// # Panics
/// Will panic if the relevant account balances fail to be updated correctly due to an invalid World State.
pub(crate) fn charge<S, E, V>(mut state: ExecutionState<S, E, V>) -> ExecutionState<S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let signer = state.txn_meta.signer;
    let base_fee = state.bd.this_base_fee;
    let priority_fee = state.txn_meta.priority_fee_per_gas;

    let gas_used = std::cmp::min(
        state.ctx.gas_meter.total_gas_used_for_executed_commands(),
        state.txn_meta.gas_limit,
    );
    let gas_unused = state.txn_meta.gas_limit.saturating_sub(gas_used); // Safety for avoiding underflow

    let ws_cache = state.ctx.gas_free_ws_cache_mut();

    // Finalize signer's balance
    let signer_balance = ws_cache.purge_balance(signer);
    let new_signer_balance = signer_balance + gas_unused * (base_fee + priority_fee);

    // Transfer priority fee to Proposer
    let proposer_address = state.bd.proposer_address;
    let mut proposer_balance = ws_cache.purge_balance(proposer_address);
    if signer == proposer_address {
        proposer_balance = new_signer_balance;
    }
    let new_proposer_balance = proposer_balance.saturating_add(gas_used * priority_fee);

    // Burn the gas to Treasury account
    let treasury_address = state.bd.treasury_address;
    let mut treasury_balance = ws_cache.purge_balance(treasury_address);
    if signer == treasury_address {
        treasury_balance = new_signer_balance;
    }
    if proposer_address == treasury_address {
        treasury_balance = new_proposer_balance;
    }
    let new_treasury_balance = treasury_balance.saturating_add(
        (gas_used * base_fee * TREASURY_CUT_OF_BASE_FEE_NUM) / TREASURY_CUT_OF_BASE_FEE_DENOM,
    );

    // Commit updated balances
    ws_cache
        .ws
        .account_trie_mut()
        .set_balance(&signer, new_signer_balance)
        .expect(&format!("Account trie should set balance for {:?}", signer));
    ws_cache
        .ws
        .account_trie_mut()
        .set_balance(&proposer_address, new_proposer_balance)
        .expect(&format!(
            "Account trie should set balance for {:?}",
            proposer_address
        ));
    ws_cache
        .ws
        .account_trie_mut()
        .set_balance(&treasury_address, new_treasury_balance)
        .expect(&format!(
            "Account trie should set balance for {:?}",
            treasury_address
        ));

    // Commit Signer's Nonce
    let nonce = ws_cache
        .ws
        .account_trie()
        .nonce(&signer)
        .expect(&format!("Account trie should get nonce for {:?}", signer))
        .saturating_add(1);

    ws_cache
        .ws
        .account_trie_mut()
        .set_nonce(&signer, nonce)
        .expect(&format!("Account trie should set nonce for {:?}", signer));

    state
}
