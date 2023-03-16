/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! An Implementation for host functions used for contract methods.

use pchain_types::{Deserializable};
use pchain_world_state::{keys::AppKey, storage::WorldStateStorage};
use ed25519_dalek::Verifier;
use sha2::{Sha256, Digest as sha256_digest};
use tiny_keccak::{Hasher as _, Keccak};
use ripemd::Ripemd160;

use crate::{
    contract::{ContractBinaryInterface, FuncError}, 
    wasmer::{wasmer_env::Env}, 
    transactions::{self}, 
    gas::{self},
};
    

/// `ContractBinaryFunctions` implements trait [ContractBinaryInterface] that defines all host functions that are used for instantiating contract for calling contract method.
/// ### CBI version: 0
pub(crate) struct ContractBinaryFunctions {}
impl<S> ContractBinaryInterface<Env<S>> for ContractBinaryFunctions where S: WorldStateStorage + Sync + Send + Clone {
    
    fn set(env: &Env<S>, key_ptr: u32, key_len: u32, val_ptr: u32, val_len: u32) -> Result<(), FuncError> {
        let app_key = env.read_bytes(key_ptr, key_len)
            .map_err(FuncError::Runtime)?;
        let app_key = AppKey::new(app_key);

        let new_value = env.read_bytes(val_ptr, val_len)
            .map_err(FuncError::Runtime)?;

        let target_address = env.call_tx.target;

        let cost_change = env.context
            .lock()
            .unwrap()
            .set_app_data(target_address, app_key, new_value);
        env.consume_non_wasm_gas(cost_change);
        Ok(())
    }

    fn get(env: &Env<S>, key_ptr: u32, key_len: u32, val_ptr_ptr: u32) -> Result<i64, FuncError> {
        let app_key = env.read_bytes(key_ptr, key_len)
            .map_err(FuncError::Runtime)?;
        let app_key = AppKey::new(app_key);

        let tx_ctx_lock = env.context.lock().unwrap();
        let (value, cost_change) = match tx_ctx_lock.app_data(env.call_tx.target, app_key) {
            (Some(value), cost_change) => (value, cost_change),
            (None, _) => return Ok(-1)
        };
        drop(tx_ctx_lock);

        env.consume_non_wasm_gas(cost_change);         

        Ok(env.write_bytes(value, val_ptr_ptr)? as i64)
    }

    fn get_network_storage(env: &Env<S>, key_ptr: u32, key_len: u32, val_ptr_ptr: u32) -> Result<i64, FuncError> {
        let app_key = env.read_bytes(key_ptr, key_len)
            .map_err(FuncError::Runtime)?;
        let app_key = AppKey::new(app_key);

        let tx_ctx_lock = env.context.lock().unwrap();
        let (value, cost_change) = match tx_ctx_lock.app_data(pchain_types::NETWORK_ADDRESS, app_key) {
            (Some(value), cost_change) => (value, cost_change),
            (None, _) => return Ok(-1)
        };
        drop(tx_ctx_lock);

        env.consume_non_wasm_gas(cost_change);         

        Ok(env.write_bytes(value, val_ptr_ptr)? as i64)
    }

    fn balance(env: &Env<S>) -> Result<u64, FuncError> {
        let (balance, cost_change) = env.context.lock().unwrap().balance(env.call_tx.target);
        env.consume_non_wasm_gas(cost_change);
        Ok(balance)
    }

    fn block_height(env: &Env<S>) -> Result<u64, FuncError> {
        Ok(env.params_from_blockchain.this_block_number)
    }
    fn block_timestamp(env: &Env<S>) -> Result<u32, FuncError> {
        Ok(env.params_from_blockchain.timestamp)
    }
    fn prev_block_hash(env: &Env<S>, hash_ptr_ptr: u32) -> Result<(), FuncError> {
        env.write_bytes(env.params_from_blockchain.prev_block_hash.to_vec(), hash_ptr_ptr)?;
        Ok(())
    }

