/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implements host functions defined in [crate::contract::cbi_host_functions].

use pchain_types::{
    blockchain::{Command, Log},
    cryptography::PublicAddress,
    runtime::CallInput,
    serialization::{Deserializable, Serializable},
};
use pchain_world_state::{VersionProvider, DB, NETWORK_ADDRESS};

use crate::{
    contract::{CBIHostFunctions, FuncError},
    execution::gas::HostFuncGasMeter,
    types::{BaseTx, CallTx, DeferredCommand},
};

use super::wasmer::env::Env;

/// [HostFunctions] implements trait [CBIHostFunctions] according to CBI version 0.
/// The Env struct is available by reference in every method to retrieve the current execution context.
/// Inside every method, we instantitate a new [HostFuncGasMeter] which holds the latest gas consumption state and
/// through it, call methods that require gas consumption.
pub(crate) struct HostFunctions {}
impl<'a, 'b, S, V> CBIHostFunctions<Env<'a, S, V>> for HostFunctions
where
    S: DB + Sync + Send + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    fn set(
        env: &Env<'a, S, V>,
        key_ptr: u32,
        key_len: u32,
        val_ptr: u32,
        val_len: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let key = fn_gas_meter.read_bytes(key_ptr, key_len)?;
        let new_value = fn_gas_meter.read_bytes(val_ptr, val_len)?;

        fn_gas_meter.ws_set_storage_data(env.call_tx.target, &key, new_value);

        Ok(())
    }

    fn get(
        env: &Env<'a, S, V>,
        key_ptr: u32,
        key_len: u32,
        val_ptr_ptr: u32,
    ) -> Result<i64, FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let key = fn_gas_meter.read_bytes(key_ptr, key_len)?;
        let value = fn_gas_meter.ws_get_storage_data(env.call_tx.target, &key);

        let ret_val = match value {
            Some(value) => fn_gas_meter.write_bytes(value, val_ptr_ptr)? as i64,
            None => -1,
        };

        Ok(ret_val)
    }

    fn get_network_storage(
        env: &Env<'a, S, V>,
        key_ptr: u32,
        key_len: u32,
        val_ptr_ptr: u32,
    ) -> Result<i64, FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let key = fn_gas_meter.read_bytes(key_ptr, key_len)?;
        let value = fn_gas_meter.ws_get_storage_data(NETWORK_ADDRESS, &key);

        let ret_val = match value {
            Some(value) => fn_gas_meter.write_bytes(value, val_ptr_ptr)? as i64,
            None => -1,
        };

        Ok(ret_val)
    }

    fn balance(env: &Env<'a, S, V>) -> Result<u64, FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);
        Ok(fn_gas_meter.ws_get_balance(env.call_tx.target))
    }

    fn block_height(env: &Env<'a, S, V>) -> Result<u64, FuncError> {
        Ok(env.params_from_blockchain.this_block_number)
    }
    fn block_timestamp(env: &Env<'a, S, V>) -> Result<u32, FuncError> {
        Ok(env.params_from_blockchain.timestamp)
    }
    fn prev_block_hash(env: &Env<'a, S, V>, hash_ptr_ptr: u32) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        fn_gas_meter
            .write_bytes(
                env.params_from_blockchain.prev_block_hash.to_vec(),
                hash_ptr_ptr,
            )
            .map(|_| ())
            .map_err(FuncError::Runtime)
    }

    fn calling_account(env: &Env<'a, S, V>, address_ptr_ptr: u32) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        fn_gas_meter
            .write_bytes(env.call_tx.signer.to_vec(), address_ptr_ptr)
            .map(|_| ())
            .map_err(FuncError::Runtime)
    }

    fn current_account(env: &Env<'a, S, V>, address_ptr_ptr: u32) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        fn_gas_meter
            .write_bytes(env.call_tx.target.to_vec(), address_ptr_ptr)
            .map(|_| ())
            .map_err(FuncError::Runtime)
    }

    fn method(env: &Env<'a, S, V>, method_ptr_ptr: u32) -> Result<u32, FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        fn_gas_meter
            .write_bytes(env.call_tx.method.as_bytes().to_vec(), method_ptr_ptr)
            .map_err(FuncError::Runtime)
    }

    fn arguments(env: &Env<'a, S, V>, arguments_ptr_ptr: u32) -> Result<u32, FuncError> {
        match &env.call_tx.arguments {
            Some(args) => {
                let arguments = <Vec<Vec<u8>> as Serializable>::serialize(args);
                let mut ctx = env.context.lock().unwrap();
                let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
                let fn_gas_meter =
                    HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

                fn_gas_meter
                    .write_bytes(arguments, arguments_ptr_ptr)
                    .map_err(FuncError::Runtime)
            }
            None => Ok(0),
        }
    }

    fn amount(env: &Env<'a, S, V>) -> Result<u64, FuncError> {
        Ok(env.call_tx.amount.map_or(0, std::convert::identity))
    }

    fn is_internal_call(env: &Env<'a, S, V>) -> Result<i32, FuncError> {
        Ok(i32::from(env.call_counter != 0))
    }

    fn transaction_hash(env: &Env<'a, S, V>, hash_ptr_ptr: u32) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        fn_gas_meter
            .write_bytes(env.call_tx.hash.to_vec(), hash_ptr_ptr)
            .map(|_| ())
            .map_err(FuncError::Runtime)
    }

    fn log(env: &Env<'a, S, V>, log_ptr: u32, log_len: u32) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let serialized_log = fn_gas_meter.read_bytes(log_ptr, log_len)?;
        let log = Log::deserialize(&serialized_log).map_err(|e| FuncError::Runtime(e.into()))?;
        fn_gas_meter.command_output_append_log(log);
        Ok(())
    }

    fn return_value(env: &Env<'a, S, V>, value_ptr: u32, value_len: u32) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let value = fn_gas_meter.read_bytes(value_ptr, value_len)?;
        fn_gas_meter.command_output_set_return_value(value);
        Ok(())
    }

    fn call(
        env: &Env<'a, S, V>,
        call_input_ptr: u32,
        call_input_len: u32,
        return_ptr_ptr: u32,
    ) -> Result<u32, FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let sc_context = ctx.clone_smart_contract_context();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        // Parse the call command arguments
        let (target, method, arguments, amount) = {
            let call_command_bytes = fn_gas_meter
                .read_bytes(call_input_ptr, call_input_len)
                .map_err(FuncError::Runtime)?;
            let call_command = Command::deserialize(&call_command_bytes)
                .map_err(|e| FuncError::Runtime(e.into()))?;

            match call_command {
                Command::Call(CallInput {
                    target,
                    method,
                    arguments,
                    amount,
                }) => (target, method, arguments, amount),
                _ => return Err(FuncError::Internal),
            }
        };

        // error if transfer amount is specified in view call.
        if env.is_view && amount.is_some() {
            return Err(FuncError::Internal);
        }

        // transfer from calling contract address (call_tx.target) to the target address first.
        if let Some(amount) = amount {
            transfer_from_contract(env.call_tx.target, amount, target, &mut fn_gas_meter)?;
        }

        // Get the Contract Code and create the contract module
        let contract_module = fn_gas_meter
            .ws_cached_contract(target, &sc_context)
            .ok_or(FuncError::ContractNotFound)?;

        // by default, fields would be inherited from parent transaction
        let call_tx = CallTx {
            base_tx: BaseTx {
                command_kinds: env.call_tx.command_kinds.clone(),
                signer: env.call_tx.target,
                gas_limit: fn_gas_meter.remaining_gas(),
                ..env.call_tx.base_tx
            },
            amount,
            arguments,
            method,
            target,
        };

        // release mutexes for child contract to acquire and instantiate
        drop(wasmer_gas_global);
        drop(ctx);

        // Instantiate and call the child contract
        let (_, child_call_gas_consumed, child_call_error) = contract_module
            .instantiate(
                env.context.clone(), // here we only clone the existing Arc from the parent
                env.call_counter.saturating_add(1),
                env.is_view,
                call_tx,
                env.params_from_blockchain.clone(),
            )
            .map_err(|_| FuncError::ContractNotFound)?
            .call();

        // reacquire the TransitionContext in the parent function
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        fn_gas_meter.deduct_gas(child_call_gas_consumed);

        match child_call_error {
            None => {
                // Take the child result in parent's execution context.
                if let Some(res) = fn_gas_meter.command_output_cache().take_return_value() {
                    return fn_gas_meter
                        .write_bytes(res, return_ptr_ptr)
                        .map_err(FuncError::Runtime);
                }
            }
            Some(e) => {
                if fn_gas_meter.remaining_gas() == 0 {
                    return Err(FuncError::GasExhaustionError);
                }
                return Err(FuncError::MethodCallError(e));
            }
        }
        Ok(0)
    }

    fn transfer(env: &Env<'a, S, V>, transfer_input_ptr: u32) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let mut fn_gas_meter =
            HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let transfer_bytes = fn_gas_meter
            .read_bytes(transfer_input_ptr, std::mem::size_of::<[u8; 40]>() as u32)
            .map_err(FuncError::Runtime)?;

        // first 32-bytes are the recipient address, last 8 is the amount
        let (recipient, amount_bytes) = transfer_bytes.split_at(32);
        let recipient = recipient.try_into().unwrap();
        let amount = u64::from_le_bytes(amount_bytes.try_into().unwrap());

        transfer_from_contract(
            env.call_tx.target, // calling contract's address from transaction execution context
            amount,
            recipient,
            &mut fn_gas_meter,
        )
    }

    fn defer_create_deposit(
        env: &Env<'a, S, V>,
        create_deposit_input_ptr: u32,
        create_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let serialized_command = fn_gas_meter
            .read_bytes(create_deposit_input_ptr, create_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command =
            Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, Command::CreateDeposit { .. }) {
            return Err(FuncError::Internal);
        }

        ctx.append_deferred_command(DeferredCommand {
            command,
            contract_address: env.call_tx.target,
        });

        Ok(())
    }

    fn defer_set_deposit_settings(
        env: &Env<'a, S, V>,
        set_deposit_settings_input_ptr: u32,
        set_deposit_settings_input_len: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let serialized_command = fn_gas_meter
            .read_bytes(
                set_deposit_settings_input_ptr,
                set_deposit_settings_input_len,
            )
            .map_err(FuncError::Runtime)?;
        let command =
            Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, Command::SetDepositSettings { .. }) {
            return Err(FuncError::Internal);
        }

        ctx.append_deferred_command(DeferredCommand {
            command,
            contract_address: env.call_tx.target,
        });

        Ok(())
    }

    fn defer_topup_deposit(
        env: &Env<'a, S, V>,
        top_up_deposit_input_ptr: u32,
        top_up_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let serialized_command = fn_gas_meter
            .read_bytes(top_up_deposit_input_ptr, top_up_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command =
            Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, Command::TopUpDeposit { .. }) {
            return Err(FuncError::Internal);
        }

        ctx.append_deferred_command(DeferredCommand {
            command,
            contract_address: env.call_tx.target,
        });

        Ok(())
    }

    fn defer_withdraw_deposit(
        env: &Env<'a, S, V>,
        withdraw_deposit_input_ptr: u32,
        withdraw_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let serialized_command = fn_gas_meter
            .read_bytes(withdraw_deposit_input_ptr, withdraw_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command =
            Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, Command::WithdrawDeposit { .. }) {
            return Err(FuncError::Internal);
        }

        ctx.append_deferred_command(DeferredCommand {
            command,
            contract_address: env.call_tx.target,
        });

        Ok(())
    }

    fn defer_stake_deposit(
        env: &Env<'a, S, V>,
        stake_deposit_input_ptr: u32,
        stake_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let serialized_command = fn_gas_meter
            .read_bytes(stake_deposit_input_ptr, stake_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command =
            Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, Command::StakeDeposit { .. }) {
            return Err(FuncError::Internal);
        }

        ctx.append_deferred_command(DeferredCommand {
            command,
            contract_address: env.call_tx.target,
        });

        Ok(())
    }

    fn defer_unstake_deposit(
        env: &Env<'a, S, V>,
        unstake_deposit_input_ptr: u32,
        unstake_deposit_input_len: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let serialized_command = fn_gas_meter
            .read_bytes(unstake_deposit_input_ptr, unstake_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command =
            Command::deserialize(&serialized_command).map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, Command::UnstakeDeposit { .. }) {
            return Err(FuncError::Internal);
        }

        ctx.append_deferred_command(DeferredCommand {
            command,
            contract_address: env.call_tx.target,
        });

        Ok(())
    }

    fn sha256(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        digest_ptr_ptr: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let input_bytes = fn_gas_meter.read_bytes(msg_ptr, msg_len)?;
        let digest = fn_gas_meter.sha256(input_bytes);

        fn_gas_meter.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn keccak256(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        digest_ptr_ptr: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let input_bytes = fn_gas_meter.read_bytes(msg_ptr, msg_len)?;
        let digest = fn_gas_meter.keccak256(input_bytes);

        fn_gas_meter.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn ripemd(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        digest_ptr_ptr: u32,
    ) -> Result<(), FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let input_bytes = fn_gas_meter.read_bytes(msg_ptr, msg_len)?;
        let digest = fn_gas_meter.ripemd(input_bytes);

        fn_gas_meter.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn verify_ed25519_signature(
        env: &Env<'a, S, V>,
        msg_ptr: u32,
        msg_len: u32,
        signature_ptr: u32,
        address_ptr: u32,
    ) -> Result<i32, FuncError> {
        let mut ctx = env.context.lock().unwrap();
        let mut wasmer_gas_global = env.wasmer_gas_global.lock().unwrap();
        let fn_gas_meter = HostFuncGasMeter::new(&mut ctx.gas_meter, &mut wasmer_gas_global, env);

        let message = fn_gas_meter.read_bytes(msg_ptr, msg_len)?;
        let signature = fn_gas_meter.read_bytes(signature_ptr, 64)?;
        let address = fn_gas_meter.read_bytes(address_ptr, 32)?;

        fn_gas_meter
            .verify_ed25519_signature(
                message,
                signature.try_into().unwrap(),
                address.try_into().unwrap(),
            )
            .map_err(|_| FuncError::Internal)
    }
}

/// Execution logic for transferring tokens from a contract
fn transfer_from_contract<S, V>(
    signer: PublicAddress,
    amount: u64,
    recipient: PublicAddress,
    gas_meter: &mut HostFuncGasMeter<'_, '_, S, Env<'_, S, V>, V>,
) -> Result<(), FuncError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
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
