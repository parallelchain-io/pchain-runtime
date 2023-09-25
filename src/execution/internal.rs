/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of internal transactions such as transferring tokens from contract and invoking another contract from contract.

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::storage::WorldStateStorage;
use std::sync::{Arc, Mutex};

use crate::{
    contract::{self, FuncError},
    cost::CostChange,
    transition::TransitionContext,
    types::CallTx,
    BlockchainParams,
};

use super::contract::ContractModule;

#[derive(Default)]
pub(crate) struct InternalCallResult {
    pub exec_gas: u64,
    pub non_wasmer_gas: CostChange,
    pub error: Option<FuncError>,
}

/// Execution logics for invoking another contract from a contract
pub(crate) fn call_from_contract<S>(
    mut tx_from_contract: CallTx,
    bd: BlockchainParams,
    txn_in_env: Arc<Mutex<TransitionContext<S>>>,
    call_counter: u32,
    is_view: bool,
) -> InternalCallResult
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    let mut ctx_locked = txn_in_env.lock().unwrap();
    let mut internal_call_result = InternalCallResult::default();

    // Transfer amount to address
    if let Some(value) = tx_from_contract.amount {
        let from_balance = ctx_locked.gas_meter.ws_get_balance(tx_from_contract.signer);
        if from_balance < value {
            internal_call_result.error = Some(contract::FuncError::InsufficientBalance);
            return internal_call_result;
        }
        let from_address_new_balance = from_balance - value;
        ctx_locked
            .gas_meter
            .ws_set_balance(tx_from_contract.signer, from_address_new_balance);

        // Safety: the balance of deployed contracts are always Some.
        let to_address_prev_balance = ctx_locked.gas_meter.ws_get_balance(tx_from_contract.target);
        let to_address_new_balance = to_address_prev_balance.saturating_add(value);
        ctx_locked
            .gas_meter
            .ws_set_balance(tx_from_contract.target, to_address_new_balance);
    }

    // Instantiate contract.
    let contract_module = match ContractModule::build_contract(tx_from_contract.target, &ctx_locked)
    {
        Ok(module) => module,
        Err(_) => {
            internal_call_result.error = Some(contract::FuncError::ContractNotFound);
            return internal_call_result;
        }
    };
    drop(ctx_locked);

    // limit the gas for child contract execution
    tx_from_contract.gas_limit = tx_from_contract
        .gas_limit
        .saturating_sub(internal_call_result.non_wasmer_gas.values().0);

    let instance = match contract_module.instantiate(
        txn_in_env,
        call_counter,
        is_view,
        tx_from_contract,
        bd,
    ) {
        Ok(instance) => instance,
        Err(_) => {
            internal_call_result.error = Some(contract::FuncError::ContractNotFound);
            return internal_call_result;
        }
    };

    // Call the contract
    let (_, gas_consumed, call_error) = instance.call();
    internal_call_result.exec_gas = gas_consumed;

    if let Some(call_error) = call_error {
        internal_call_result.error = Some(contract::FuncError::MethodCallError(call_error));
    }
    internal_call_result
}

/// Execution logics for transferring tokens from a contract
pub(crate) fn transfer_from_contract<S>(
    signer: PublicAddress,
    amount: u64,
    recipient: PublicAddress,
    txn_in_env: Arc<Mutex<TransitionContext<S>>>,
) -> InternalCallResult
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let mut ctx_locked = txn_in_env.lock().unwrap();
    let mut internal_call_result = InternalCallResult::default();

    // 1. Verify that the caller's balance is >= amount
    let from_balance = ctx_locked.gas_meter.ws_get_balance(signer);
    if from_balance < amount {
        internal_call_result.error = Some(contract::FuncError::InsufficientBalance);
        return internal_call_result;
    }

    // 2. Debit amount from from_address.
    let from_address_new_balance = from_balance - amount;
    ctx_locked
        .gas_meter
        .ws_set_balance(signer, from_address_new_balance);

    // 3. Credit amount to recipient.
    let to_address_prev_balance = ctx_locked.gas_meter.ws_get_balance(recipient);
    let to_address_new_balance = to_address_prev_balance.saturating_add(amount);
    ctx_locked
        .gas_meter
        .ws_set_balance(recipient, to_address_new_balance);

    internal_call_result
}