    fn calling_account(env: &Env<S>, address_ptr_ptr: u32) -> Result<(), FuncError> {
        env.write_bytes(env.call_tx.signer.to_vec(), address_ptr_ptr)?;
        Ok(())
    }
    fn current_account(env: &Env<S>, address_ptr_ptr: u32) -> Result<(), FuncError> {
        env.write_bytes(env.call_tx.target.to_vec(), address_ptr_ptr)?;
        Ok(())
    }

    fn method(env: &Env<S>, method_ptr_ptr: u32) -> Result<u32, FuncError> {
        Ok(env.write_bytes(env.call_tx.method.as_bytes().to_vec(), method_ptr_ptr)?)
    }

    fn arguments(env: &Env<S>, arguments_ptr_ptr: u32) -> Result<u32, FuncError> {
        match &env.call_tx.arguments {
            Some(arguments) => {
                let arguments = <Vec<Vec<u8>> as pchain_types::Serializable>::serialize(arguments);
                Ok(env.write_bytes(arguments, arguments_ptr_ptr)?)
            },
            None => Ok(0)
        }
    }

    fn amount(env: &Env<S>) -> Result<u64, FuncError> {
        match env.call_tx.amount {
            Some(amount) => Ok(amount),
            None => Ok(0)
        }
    }
    fn is_internal_call(env: &Env<S>) -> Result<i32, FuncError> {
        Ok(i32::from(env.call_counter != 0))
    }
    fn transaction_hash(env: &Env<S>, hash_ptr_ptr: u32) -> Result<(), FuncError> {
        env.write_bytes(env.call_tx.hash.to_vec(), hash_ptr_ptr)?;
        Ok(())
    }

    fn log(env: &Env<S>, log_ptr: u32, log_len: u32) -> Result<(), FuncError> {
        let serialized_log = env.read_bytes(log_ptr, log_len)
            .map_err(FuncError::Runtime)?;
        let log = pchain_types::transaction::Log::deserialize(&serialized_log)
            .map_err(|e| FuncError::Runtime(e.into()))?;

        let mut ctx = env.context.lock().unwrap();
        let cost_change = gas::blockchain_txlog_cost(log.topic.len(), log.value.len());
        ctx.receipt_write_gas += cost_change;
        ctx.logs.push(log);
        drop(ctx);

        env.consume_non_wasm_gas(cost_change);

        Ok(())
    }

    fn return_value(env: &Env<S>, value_ptr: u32, value_len: u32) -> Result<(), FuncError> {
        let value = env.read_bytes(value_ptr, value_len)
            .map_err(FuncError::Runtime)?;
        let value = if value.is_empty() { None } else { Some(value) };

        let cost_change = gas::blockchain_txreceipt_cost(value.as_ref().map_or(0, |v| v.len()));

        let mut ctx = env.context.lock().unwrap();
        ctx.receipt_write_gas += cost_change;
        ctx.return_value = value;
        drop(ctx);

        env.consume_non_wasm_gas(cost_change);

        Ok(())
    }

    fn call(env: &Env<S>, call_input_ptr: u32, call_input_len :u32, return_ptr_ptr: u32) -> Result<u32, FuncError>{
        let call_command_bytes = env.read_bytes(call_input_ptr, call_input_len)
            .map_err(FuncError::Runtime)?;
        let call_command = pchain_types::Command::deserialize(&call_command_bytes)
            .map_err(|e| FuncError::Runtime(e.into()))?;
        
        let (
            target, method, arguments, amount
        ) = match call_command {
            pchain_types::Command::Call { target, method, arguments, amount } => (
                target, method, arguments, amount
            ),
            _=> return Err(FuncError::Internal)
        };

        // obtain the signer address (this contract's address) from transaction execution context
        let signer = env.call_tx.target;
        // gas limit bounded by remaining gas
        let gas_limit = env.get_wasmer_remaining_points();

        // by default, fields would be inherited from parent transaction
        let mut call_tx = env.call_tx.clone();
        call_tx.signer = signer;
        call_tx.gas_limit = gas_limit;

        call_tx.target = target;
        call_tx.method = method;
        call_tx.arguments = arguments;
        call_tx.amount = amount;

        let result = transactions::internal::call_from_contract(
            call_tx,
            env.params_from_blockchain.clone(),
            env.context.clone(),
            env.call_counter.saturating_add(1),
        );
        env.consume_non_wasm_gas(result.non_wasmer_gas); 
        env.consume_wasm_gas(result.exec_gas); // subtract gas consumed from parent contract's environment
        match result.error {
            None => {
                let mut tx_ctx_locked = env.context.lock().unwrap();
                let res = tx_ctx_locked.return_value.clone();
                
                tx_ctx_locked.return_value = None; // clear child result in parent's execution context
                
                if let Some(res) = res {
                    return Ok(env.write_bytes(res, return_ptr_ptr)?)
                }
            }
            Some(e) => {
                if env.get_wasmer_remaining_points() == 0 {
                    return Err(FuncError::GasExhaustionError);
                }
                return Err(e)
            }
        }

        Ok(0)
    }

