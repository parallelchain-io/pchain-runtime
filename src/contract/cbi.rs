/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Definition of host functions that are imported by ParallelChain Smart Contracts.
//!
//! The definitions follows the specification in [ParallelChain protocol](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Contracts.md).

use wasmer::{imports, Function, ImportObject, Store};

use super::MethodCallError;

/// Definition of host functions with [wasmer::WasmerEnv]. Implement this trait for creation of importable
/// that can be used in instantiation of contract module.
///
/// Host function arguments with suffix `_ptr_ptr` are namely pointer-to-pointer variable that
/// is considered as mutable reference to memory as an output value. Method `wasmer_memory::MemoryContext::set_return_values_to_memory`
/// can be used to set output value into this variable.
///
/// Host functions arguments with suffix `_ptr` are namely pointer-to variable that
/// is considers as immutable reference to memory as an input value. Method `wasmer_memory::MemoryContext::read_bytes`
/// can be used to read the value from this variable.
pub trait ContractBinaryInterface<T>
where
    T: wasmer::WasmerEnv + 'static,
{
    /// Sets a key to a value in the current Contract Account’s Storage.
    fn set(
        env: &T,
        key_ptr: u32,
        key_len: u32,
        value_ptr: u32,
        value_len: u32,
    ) -> Result<(), FuncError>;

    /// Gets the value corresponding to a key in the current Contract Account’s Storage.
    /// It returns the length of the value.
    fn get(env: &T, key_ptr: u32, key_len: u32, value_ptr_ptr: u32) -> Result<i64, FuncError>;

    /// Gets the value corresponding to a key in the Network Account’s Storage.
    /// It returns the length of the value.
    fn get_network_storage(
        env: &T,
        key_ptr: u32,
        key_len: u32,
        value_ptr_ptr: u32,
    ) -> Result<i64, FuncError>;

    /// Get the balance of current account
    fn balance(env: &T) -> Result<u64, FuncError>;

    /// Gets the Height of the Block which the Transaction at the start of the current Call Chain is included in.
    fn block_height(env: &T) -> Result<u64, FuncError>;

    /// Gets the Timestamp of the Block which the Transaction at the start of the current Call Chain is included in.
    fn block_timestamp(env: &T) -> Result<u32, FuncError>;

    /// Get the Hash field of the previous Block.
    /// - `hash_ptr_ptr` points to memory of 32 bytes address.
    fn prev_block_hash(env: &T, hash_ptr_ptr: u32) -> Result<(), FuncError>;

    /// Gets the Address of the Account that triggered the current Call. This could either be an External
    /// Account (if the Call is directly triggered by a Call Transaction), or a Contract Account (if the Call is an Internal Call).
    /// - `address_ptr_ptr` points to memory of 32 bytes address.
    /// - returns the length of the value.
    fn calling_account(env: &T, address_ptr_ptr: u32) -> Result<(), FuncError>;

    /// Gets the Address of the current Account.
    /// - `address_ptr_ptr` points to memory of 32 bytes address.
    /// - returns the length of the value.
    fn current_account(env: &T, address_ptr_ptr: u32) -> Result<(), FuncError>;

    /// Gets the Method of the current Call.
    /// - `method_ptr_ptr` points to memory of bytes.
    /// - returns the length of the value.
    fn method(env: &T, method_ptr_ptr: u32) -> Result<u32, FuncError>;

    /// Gets the Arguments of the current Call.
    /// - `arguments_ptr_ptr` points to memory of bytes.
    /// - returns the length of the value.
    fn arguments(env: &T, arguments_ptr_ptr: u32) -> Result<u32, FuncError>;

    /// get transaction value of this transaction.
    /// - returns the amount.
    fn amount(env: &T) -> Result<u64, FuncError>;

    /// Returns whether the current Call is an Internal Call.
    fn is_internal_call(env: &T) -> Result<i32, FuncError>;

    /// get transaction hash of this transaction.
    /// -`hash_ptr_ptr` points to memory of 32 bytes data.
    fn transaction_hash(env: &T, hash_ptr_ptr: u32) -> Result<(), FuncError>;

    /// call methods of another contract.
    /// - `call_ptr` points to memory of [pchain_types::blockchain::Command::Call]
    /// - `return_ptr_ptr` points to memory of bytes.
    /// - returns the length of Return Value.
    fn call(
        env: &T,
        call_input_ptr: u32,
        call_input_len: u32,
        rval_ptr_ptr: u32,
    ) -> Result<u32, FuncError>;

    /// Set return value of contract execution, which is also a field in resulting receipt.
    /// - `value_ptr` points to memory of bytes of arbitary input.
    fn return_value(env: &T, value_ptr: u32, value_len: u32) -> Result<(), FuncError>;

    /// Transfers the specified number of Grays to a specified Address
    /// - `transfer_input_ptr` points to memory of 40 bytes address: 32-byte address and 8-byte little endian integer.
    fn transfer(env: &T, transfer_input_ptr: u32) -> Result<(), FuncError>;

    /// Insert command execution after success of this contract call.
    /// - `create_deposit_input_ptr` points to memory of arbitrary input which expects to be a serialized [pchain_types::blockchain::Command::CreateDeposit].
    fn defer_create_deposit(
        env: &T,
        create_deposit_input_ptr: u32,
        create_deposit_input_len: u32,
    ) -> Result<(), FuncError>;

    /// Insert command execution after success of this contract call.
    /// - `set_deposit_settings_input_ptr` points to memory of arbitrary input which expects to be a serialized [pchain_types::blockchain::Command::SetDepositSettings].
    fn defer_set_deposit_settings(
        env: &T,
        set_deposit_settings_input_ptr: u32,
        set_deposit_settings_input_len: u32,
    ) -> Result<(), FuncError>;

    /// Insert command execution after success of this contract call.
    /// - `top_up_deposit_input_ptr` points to memory of arbitrary input which expects to be a serialized [pchain_types::blockchain::Command::TopUpDeposit].
    fn defer_topup_deposit(
        env: &T,
        top_up_deposit_input_ptr: u32,
        top_up_deposit_input_len: u32,
    ) -> Result<(), FuncError>;

    /// Insert command execution after success of this contract call.
    /// - `withdraw_deposit_input_ptr` points to memory of arbitrary input which expects to be a serialized [pchain_types::blockchain::Command::WithdrawDeposit].
    fn defer_withdraw_deposit(
        env: &T,
        withdraw_deposit_input_ptr: u32,
        withdraw_deposit_input_len: u32,
    ) -> Result<(), FuncError>;

    /// Insert command execution after success of this contract call.
    /// - `stake_deposit_input_ptr` points to memory of arbitrary input which expects to be a serialized [pchain_types::blockchain::Command::StakeDeposit].
    fn defer_stake_deposit(
        env: &T,
        stake_deposit_input_ptr: u32,
        stake_deposit_input_len: u32,
    ) -> Result<(), FuncError>;

    /// Insert command execution after success of this contract call.
    /// - `unstake_deposit_input_ptr` points to memory of arbitrary input which expects to be a serialized [pchain_types::blockchain::Command::UnstakeDeposit].
    fn defer_unstake_deposit(
        env: &T,
        unstake_deposit_input_ptr: u32,
        unstake_deposit_input_len: u32,
    ) -> Result<(), FuncError>;

    /// Add a log to the Transaction's Receipt.
    fn log(env: &T, log_ptr: u32, log_len: u32) -> Result<(), FuncError>;

    /// Computes the SHA256 digest of arbitrary input.
    /// - returns 32 bytes digest.
    fn sha256(env: &T, msg_ptr: u32, msg_len: u32, digest_ptr_ptr: u32) -> Result<(), FuncError>;

    /// Computes the Keccak256 digest of arbitrary input.
    /// - returns 32 bytes digest.
    fn keccak256(env: &T, msg_ptr: u32, msg_len: u32, digest_ptr_ptr: u32)
        -> Result<(), FuncError>;

    /// Computes the RIPEMD160 digest of arbitrary input.
    /// - returns 20 bytes digest.
    fn ripemd(env: &T, msg_ptr: u32, msg_len: u32, digest_ptr_ptr: u32) -> Result<(), FuncError>;

    /// Returns whether an Ed25519 signature was produced by a specified by a specified address over some specified message.
    fn verify_ed25519_signature(
        env: &T,
        msg_ptr: u32,
        msg_len: u32,
        signature_ptr: u32,
        address_ptr: u32,
    ) -> Result<i32, FuncError>;
}

