/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Internal transactions includes transfering tokens from contract and invoking another contract from contract.

use std::{sync::{Arc, Mutex}};
use pchain_types::PublicAddress;
use pchain_world_state::{storage::WorldStateStorage};

use crate::{
    transition::TransitionContext, 
    contract::{self, FuncError}, 
    gas::{CostChange}, 
    types::CallTx, BlockchainParams
};

use super::{phase::ContractModule};

#[derive(Default)]
pub(crate) struct InternalCallResult {
    pub exec_gas: u64,
    pub non_wasmer_gas: CostChange,
    pub error: Option<FuncError>
}

/// Execution logics for invoking another contract from a contract
pub(crate) fn call_from_contract<S>(
    mut tx_from_contract: CallTx,
    bd: BlockchainParams,
    txn_in_env: Arc<Mutex<TransitionContext<S>>>,
    call_counter: u32
) -> InternalCallResult 
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    let mut ctx_locked = txn_in_env.lock().unwrap();
    let mut internal_call_result = InternalCallResult::default();

    // Transfer amount to address
    if let Some(value) = tx_from_contract.amount {
        let (from_balance, cost_change) = ctx_locked.balance(tx_from_contract.signer);
        internal_call_result.non_wasmer_gas += cost_change;
        if from_balance < value {
            internal_call_result.error = Some(contract::FuncError::InsufficientBalance);
            return internal_call_result;
        }

        let from_address_new_balance = from_balance - value;
        let cost_change = ctx_locked.set_balance(tx_from_contract.signer, from_address_new_balance);
        internal_call_result.non_wasmer_gas += cost_change;

        // Safety: the balance of deployed contracts are always Some.
        let (to_address_prev_balance, cost_change) = ctx_locked.balance(tx_from_contract.target);
        internal_call_result.non_wasmer_gas += cost_change;
        let to_address_new_balance = to_address_prev_balance + value;
        let cost_change = ctx_locked.set_balance(tx_from_contract.target, to_address_new_balance);
        internal_call_result.non_wasmer_gas += cost_change;
    }

    // Instantiate contract.
    let contract_module = match ContractModule::build_contract(
        tx_from_contract.target,
        &ctx_locked.sc_context,
        &ctx_locked.rw_set,
    ) {
        Ok(module) => module,
        Err(_) => {
            internal_call_result.error = Some(contract::FuncError::ContractNotFound);
            return internal_call_result;
        }
    };
    internal_call_result.non_wasmer_gas += contract_module.gas_cost;
    drop(ctx_locked);
    
    // limit the gas for child contract execution
    tx_from_contract.gas_limit = tx_from_contract.gas_limit.saturating_sub(internal_call_result.non_wasmer_gas.values().0);

    let instance = match contract_module.instantiate(
        txn_in_env, 
        call_counter, 
        tx_from_contract, 
        bd
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

/// Execution logics for transfering tokens from a contract
pub(crate) fn transfer_from_contract<S>(
    signer: PublicAddress,
    amount: u64,
    recipient: PublicAddress,
    txn_in_env: Arc<Mutex<TransitionContext<S>>>
) -> InternalCallResult 
    where S: WorldStateStorage + Send + Sync + Clone
{
    let mut ctx_locked = txn_in_env.lock().unwrap();
    let mut internal_call_result = InternalCallResult::default();

    // 1. Verify that the caller's balance is >= value
    let (from_balance, cost_change) = ctx_locked.balance(signer);
    internal_call_result.non_wasmer_gas += cost_change;

    if from_balance < amount {
        internal_call_result.error = Some(contract::FuncError::InsufficientBalance);
        return internal_call_result;
    }

    // 2. Debit value from from_address.
    let from_address_new_balance = from_balance - amount;
    let cost_change = ctx_locked.set_balance(signer, from_address_new_balance);
    internal_call_result.non_wasmer_gas += cost_change;

    // 3. Credit value to target_address.
    let (to_address_prev_balance, cost_change) = ctx_locked.balance(recipient);
    internal_call_result.non_wasmer_gas += cost_change;
    let to_address_new_balance = to_address_prev_balance + amount;
    let cost_change = ctx_locked.set_balance(recipient, to_address_new_balance);
    internal_call_result.non_wasmer_gas += cost_change;

    internal_call_result
}