    fn transfer(env: &Env<S>, transfer_input_ptr: u32) -> Result<(), FuncError> {
        let transfer_bytes = env.read_bytes(transfer_input_ptr, std::mem::size_of::<[u8; 40]>() as u32)
            .map_err(FuncError::Runtime)?;

        let (recipient, amount_bytes) = transfer_bytes.split_at(32);
        let recipient = recipient.try_into().unwrap();
        let amount = u64::from_le_bytes(amount_bytes.try_into().unwrap());

        let result = transactions::internal::transfer_from_contract(
            env.call_tx.target, // the signer address (this contract's address) from transaction execution context
            amount,
            recipient,
            env.context.clone()
        );
        env.consume_non_wasm_gas(result.non_wasmer_gas);

        match result.error {
            None => Ok(()),
            Some(e) => Err(e)
        }
    }

    fn defer_create_deposit(env: &Env<S>, create_deposit_input_ptr: u32, create_deposit_input_len: u32) -> Result<(), FuncError> {
        let serialized_command = env.read_bytes(create_deposit_input_ptr, create_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command = pchain_types::Command::deserialize(&serialized_command)
            .map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, pchain_types::Command::CreateDeposit { .. }) {
            return Err(FuncError::Internal)
        }

        env.context.lock().unwrap().commands.push(command);

        Ok(())
    }

    fn defer_set_deposit_settings(env: &Env<S>, set_deposit_settings_input_ptr: u32, set_deposit_settings_input_len: u32) -> Result<(), FuncError> {
        let serialized_command = env.read_bytes(set_deposit_settings_input_ptr, set_deposit_settings_input_len)
            .map_err(FuncError::Runtime)?;
        let command = pchain_types::Command::deserialize(&serialized_command)
            .map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, pchain_types::Command::SetDepositSettings { .. }) {
            return Err(FuncError::Internal)
        }
        
        env.context.lock().unwrap().commands.push(command);