/// Create importable for instantiation of contract module.
pub(crate) fn create_importable<'a, T, K>(store: &'a Store, env: &T) -> Importable<'a>
where
    T: wasmer::WasmerEnv + 'static,
    K: ContractBinaryInterface<T> + 'static,
{
    Importable(
        imports! {
            "env" => {
                "set" =>  Function::new_native_with_env(store, env.clone(), K::set),
                "get" => Function::new_native_with_env(store, env.clone(), K::get),
                "get_network_storage" => Function::new_native_with_env(store, env.clone(), K::get_network_storage),
                "balance" => Function::new_native_with_env(store, env.clone(), K::balance),

                "block_height" => Function::new_native_with_env(store, env.clone(), K::block_height),
                "block_timestamp" => Function::new_native_with_env(store, env.clone(), K::block_timestamp),
                "prev_block_hash" => Function::new_native_with_env(store, env.clone(), K::prev_block_hash),

                "calling_account" => Function::new_native_with_env(store, env.clone(), K::calling_account),
                "current_account" => Function::new_native_with_env(store, env.clone(), K::current_account),
                "method" => Function::new_native_with_env(store, env.clone(), K::method),
                "arguments" => Function::new_native_with_env(store, env.clone(), K::arguments),
                "amount" => Function::new_native_with_env(store, env.clone(), K::amount),
                "is_internal_call" => Function::new_native_with_env(store, env.clone(), K::is_internal_call),
                "transaction_hash" => Function::new_native_with_env(store, env.clone(), K::transaction_hash),

                "call" => Function::new_native_with_env(store, env.clone(), K::call),
                "return_value" => Function::new_native_with_env(store, env.clone(), K::return_value),
                "transfer" => Function::new_native_with_env(store, env.clone(), K::transfer),
                "defer_create_deposit" => Function::new_native_with_env(store, env.clone(), K::defer_create_deposit),
                "defer_set_deposit_settings" => Function::new_native_with_env(store, env.clone(), K::defer_set_deposit_settings),
                "defer_topup_deposit" => Function::new_native_with_env(store, env.clone(), K::defer_topup_deposit),
                "defer_withdraw_deposit" => Function::new_native_with_env(store, env.clone(), K::defer_withdraw_deposit),
                "defer_stake_deposit" => Function::new_native_with_env(store, env.clone(), K::defer_stake_deposit),
                "defer_unstake_deposit" => Function::new_native_with_env(store, env.clone(), K::defer_unstake_deposit),

                "_log" => Function::new_native_with_env(store, env.clone(), K::log),

                "sha256" => Function::new_native_with_env(store, env.clone(), K::sha256),
                "keccak256" => Function::new_native_with_env(store, env.clone(), K::keccak256),
                "ripemd" => Function::new_native_with_env(store, env.clone(), K::ripemd),
                "verify_ed25519_signature" => Function::new_native_with_env(store, env.clone(), K::verify_ed25519_signature),
            }
        },
        store,
    )
}

