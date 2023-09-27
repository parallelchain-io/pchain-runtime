/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines structures and functions which are useful in state transition across common phases.
//!
//! Common Phases include:
//! - [Pre-Charge](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#pre-charge): simple checks to ensure
//! transaction can be included in a block.
//! - [Charge](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#charge): refunds the amount of gas charged
//! in the pre-charge step that wasn't used in the transaction's execution. It then transfers fee to the proposer and the treasury.
//!
//! The actual command execution happens in Commands Phase. It is implemented in modules [account](crate::execution::account) and
//! [protocol](crate::execution::protocol).

use pchain_world_state::{
    network::network_account::NetworkAccountStorage, storage::WorldStateStorage,
};

use crate::{
    formulas::{TOTAL_BASE_FEE, TREASURY_CUT_OF_BASE_FEE},
    transition::StateChangesResult,
    TransitionError,
};

use super::state::ExecutionState;

/// Pre-Charge is a Phase in State Transition. It transits state and returns gas consumption if success.
pub(crate) fn pre_charge<S>(state: &mut ExecutionState<S>) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    state
        .ctx
        .gas_meter
        .charge_txn_pre_exec_inclusion(state.tx_size, state.commands_len)?;

    // note, remaining reads/ writes are performed directly on WS
    // not through GasMeter, hence not chargeable
    // because they are internal housekeeping and not part of the txn execution

    let signer = state.tx.signer;
    let mut rw_set = state.rw_set.lock().unwrap();

    let origin_nonce = rw_set.ws.nonce(signer);
    if state.tx.nonce != origin_nonce {
        return Err(TransitionError::WrongNonce);
    }

    let origin_balance = rw_set.ws.balance(signer);
    let gas_limit = state.tx.gas_limit;
    let base_fee = state.bd.this_base_fee;
    let priority_fee = state.tx.priority_fee_per_gas;

    // pre_charge = gas_limit * (base_fee + priority_fee)
    let pre_charge = base_fee
        .checked_add(priority_fee)
        .and_then(|fee| gas_limit.checked_mul(fee))
        .ok_or(TransitionError::NotEnoughBalanceForGasLimit)?; // Overflow check

    // pre_charged_balance = origin_balance - pre_charge
    let pre_charged_balance = origin_balance
        .checked_sub(pre_charge)
        .ok_or(TransitionError::NotEnoughBalanceForGasLimit)?; // pre_charge > origin_balance

    rw_set
        .ws
        .with_commit()
        .set_balance(signer, pre_charged_balance);
    drop(rw_set);

    Ok(())
}

/// finalize gas consumption of this Command Phase. Return Error GasExhaust if gas has already been exhausted
pub(crate) fn finalize_gas_consumption<S>(
    mut state: ExecutionState<S>,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static,
{
    // TODO 6 - check GasExhausted against centralized `gas_limit` field instead of other fields
    // should centralize gas checking in GasMeter
    // may need to tackle with lifecycle change
    if state.tx.gas_limit < state.ctx.gas_meter.get_gas_to_be_used_in_theory() {
        return Err(abort(state, TransitionError::ExecutionProperGasExhausted));
    }
    Ok(state)
}

/// Abort is operation that causes all World State sets in the Commands Phase to be reverted.
pub(crate) fn abort<S>(
    mut state: ExecutionState<S>,
    transition_err: TransitionError,
) -> StateChangesResult<S>
where
    S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static,
{
    state.revert_changes();
    // // TODO 4 - temp keeping the total_gas_used_clamped field, but should remove if no use
    let gas_used = std::cmp::min(
        state.tx.gas_limit,
        state.ctx.gas_meter.get_gas_to_be_used_in_theory(),
    );

    // //
    // // technically the Charge phase resolves this again by doing min comparison
    // // why need to store?
    state.ctx.gas_meter.total_gas_used_clamped = gas_used;

    charge(state, Some(transition_err))
}

/// Charge is a Phase in State Transition. It finalizes balance of accounts to world state.
pub(crate) fn charge<S>(
    mut state: ExecutionState<S>,
    transition_result: Option<TransitionError>,
) -> StateChangesResult<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    let signer = state.tx.signer;
    let base_fee = state.bd.this_base_fee;
    let priority_fee = state.tx.priority_fee_per_gas;
    // let gas_used = state.gas_meter.get_gas_already_used();
    // TODO 4 remove
    let gas_used = std::cmp::min(
        state.gas_meter.get_gas_to_be_used_in_theory(),
        state.tx.gas_limit,
    ); // Safety for avoiding overflow
    let gas_unused = state.tx.gas_limit.saturating_sub(gas_used);

    let mut rw_set = state.rw_set.lock().unwrap();

    // Finalize signer's balance
    let signer_balance = rw_set.purge_balance(signer);
    let new_signer_balance = signer_balance + gas_unused * (base_fee + priority_fee);

    // Transfer priority fee to Proposer
    let proposer_address = state.bd.proposer_address;
    let mut proposer_balance = rw_set.purge_balance(proposer_address);
    if signer == proposer_address {
        proposer_balance = new_signer_balance;
    }
    let new_proposer_balance = proposer_balance.saturating_add(gas_used * priority_fee);

    // Burn the gas to Treasury account
    let treasury_address = state.bd.treasury_address;
    let mut treasury_balance = rw_set.purge_balance(treasury_address);
    if signer == treasury_address {
        treasury_balance = new_signer_balance;
    }
    if proposer_address == treasury_address {
        treasury_balance = new_proposer_balance;
    }
    let new_treasury_balance = treasury_balance
        .saturating_add((gas_used * base_fee * TREASURY_CUT_OF_BASE_FEE) / TOTAL_BASE_FEE);

    // Commit updated balances
    rw_set
        .ws
        .with_commit()
        .set_balance(signer, new_signer_balance);
    rw_set
        .ws
        .with_commit()
        .set_balance(proposer_address, new_proposer_balance);
    rw_set
        .ws
        .with_commit()
        .set_balance(treasury_address, new_treasury_balance);

    // Commit Signer's Nonce
    let nonce = rw_set.ws.nonce(signer).saturating_add(1);
    rw_set.ws.with_commit().set_nonce(signer, nonce);
    drop(rw_set);

    // TODO 4 - temp keeping the total_gas_used_clamped field, but should remove if no use
    // CY: this looks like it's really not being used, even though the old code was saving it to state
    //
    // old code
    // state.set_gas_consumed(gas_used);
    // state.gas_meter.total_gas_used_clamped = gas_used;

    StateChangesResult::new(state, transition_result)
}
