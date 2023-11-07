/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation for host functions used for contract methods according to [crate::contract::cbi].

use pchain_types::{
    blockchain::{Command, Log},
    cryptography::PublicAddress,
    runtime::CallInput,
    serialization::{Deserializable, Serializable},
};
use pchain_world_state::{VersionProvider, DB, NETWORK_ADDRESS};

use crate::{
    contract::{CBIHostFunctions, FuncError},
    execution::gas::WasmerGasMeter,
    types::{BaseTx, CallTx, DeferredCommand},
};

use super::wasmer::env::Env;

/// [HostFunctions] implements trait [CBIHostFunctions].
/// ### CBI version: 0
pub(crate) struct HostFunctions {}
impl<'a, 'b, S, V> CBIHostFunctions<Env<'a, S, V>> for HostFunctions
where
    S: DB + Sync + Send + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    fn set(
        env: &Env<'a, S, V>,
        key_ptr: u32,
        key_len: u32,
        val_ptr: u32,
        val_len: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let mut gas_meter = ctx.gas_meter();

        // let key = gas_meter.read_bytes(key_ptr, key_len)?;
        // let new_value = gas_meter.read_bytes(val_ptr, val_len)?;

        // gas_meter.ws_set_storage_data(env.call_tx.target, &key, new_value);

        Ok(())
    }

    fn get(
        env: &Env<'a, S, V>,
        key_ptr: u32,
        key_len: u32,
        val_ptr_ptr: u32,
    ) -> Result<i64, FuncError> {
        // let mut ctx = env.lock();

        // let gas_meter = ctx.gas_meter();

        // let key = gas_meter.read_bytes(key_ptr, key_len)?;
        // let value = gas_meter.ws_get_storage_data(env.call_tx.target, &key);

        // let ret_val = match value {
        //     Some(value) => gas_meter.write_bytes(value, val_ptr_ptr)? as i64,
        //     None => -1,
        // };

        // Ok(ret_val)

        Ok(1)
    }

    fn get_network_storage(
        env: &Env<'a, S, V>,
        key_ptr: u32,
        key_len: u32,
        val_ptr_ptr: u32,
    ) -> Result<i64, FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let key = gas_meter.read_bytes(key_ptr, key_len)?;
        // let value = gas_meter.ws_get_storage_data(NETWORK_ADDRESS, &key);

        // let ret_val = match value {
        //     Some(value) => gas_meter.write_bytes(value, val_ptr_ptr)? as i64,
        //     None => -1,
        // };

        // Ok(ret_val)

        Ok(1)
    }

    fn balance(env: &Env<'a, S, V>) -> Result<u64, FuncError> {
        // Ok(env.lock().gas_meter().ws_get_balance(env.call_tx.target))
        Ok(1)
    }

    fn block_height(env: &Env<'a, S, V>) -> Result<u64, FuncError> {
        Ok(env.params_from_blockchain.this_block_number)
    }
    fn block_timestamp(env: &Env<'a, S, V>) -> Result<u32, FuncError> {
        Ok(env.params_from_blockchain.timestamp)
    }
    fn prev_block_hash(env: &Env<'a, S, V>, hash_ptr_ptr: u32) -> Result<(), FuncError> {
        // env.lock()
        //     .gas_meter()
        //     .write_bytes(
        //         env.params_from_blockchain.prev_block_hash.to_vec(),
        //         hash_ptr_ptr,
        //     )
        //     .map(|_| ())
        //     .map_err(FuncError::Runtime)

        Ok(())
    }

    fn calling_account(env: &Env<'a, S, V>, address_ptr_ptr: u32) -> Result<(), FuncError> {
        // env.lock()
        //     .gas_meter()
        //     .write_bytes(env.call_tx.signer.to_vec(), address_ptr_ptr)
        //     .map(|_| ())
        //     .map_err(FuncError::Runtime)

        Ok(())
    }
    fn current_account(env: &Env<'a, S, V>, address_ptr_ptr: u32) -> Result<(), FuncError> {
        // env.lock()
        //     .gas_meter()
        //     .write_bytes(env.call_tx.target.to_vec(), address_ptr_ptr)
        //     .map(|_| ())
        //     .map_err(FuncError::Runtime)

        Ok(())
    }

    fn method(env: &Env<'a, S, V>, method_ptr_ptr: u32) -> Result<u32, FuncError> {
        // env.lock()
        //     .gas_meter()
        //     .write_bytes(env.call_tx.method.as_bytes().to_vec(), method_ptr_ptr)
        //     .map_err(FuncError::Runtime)
        Ok(1)
    }

    fn arguments(env: &Env<'a, S, V>, arguments_ptr_ptr: u32) -> Result<u32, FuncError> {
        // match &env.call_tx.arguments {
        //     Some(args) => {
        //         let arguments = <Vec<Vec<u8>> as Serializable>::serialize(args);
        //         env.lock()
        //             .gas_meter()
        //             .write_bytes(arguments, arguments_ptr_ptr)
        //             .map_err(FuncError::Runtime)
        //     }
        //     None => Ok(0),
        // }
        Ok(1)
    }

    fn amount(env: &Env<'a, S, V>) -> Result<u64, FuncError> {
        Ok(env.call_tx.amount.map_or(0, std::convert::identity))
    }

    fn is_internal_call(env: &Env<'a, S, V>) -> Result<i32, FuncError> {
        Ok(i32::from(env.call_counter != 0))
    }

    fn transaction_hash(env: &Env<'a, S, V>, hash_ptr_ptr: u32) -> Result<(), FuncError> {
        // env.lock()
        //     .gas_meter()
        //     .write_bytes(env.call_tx.hash.to_vec(), hash_ptr_ptr)
        //     .map(|_| ())
        //     .map_err(FuncError::Runtime)
        Ok(())
    }

    fn log(env: &Env<'a, S, V>, log_ptr: u32, log_len: u32) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let mut gas_meter = ctx.gas_meter();

        // let serialized_log = gas_meter.read_bytes(log_ptr, log_len)?;
        // let log = Log::deserialize(&serialized_log).map_err(|e| FuncError::Runtime(e.into()))?;
        // gas_meter.command_output_append_log(log);
        Ok(())
    }

    fn return_value(env: &Env<'a, S, V>, value_ptr: u32, value_len: u32) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let mut gas_meter = ctx.gas_meter();

        // let value = gas_meter.read_bytes(value_ptr, value_len)?;
        // gas_meter.command_output_set_return_value(value);
        Ok(())
    }

    fn call(
        env: &Env<'a, S, V>,
        call_input_ptr: u32,
        call_input_len: u32,
        return_ptr_ptr: u32,
    ) -> Result<u32, FuncError> {
        // TODO 96, investigate this,
        // I get the gist is locking the context  Arc<Mutex<TransitionContext<'a, S, V>>>,
        // albeit wrapped
        // let mut ctx = env.lock();
        // let sc_context = ctx.smart_contract_context();
        // let mut gas_meter = ctx.gas_meter();

        // // Parse the call command arguments
        // let (target, method, arguments, amount) = {
        //     let call_command_bytes = gas_meter
        //         .read_bytes(call_input_ptr, call_input_len)
        //         .map_err(FuncError::Runtime)?;
        //     let call_command = Command::deserialize(&call_command_bytes)
        //         .map_err(|e| FuncError::Runtime(e.into()))?;

        //     match call_command {
        //         Command::Call(CallInput {
        //             target,
        //             method,
        //             arguments,
        //             amount,
        //         }) => (target, method, arguments, amount),
        //         _ => return Err(FuncError::Internal),
        //     }
        // };

        // // error if transfer amount is specified in view call.
        // if env.is_view && amount.is_some() {
        //     return Err(FuncError::Internal);
        // }

        // // transfer from calling contract address (call_tx.target) to the target address first.
        // if let Some(amount) = amount {
        //     transfer_from_contract(env.call_tx.target, amount, target, &mut gas_meter)?;
        // }

        // // Get the Contract Code and create the contract module
        // let contract_module = gas_meter
        //     .ws_get_cached_contract(target, &sc_context)
        //     .ok_or(FuncError::ContractNotFound)?;

        // // by default, fields would be inherited from parent transaction
        // let call_tx = CallTx {
        //     base_tx: BaseTx {
        //         command_kinds: env.call_tx.command_kinds.clone(),
        //         signer: env.call_tx.target,
        //         gas_limit: gas_meter.remaining_gas(),
        //         ..env.call_tx.base_tx
        //     },
        //     amount,
        //     arguments,
        //     method,
        //     target,
        // };

        // drop(ctx); // Drop the transition context and pass it to child contract.

        // // Call the contract
        // let (_, gas_consumed, call_error) = contract_module
        //     .instantiate(
        //         env.context.clone(), // here we only clone the Arc
        //         env.call_counter.saturating_add(1),
        //         env.is_view,
        //         call_tx,
        //         env.params_from_blockchain.clone(),
        //     )
        //     .map_err(|_| FuncError::ContractNotFound)?
        //     .call();

        // let mut ctx = env.lock();
        // let mut gas_meter = ctx.gas_meter();

        // gas_meter.reduce_gas(gas_consumed); // subtract gas consumed from parent contract's environment

        // match call_error {
        //     None => {
        //         // Take the child result in parent's execution context.
        //         if let Some(res) = gas_meter.command_output_cache().take_return_value() {
        //             return gas_meter
        //                 .write_bytes(res, return_ptr_ptr)
        //                 .map_err(FuncError::Runtime);
        //         }
        //     }
        //     Some(e) => {
        //         if gas_meter.remaining_gas() == 0 {
        //             return Err(FuncError::GasExhaustionError);
        //         }
        //         return Err(FuncError::MethodCallError(e));
        //     }
        // }

        Ok(0)
    }

    fn transfer(env: &Env<'a, S, V>, transfer_input_ptr: u32) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let mut gas_meter = ctx.gas_meter();

        // let transfer_bytes = gas_meter
        //     .read_bytes(transfer_input_ptr, std::mem::size_of::<[u8; 40]>() as u32)
        //     .map_err(FuncError::Runtime)?;

        // // first 32-bytes are the recipient address, last 8 is the amount
        // let (recipient, amount_bytes) = transfer_bytes.split_at(32);
        // let recipient = recipient.try_into().unwrap();
        // let amount = u64::from_le_bytes(amount_bytes.try_into().unwrap());

        // transfer_from_contract(
        //     env.call_tx.target, // calling contract's address from transaction execution context
        //     amount,
        //     recipient,
        //     &mut gas_meter,
        // )

        Ok(())
    }

    fn defer_create_deposit(
        env: &Env<'a, S, V>,
        create_deposit_input_ptr: u32,
        create_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let serialized_command = gas_meter
        //     .read_bytes(create_deposit_input_ptr, create_deposit_input_len)
        //     .map_err(FuncError::Runtime)?;
        // let command =
        //     Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        // if !matches!(command, Command::CreateDeposit { .. }) {
        //     return Err(FuncError::Internal);
        // }

        // ctx.append_deferred_command(DeferredCommand {
        //     command,
        //     contract_address: env.call_tx.target,
        // });

        Ok(())
    }

    fn defer_set_deposit_settings(
        env: &Env<'a, S, V>,
        set_deposit_settings_input_ptr: u32,
        set_deposit_settings_input_len: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let serialized_command = gas_meter
        //     .read_bytes(
        //         set_deposit_settings_input_ptr,
        //         set_deposit_settings_input_len,
        //     )
        //     .map_err(FuncError::Runtime)?;
        // let command =
        //     Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        // if !matches!(command, Command::SetDepositSettings { .. }) {
        //     return Err(FuncError::Internal);
        // }

        // ctx.append_deferred_command(DeferredCommand {
        //     command,
        //     contract_address: env.call_tx.target,
        // });

        Ok(())
    }

    fn defer_topup_deposit(
        env: &Env<'a, S, V>,
        top_up_deposit_input_ptr: u32,
        top_up_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let serialized_command = gas_meter
        //     .read_bytes(top_up_deposit_input_ptr, top_up_deposit_input_len)
        //     .map_err(FuncError::Runtime)?;
        // let command =
        //     Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        // if !matches!(command, Command::TopUpDeposit { .. }) {
        //     return Err(FuncError::Internal);
        // }

        // ctx.append_deferred_command(DeferredCommand {
        //     command,
        //     contract_address: env.call_tx.target,
        // });

        Ok(())
    }

    fn defer_withdraw_deposit(
        env: &Env<'a, S, V>,
        withdraw_deposit_input_ptr: u32,
        withdraw_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let serialized_command = gas_meter
        //     .read_bytes(withdraw_deposit_input_ptr, withdraw_deposit_input_len)
        //     .map_err(FuncError::Runtime)?;
        // let command =
        //     Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        // if !matches!(command, Command::WithdrawDeposit { .. }) {
        //     return Err(FuncError::Internal);
        // }

        // ctx.append_deferred_command(DeferredCommand {
        //     command,
        //     contract_address: env.call_tx.target,
        // });

        Ok(())
    }

    fn defer_stake_deposit(
        env: &Env<'a, S, V>,
        stake_deposit_input_ptr: u32,
        stake_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let serialized_command = gas_meter
        //     .read_bytes(stake_deposit_input_ptr, stake_deposit_input_len)
        //     .map_err(FuncError::Runtime)?;
        // let command =
        //     Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        // if !matches!(command, Command::StakeDeposit { .. }) {
        //     return Err(FuncError::Internal);
        // }

        // ctx.append_deferred_command(DeferredCommand {
        //     command,
        //     contract_address: env.call_tx.target,
        // });

        Ok(())
    }

    fn defer_unstake_deposit(
        env: &Env<'a, S, V>,
        unstake_deposit_input_ptr: u32,
        unstake_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let serialized_command = gas_meter
        //     .read_bytes(unstake_deposit_input_ptr, unstake_deposit_input_len)
        //     .map_err(FuncError::Runtime)?;
        // let command =
        //     Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        // if !matches!(command, Command::UnstakeDeposit { .. }) {
        //     return Err(FuncError::Internal);
        // }

        // ctx.append_deferred_command(DeferredCommand {
        //     command,
        //     contract_address: env.call_tx.target,
        // });

        Ok(())
    }

    fn sha256(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        digest_ptr_ptr: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let input_bytes = gas_meter.read_bytes(msg_ptr, msg_len)?;
        // let digest = gas_meter.sha256(input_bytes);

        // gas_meter.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn keccak256(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        digest_ptr_ptr: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let input_bytes = gas_meter.read_bytes(msg_ptr, msg_len)?;
        // let digest = gas_meter.keccak256(input_bytes);

        // gas_meter.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn ripemd(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        digest_ptr_ptr: u32,
    ) -> Result<(), FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let input_bytes = gas_meter.read_bytes(msg_ptr, msg_len)?;
        // let digest = gas_meter.ripemd(input_bytes);

        // gas_meter.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn verify_ed25519_signature(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        signature_ptr: u32,
        address_ptr: u32,
    ) -> Result<i32, FuncError> {
        // let mut ctx = env.lock();
        // let gas_meter = ctx.gas_meter();

        // let message = gas_meter.read_bytes(msg_ptr, msg_len)?;
        // let signature = gas_meter.read_bytes(signature_ptr, 64)?;
        // let address = gas_meter.read_bytes(address_ptr, 32)?;

        // gas_meter
        //     .verify_ed25519_signature(
        //         message,
        //         signature.try_into().unwrap(),
        //         address.try_into().unwrap(),
        //     )
        //     .map_err(|_| FuncError::Internal)

        Ok(1)
    }
}

/// Execution logic for transferring tokens from a contract
fn transfer_from_contract<'a, 'lock, S, V>(
    signer: PublicAddress,
    amount: u64,
    recipient: PublicAddress,
    gas_meter: &mut WasmerGasMeter<'lock, 'a, S, Env<'a, S, V>, V>,
) -> Result<(), FuncError>
where
    S: DB + Send + Sync + Clone + 'a,
    V: VersionProvider + Send + Sync + Clone,
    // 'a: 'lock,
{
    // 1. Verify that the caller's balance is >= amount
    let from_balance = gas_meter.ws_get_balance(signer);
    let from_address_new_balance = from_balance
        .checked_sub(amount)
        .ok_or(FuncError::InsufficientBalance)?;

    // 2. Debit amount from from_address.
    gas_meter.ws_set_balance(signer, from_address_new_balance);

    // 3. Credit amount to recipient.
    let to_address_prev_balance = gas_meter.ws_get_balance(recipient);
    let to_address_new_balance = to_address_prev_balance.saturating_add(amount);
    gas_meter.ws_set_balance(recipient, to_address_new_balance);

    Ok(())
}