/// Create importable (View) for instantiation of contract module.
pub(crate) fn create_importable_view<'a, T, K>(store: &'a Store, env: &T) -> Importable<'a>
where
    T: wasmer::WasmerEnv + 'static,
    K: ContractBinaryInterface<T> + 'static,
{
    Importable(
        imports! {
            "env" => {
                "set" => Function::new_native(store, not_callable::set),
                "get" => Function::new_native_with_env(store, env.clone(), K::get),
                "get_network_storage" => Function::new_native_with_env(store, env.clone(), K::get_network_storage),
                "balance" => Function::new_native_with_env(store, env.clone(), K::balance),

                "block_height" => Function::new_native(store, not_callable::block_height),
                "block_timestamp" => Function::new_native(store, not_callable::block_timestamp),
                "prev_block_hash" => Function::new_native(store, not_callable::prev_block_hash),

                "calling_account" => Function::new_native(store, not_callable::calling_account),
                "current_account" => Function::new_native_with_env(store, env.clone(), K::current_account),
                "method" => Function::new_native_with_env(store, env.clone(), K::method),
                "arguments" => Function::new_native_with_env(store, env.clone(), K::arguments),
                "amount" => Function::new_native(store, not_callable::amount),
                "is_internal_call" => Function::new_native_with_env(store, env.clone(), K::is_internal_call),
                "transaction_hash" => Function::new_native(store, not_callable::transaction_hash),

                "call" => Function::new_native_with_env(store, env.clone(), K::call), // TODO
                "return_value" => Function::new_native_with_env(store, env.clone(), K::return_value),
                "transfer" => Function::new_native(store, not_callable::transfer),
                "defer_create_deposit" => Function::new_native(store, not_callable::defer_create_deposit),
                "defer_set_deposit_settings" => Function::new_native(store, not_callable::defer_set_deposit_settings),
                "defer_topup_deposit" => Function::new_native(store, not_callable::defer_topup_deposit),
                "defer_withdraw_deposit" => Function::new_native(store, not_callable::defer_withdraw_deposit),
                "defer_stake_deposit" => Function::new_native(store, not_callable::defer_stake_deposit),
                "defer_unstake_deposit" => Function::new_native(store, not_callable::defer_unstake_deposit),

                "_log" => Function::new_native_with_env(store, env.clone(), K::log),

                "sha256" => Function::new_native_with_env(store, env.clone(), K::sha256),
                "keccak256" => Function::new_native_with_env(store, env.clone(), K::keccak256),
                "ripemd" => Function::new_native_with_env(store, env.clone(), K::ripemd),
                "verify_ed25519_signature" => Function::new_native_with_env(store, env.clone(), K::verify_ed25519_signature),
            }
        },
        store,
    )
}