        Ok(())
    }

    fn defer_topup_deposit(env: &Env<S>, top_up_deposit_input_ptr: u32, top_up_deposit_input_len: u32) -> Result<(), FuncError> {
        let serialized_command = env.read_bytes(top_up_deposit_input_ptr, top_up_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command = pchain_types::Command::deserialize(&serialized_command)
            .map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, pchain_types::Command::TopUpDeposit { .. }) {
            return Err(FuncError::Internal)
        }
        
        env.context.lock().unwrap().commands.push(command);

        Ok(())
    }

    fn defer_withdraw_deposit(env: &Env<S>, withdraw_deposit_input_ptr: u32, withdraw_deposit_input_len: u32) -> Result<(), FuncError> {
        let serialized_command = env.read_bytes(withdraw_deposit_input_ptr, withdraw_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command = pchain_types::Command::deserialize(&serialized_command)
            .map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, pchain_types::Command::WithdrawDeposit { .. }) {
            return Err(FuncError::Internal)
        }
        
        env.context.lock().unwrap().commands.push(command);

        Ok(())
    }

    fn defer_stake_deposit(env: &Env<S>, stake_deposit_input_ptr: u32, stake_deposit_input_len: u32) -> Result<(), FuncError> {
        let serialized_command = env.read_bytes(stake_deposit_input_ptr, stake_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command = pchain_types::Command::deserialize(&serialized_command)
            .map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, pchain_types::Command::StakeDeposit { .. }) {
            return Err(FuncError::Internal)
        }
        
        env.context.lock().unwrap().commands.push(command);

        Ok(())
    }

    fn defer_unstake_deposit(env: &Env<S>, unstake_deposit_input_ptr: u32, unstake_deposit_input_len: u32) -> Result<(), FuncError> {
        let serialized_command = env.read_bytes(unstake_deposit_input_ptr, unstake_deposit_input_len)
            .map_err(FuncError::Runtime)?;
        let command = pchain_types::Command::deserialize(&serialized_command)
            .map_err(|e| FuncError::Runtime(e.into()))?;

        if !matches!(command, pchain_types::Command::UnstakeDeposit { .. }) {
            return Err(FuncError::Internal)
        }
        
        env.context.lock().unwrap().commands.push(command);

        Ok(())
    }

    fn sha256(env: &Env<S>, msg_ptr: u32, msg_len: u32, digest_ptr_ptr: u32) -> Result<(), FuncError> {
        let input_bytes = env.read_bytes(msg_ptr, msg_len)
            .map_err(FuncError::Runtime)?;

        env.consume_wasm_gas(crate::cost::CRYPTO_SHA256_PER_BYTE * input_bytes.len() as u64);

        let mut hasher = Sha256::new();
        sha2::Digest::update(&mut hasher, input_bytes);
        let digest = hasher.finalize().to_vec();

        env.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn keccak256(env: &Env<S>, msg_ptr: u32, msg_len: u32, digest_ptr_ptr: u32) -> Result<(), FuncError> {
        let mut input_bytes = env.read_bytes(msg_ptr, msg_len)
            .map_err(FuncError::Runtime)?;

        env.consume_wasm_gas(crate::cost::CRYPTO_KECCAK256_PER_BYTE * input_bytes.len() as u64);

        let mut keccak = Keccak::v256();
        keccak.update(&input_bytes);
        keccak.finalize(&mut input_bytes);
        let digest = input_bytes.to_vec();

        env.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn ripemd(env: &Env<S>, msg_ptr: u32, msg_len: u32, digest_ptr_ptr :u32) -> Result<(), FuncError>  {
        let input_bytes = env.read_bytes(msg_ptr, msg_len)
            .map_err(FuncError::Runtime)?;

        env.consume_wasm_gas(crate::cost::CRYPTO_RIPEMD160_PER_BYTE * input_bytes.len() as u64);

        let mut hasher = Ripemd160::new();
        hasher.update(&input_bytes);
        let digest = hasher.finalize().to_vec();

        env.write_bytes(digest, digest_ptr_ptr)?;
        Ok(())
    }

    fn verify_ed25519_signature(env: &Env<S>, msg_ptr: u32, msg_len: u32, signature_ptr: u32, address_ptr: u32) -> Result<i32, FuncError> {
        let message = env.read_bytes(msg_ptr, msg_len)
            .map_err(FuncError::Runtime)?;

        let signature = env.read_bytes(signature_ptr, 64)
            .map_err(FuncError::Runtime)?;

        let address = env.read_bytes(address_ptr, 32)
            .map_err(FuncError::Runtime)?;

        env.consume_wasm_gas(crate::cost::crypto_verify_ed25519_signature_cost(message.len()));

        let public_key = match ed25519_dalek::PublicKey::from_bytes(&address) {
            Ok(dalek_pk) => dalek_pk,
            Err(_) => return Err(FuncError::Internal)
        };
    
        let dalek_signature = match ed25519_dalek::Signature::from_bytes(&signature) {
            Ok(dalek_sig) => dalek_sig,
            Err(_) => return Err(FuncError::Internal)
        };
    
        let result = public_key.verify(&message, &dalek_signature).is_ok();

        Ok(result as i32)
    }
}