/// Importable is data object required to instantiate contract module
pub(crate) struct Importable<'a>(pub(crate) ImportObject, &'a Store);

/// `blank` implementations of exports functions, used to instantiate a contract to
/// extract its exported metadata (without executing any of its methods).
pub(crate) mod blank {
    use wasmer::{imports, Function, Store};

    pub(crate) fn imports(store: &Store) -> wasmer::ImportObject {
        imports! {
            "env" => {
                "set" => Function::new_native(store, set),
                "get" => Function::new_native(store, get),
                "get_network_storage" => Function::new_native(store, get_network_storage),
                "balance" => Function::new_native(store, balance),

                "block_height" => Function::new_native(store, block_height),
                "block_timestamp" => Function::new_native(store, block_timestamp),
                "prev_block_hash" => Function::new_native(store, prev_block_hash),

                "calling_account" => Function::new_native(store, calling_account),
                "current_account" => Function::new_native(store, current_account),
                "method" => Function::new_native(store, method),
                "arguments" => Function::new_native(store, arguments),
                "amount" => Function::new_native(store, amount),
                "is_internal_call" => Function::new_native(store, is_internal_call),
                "transaction_hash" => Function::new_native(store, transaction_hash),

                "call" => Function::new_native(store, call),
                "return_value" => Function::new_native(store, return_value),
                "transfer" => Function::new_native(store, transfer),
                "defer_create_deposit" => Function::new_native(store, defer_create_deposit),
                "defer_set_deposit_settings" => Function::new_native(store, defer_set_deposit_settings),
                "defer_topup_deposit" => Function::new_native(store, defer_topup_deposit),
                "defer_withdraw_deposit" => Function::new_native(store, defer_withdraw_deposit),
                "defer_stake_deposit" => Function::new_native(store, defer_stake_deposit),
                "defer_unstake_deposit" => Function::new_native(store, defer_unstake_deposit),

                "_log" => Function::new_native(store, log),

                "sha256" => Function::new_native(store, sha256),
                "keccak256" => Function::new_native(store, keccak256),
                "ripemd" => Function::new_native(store, ripemd),
                "verify_ed25519_signature" => Function::new_native(store, verify_ed25519_signature),
            }
        }
    }

    pub(crate) fn set(_: u32, _: u32, _: u32, _: u32) {}
    pub(crate) fn get(_: u32, _: u32, _: u32) -> i64 {
        0
    }
    pub(crate) fn get_network_storage(_: u32, _: u32, _: u32) -> i64 {
        0
    }
    pub(crate) fn balance() -> u64 {
        0
    }

    pub(crate) fn block_height() -> u64 {
        0
    }
    pub(crate) fn block_timestamp() -> u32 {
        0
    }
    pub(crate) fn prev_block_hash(_: u32) {}

    pub(crate) fn calling_account(_: u32) {}
    pub(crate) fn current_account(_: u32) {}
    pub(crate) fn method(_: u32) -> u32 {
        0
    }
    pub(crate) fn arguments(_: u32) -> u32 {
        0
    }
    pub(crate) fn amount() -> u64 {
        0
    }
    pub(crate) fn is_internal_call() -> i32 {
        0
    }
    pub(crate) fn transaction_hash(_: u32) {}

    pub(crate) fn call(_: u32, _: u32, _: u32) -> u32 {
        0
    }
    pub(crate) fn return_value(_: u32, _: u32) {}
    pub(crate) fn transfer(_: u32) {}
    pub(crate) fn defer_create_deposit(_: u32, _: u32) {}
    pub(crate) fn defer_set_deposit_settings(_: u32, _: u32) {}
    pub(crate) fn defer_topup_deposit(_: u32, _: u32) {}
    pub(crate) fn defer_withdraw_deposit(_: u32, _: u32) {}
    pub(crate) fn defer_stake_deposit(_: u32, _: u32) {}
    pub(crate) fn defer_unstake_deposit(_: u32, _: u32) {}

    pub(crate) fn log(_: u32, _: u32) {}

    pub(crate) fn sha256(_: u32, _: u32, _: u32) {}
    pub(crate) fn keccak256(_: u32, _: u32, _: u32) {}
    pub(crate) fn ripemd(_: u32, _: u32, _: u32) {}
    pub(crate) fn verify_ed25519_signature(_: u32, _: u32, _: u32, _: u32) -> i32 {
        0
    }
}

/// stubs that are used as non-callable host functions. E.g. set() in view calls.
mod not_callable {
    use super::FuncError;

    pub(crate) fn set(_: u32, _: u32, _: u32, _: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }

    pub(crate) fn block_height() -> Result<u64, FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn block_timestamp() -> Result<u32, FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn prev_block_hash(_: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }

    pub(crate) fn calling_account(_: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn amount() -> Result<u64, FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn transaction_hash(_: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }

    pub(crate) fn transfer(_: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn defer_create_deposit(_: u32, _: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn defer_set_deposit_settings(_: u32, _: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn defer_topup_deposit(_: u32, _: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn defer_withdraw_deposit(_: u32, _: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn defer_stake_deposit(_: u32, _: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
    pub(crate) fn defer_unstake_deposit(_: u32, _: u32) -> Result<(), FuncError> {
        Err(FuncError::Internal)
    }
}

/// FuncError defines the error returns from execution of host functions
#[derive(Debug, thiserror::Error)]
pub enum FuncError {
    #[error("Internal")]
    Internal,

    #[error("Runtime")]
    Runtime(anyhow::Error),

    #[error("GasExhaustionError")]
    GasExhaustionError,

    /// MethodCallError inside host function is the error from CtoC call.
    #[error("Runtime")]
    MethodCallError(MethodCallError),

    /// Transaction inferred to be CtoC but no contract found with its to_address
    #[error("ContractNotFound")]
    ContractNotFound,

    #[error("InsufficientBalance")]
    InsufficientBalance,
}

impl From<wasmer::RuntimeError> for FuncError {
    fn from(e: wasmer::RuntimeError) -> Self {
        Self::Runtime(e.into())
    }
}

impl From<anyhow::Error> for FuncError {
    fn from(e: anyhow::Error) -> Self {
        Self::Runtime(e)
    }
}
