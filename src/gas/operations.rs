/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Functions defining the business logic of each gas-chargeable operation 
//! and its associated cost. 
//! 
//! These functions will be called by the relevant gas meter of the respective execution environment
//! whether it be the native runtime [gas meter](crate::gas::GasMeter), 
//! or [host function gas meter](crate::gas::wasmer_gas::HostFuncGasMeter) in the context of Wasm execution.
//! 
//! The functions here fall into one of four categories of operations which incur gas, namely:
//! - World State Access
//! - Reading and writing to linear Wasm memory through host functions
//! - Transaction-related data storage on the blockchain
//! - Cryptographic operations in contract calls
//! 
//! There is a fifth category of operations which incur gas, namely executing Wasm opcodes in the Wasm runtime.
//! The Wasm runtime (Wasmer) translates the primitive Wasm opcodes into machine code instructions that the host processor can execute. 
//! By reading configuration defined in [gas primitives](crate::gas), Wasmer tallies the cost for each opcode executed
//! and we can access this tally through the [WasmerGasGlobal](crate::gas::wasmer_gas::WasmerGasGlobal) struct.

use ed25519_dalek::Verifier;
use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::{VersionProvider, DB};
use ripemd::Ripemd160;
use sha2::{Digest as sha256_digest, Sha256};
use tiny_keccak::{Hasher as _, Keccak};

use crate::{
    contract::{wasmer::memory::MemoryContext, ContractModule, SmartContractContext},
    execution::cache::{CacheValue, WorldStateCache},
    types::TxnVersion,
};

use super::constants::*;
use super::CostChange;

pub(crate) type OperationReceipt<T> = (T, CostChange);


/* ↓↓↓ Functions for World State Access ↓↓↓ */

/// Implements the `G_st_set` and `G_st_set_v2` gas cost formulas in the Mainnet Protocol,
/// and sets storage data on the Storage Trie for a particular account address
pub(crate) fn ws_set_storage_data<S, V>(
    txn_version: TxnVersion,
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    key: &[u8],
    value: Vec<u8>,
) -> OperationReceipt<()>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let new_val_len = CacheValue::len(&value);
    let (old_val_len, get_cost) = ws_storage_data(txn_version, ws_cache, address, key);
    let old_val_len = old_val_len.as_ref().map_or(0, CacheValue::len);
    
    ws_cache.set_storage_data(address, key, value);

    let traversed_key_len = storage_trie_traversed_key_len(txn_version, &address, key);
    let cost = 
        // steps 1 and 2, note the hashing cost is included in get cost
        get_cost
        // step 3   
        + CostChange::reward(set_cost_delete_old_value(
            traversed_key_len,
            old_val_len,
            new_val_len))
        // step 4 
        + CostChange::deduct(set_cost_write_new_value(new_val_len))
        // step 5    
        + CostChange::deduct(set_cost_rehash(traversed_key_len));

    ((), cost)
}

/// Implements the `G_at_set` gas cost formula in the Mainnet Protocol,
/// and sets an account's balance in the Account Trie
pub(crate) fn ws_set_balance<S, V>(
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    balance: u64,
) -> OperationReceipt<()>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let key_len = ACCOUNT_TRIE_KEY_LENGTH;
    let new_val_len = balance.len();
    let (old_val_len, get_cost) = ws_balance(ws_cache, &address);
    let old_val_len = old_val_len.len();

    // old_val_len is obtained from Get so the cost of reading the key is already charged
    let set_cost = CostChange::reward(set_cost_delete_old_value(
        key_len,
        old_val_len,
        new_val_len,
    )) + CostChange::deduct(set_cost_write_new_value(new_val_len))
        + CostChange::deduct(set_cost_rehash(key_len));

    ws_cache.set_balance(address, balance);

    ((), get_cost + set_cost)
}

/// Implements the `G_at_set` gas cost formula in the Mainnet Protocol,
/// and sets a contract account's CBI version in the Account Trie
pub(crate) fn ws_set_cbi_version<S, V>(
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    version: u32,
) -> OperationReceipt<()>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let key_len = ACCOUNT_TRIE_KEY_LENGTH;
    let new_val_len = version.len();
    let (old_val_len, get_cost) = ws_cbi_version(ws_cache, &address);
    let old_val_len = old_val_len.as_ref().map_or(0, CacheValue::len);

    // old_val_len is obtained from Get so the cost of reading the key is already charged
    let set_cost = CostChange::reward(set_cost_delete_old_value(
        key_len,
        old_val_len,
        new_val_len,
    )) + CostChange::deduct(set_cost_write_new_value(new_val_len))
        + CostChange::deduct(set_cost_rehash(key_len));

    ws_cache.set_cbi_version(address, version);

    ((), get_cost + set_cost)
}

/// Implements the `G_at_set` gas cost formula in the Mainnet Protocol,
/// and stores a contract account's CBI balance in the Account Trie
pub(crate) fn ws_set_contract_code<S, V>(
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    code: Vec<u8>,
) -> OperationReceipt<()>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let key_len = ACCOUNT_TRIE_KEY_LENGTH;
    let new_val_len = CacheValue::len(&code);
    let (old_val_len, get_cost) = ws_cached_contract_code(ws_cache, &address);
    let old_val_len = old_val_len.as_ref().map_or(0, CacheValue::len);

    // old_val_len is obtained from Get so the cost of reading the key is already charged
    let set_cost = CostChange::reward(set_cost_delete_old_value(
        key_len,
        old_val_len,
        new_val_len,
    )) + CostChange::deduct(set_cost_write_new_value(new_val_len))
        + CostChange::deduct(set_cost_rehash(key_len));

    ws_cache.set_contract_code(address, code);

    ((), get_cost + set_cost)
}


/// Implements the `G_st_get` and `G_st_get_v2` gas cost formulas in the Mainnet Protocol,
/// and fetches a value associated with a provided key 
/// from the Storage Trie for a particular account address
pub(crate) fn ws_storage_data<S, V>(
    txn_version: TxnVersion,
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    key: &[u8],
) -> OperationReceipt<Option<Vec<u8>>>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.storage_data(address, key);
    let traversed_key_len = storage_trie_traversed_key_len(txn_version, &address, key);
    let get_cost = CostChange::deduct(
        // step 1
        storage_trie_key_hash_cost(txn_version, key)
            // step 2
            .saturating_add(get_cost_traverse(traversed_key_len))
            // step 3
            .saturating_add(get_cost_read(value.as_ref().map_or(0, CacheValue::len))),
    );

    (value, get_cost)
}

/// Implements the `G_at_get` gas cost formula in the Mainnet Protocol,
/// and fetches the balance of a particular address from the Account Trie
pub(crate) fn ws_balance<S, V>(
    ws_cache: &WorldStateCache<S, V>,
    address: &PublicAddress,
) -> OperationReceipt<u64>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.balance(address);
    let get_cost = CostChange::deduct(
        // step 1
        get_cost_traverse(ACCOUNT_TRIE_KEY_LENGTH).saturating_add
            // step 2
            (get_cost_read(value.len())),
    );
    (value, get_cost)
}

/// Implements the `G_at_get` gas cost formula in the Mainnet Protocol,
/// and fetches the CBI version of a particular contract address from the Account Trie
pub(crate) fn ws_cbi_version<S, V>(
    ws_cache: &WorldStateCache<S, V>,
    address: &PublicAddress,
) -> OperationReceipt<Option<u32>>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.cbi_version(address);
    let get_cost = CostChange::deduct(
        // step 1
        get_cost_traverse(ACCOUNT_TRIE_KEY_LENGTH)
            // step 2
            .saturating_add(get_cost_read(value.as_ref().map_or(0, |v| v.len()))),
    );
    (value, get_cost)
}

/// Implements the `G_at_get` gas cost formula in the Mainnet Protocol,
/// and fetches the code bytes of a particular contract address from the Account Trie
pub(crate) fn ws_cached_contract_code<S, V>(
    ws_cache: &WorldStateCache<S, V>,
    address: &PublicAddress,
) -> OperationReceipt<Option<Vec<u8>>>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.contract_code(address);
    let get_cost = CostChange::deduct(discount_code_read(
        // step 1
        get_cost_traverse(ACCOUNT_TRIE_KEY_LENGTH)
            // step 2
            .saturating_add(get_cost_read(value.as_ref().map_or(0, CacheValue::len))),
    ));

    (value, get_cost)
}

/// Implements the `G_at_get` gas cost formula in the Mainnet Protocol,
/// and tries to fetch the code bytes of a particular contract address from the contract cache,
/// failing which fetches the code bytes from the Account Trie
pub(crate) fn ws_cached_contract<S, V>(
    ws_cache: &WorldStateCache<S, V>,
    sc_context: &SmartContractContext,
    address: PublicAddress,
) -> OperationReceipt<Option<ContractModule>>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    // check smart contract cache
    if let Some(contract_module) = ContractModule::from_cache(address, sc_context) {
        let contract_get_cost = CostChange::deduct(discount_code_read(
            // step 1
            get_cost_traverse(ACCOUNT_TRIE_KEY_LENGTH)
                // step 2
                .saturating_add(get_cost_read(contract_module.bytecode_length())),
        ));

        return (Some(contract_module), contract_get_cost);
    }

    // else check ws and charge
    let (value, contract_get_cost) = ws_cached_contract_code(ws_cache, &address);
    let contract_code = match value {
        Some(value) => value,
        None => return (None, contract_get_cost),
    };

    match ContractModule::from_bytecode_unchecked(address, &contract_code, sc_context) {
        Some(contract_module) => (Some(contract_module), contract_get_cost),
        None => (None, contract_get_cost),
    }
}

/// Implements the `G_st_contains` and `G_st_contains_v2` gas cost formulas in the Mainnet Protocol,
/// and checks the existence of a value associated with a provided key
/// from the Storage Trie for a particular account address
pub(crate) fn ws_contains_storage_data<S, V>(
    txn_version: TxnVersion,
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    key: &[u8],
) -> OperationReceipt<bool>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let ret = ws_cache.contains_storage_data(address, key);
    let traversed_key_len = storage_trie_traversed_key_len(txn_version, &address, key);
    let cost_change = CostChange::deduct(
        // step 1
        storage_trie_key_hash_cost(txn_version, key)
            // step 2
            .saturating_add(get_cost_traverse(traversed_key_len)),
    );
    (ret, cost_change)
}

/* ↓↓↓ Functions for reading and writing to Wasm memory ↓↓↓ */

/// Calculates the cost of writing data to memory and writes it to the provided pointer location
pub(crate) fn write_bytes<M: MemoryContext>(
    memory_ctx: &M,
    value: Vec<u8>,
    val_ptr_ptr: u32,
) -> OperationReceipt<Result<u32, anyhow::Error>> {
    let write_cost: u64 = wasm_memory_write_cost(value.len());
    let ret = MemoryContext::write_bytes_to_memory(memory_ctx, value, val_ptr_ptr);
    (ret, CostChange::deduct(write_cost))
}

/// Calculates the cost of reading data to memory and reads it
pub(crate) fn read_bytes<M: MemoryContext>(
    memory_ctx: &M,
    offset: u32,
    len: u32,
) -> OperationReceipt<Result<Vec<u8>, anyhow::Error>> {
    let read_cost = wasm_memory_read_cost(len as usize);
    let ret = MemoryContext::read_bytes_from_memory(memory_ctx, offset, len);
    (ret, CostChange::deduct(read_cost))
}


/* ↓↓↓ Functions for transaction-related data storage ↓↓↓ */

/// Calculates the cost of storing a log on the blockchain 
/// and pushes the relevant log onto the provided holder vector
pub(crate) fn command_output_append_log(logs: &mut Vec<Log>, log: Log) -> OperationReceipt<()> {
    let cost = CostChange::deduct(blockchain_log_cost(log.topic.len(), log.value.len()));
    logs.push(log);
    ((), cost)
}

/// Calculates the cost of storing a generic return value in CommandReceiptV1, or CommandReceiptV2::Call  on the blockchain
/// and sets the value in the provided reference
pub(crate) fn command_output_set_return_value(
    command_output_return_value: &mut Vec<u8>,
    return_value: Vec<u8>,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(blockchain_storage_cost(return_value.len()));
    *command_output_return_value = return_value;
    ((), cost)
}

/// Calculates the cost of storing the amount withdrawn field in CommandReceiptV2::WithdrawDeposit on the blockchain
/// and sets the value in the provided reference
pub(crate) fn command_output_set_amount_withdrawn(
    command_output_amount_withdrawn: &mut u64,
    amount_withdrawn: u64,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(blockchain_storage_cost(std::mem::size_of::<u64>()));
    *command_output_amount_withdrawn = amount_withdrawn;
    ((), cost)
}

/// Calculates the cost of storing the amount staked field in CommandReceiptV2::StakeDeposit on the blockchain
/// and sets the value in the provided reference
pub(crate) fn command_output_set_amount_staked(
    command_output_amount_staked: &mut u64,
    amount_staked: u64,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(blockchain_storage_cost(std::mem::size_of::<u64>()));
    *command_output_amount_staked = amount_staked;
    ((), cost)
}

/// Calculates the cost of storing the amount unstaked field in CommandReceiptV2::UnstakeDeposit on the blockchain
/// and sets the value in the provided reference
pub(crate) fn command_output_set_amount_unstaked(
    command_output_amount_unstaked: &mut u64,
    amount_unstaked: u64,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(blockchain_storage_cost(std::mem::size_of::<u64>()));
    *command_output_amount_unstaked = amount_unstaked;
    ((), cost)
}



/* ↓↓↓ Functions for cryptographic operations ↓↓↓ */

/// Implements the `G_wsha256` gas cost formula in the Mainnet Protocol,
/// and hashes a provided input using the SHA256 algorithm
pub(crate) fn sha256(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
    let cost = CostChange::deduct(CRYPTO_SHA256_PER_BYTE * input_bytes.len() as u64);
    let mut hasher = Sha256::new();
    hasher.update(input_bytes);
    let ret = hasher.finalize().to_vec();
    (ret, cost)
}

/// Implements the `G_wkeccak256` gas cost formula in the Mainnet Protocol,
/// and hashes a provided input using the Keccak256 algorithm
pub(crate) fn keccak256(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
    let cost = CostChange::deduct(CRYPTO_KECCAK256_PER_BYTE * input_bytes.len() as u64);
    let mut output_bytes = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(&input_bytes);
    keccak.finalize(&mut output_bytes);
    let ret = output_bytes.to_vec();
    (ret, cost)
}

/// Implements the `G_wripemd160` gas cost formula in the Mainnet Protocol,
/// and hashes a provided input using the RIPEMD160 algorithm
pub(crate) fn ripemd(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
    let cost = CostChange::deduct(CRYPTO_RIPEMD160_PER_BYTE * input_bytes.len() as u64);
    let mut hasher = Ripemd160::new();
    hasher.update(&input_bytes);
    let ret = hasher.finalize().to_vec();
    (ret, cost)
}

/// Implements the `G_wvrfy25519` gas cost formula in the Mainnet Protocol,
/// and verifies a provided message signature using the Ed25519 algorithm, given a public key
pub(crate) fn verify_ed25519_signature(
    message: Vec<u8>,
    signature: [u8; 64],
    pub_key: [u8; 32],
) -> OperationReceipt<Result<i32, anyhow::Error>> {
    let cost = CostChange::deduct(1_400_000_u64.saturating_add(
        (message.len() as u64).saturating_mul(CRYPTO_ED25519_PER_BYTE)));
    let public_key = match ed25519_dalek::VerifyingKey::from_bytes(&pub_key) {
        Ok(public_key) => public_key,
        Err(e) => return (Err(e.into()), cost),
    };
    let dalek_signature = ed25519_dalek::Signature::from_bytes(&signature);
    let is_ok = public_key.verify(&message, &dalek_signature).is_ok();

    (Ok(is_ok as i32), cost)
}

/* ↓↓↓ Misc helpers ↓↓↓ */

/// Helper function to calculate the length of the Storage Trie key for gas charging purposes
/// References the key length definition which is used across
/// the G_st_get and G_st_get_v2 and
/// the G_st_set and G_st_set_v2 and 
/// the G_st_contains and G_st_contains_v2
/// functions of gas charging section in the Mainnet Protocol
pub (crate) fn storage_trie_traversed_key_len(
    version: TxnVersion,
    address: &PublicAddress,
    key: &[u8],
) -> usize {
    match version {
        // protocol v0.4.0 (using TransactionV1) included extra address length on top of Account Trie key length
        TxnVersion::V1 => ACCOUNT_TRIE_KEY_LENGTH + address.len() + key.len(),
        // protocol v0.5.0 (using TransactionV2) removes extra address length,
        // and specifies that hashing is peformed on Storage Trie keys if longer than or equal to 32 bytes
        // due to updates in the World State implementation
        TxnVersion::V2 => {
            ACCOUNT_TRIE_KEY_LENGTH + 
            if key.len() < 32 {
                key.len()
            } else {
                KECCAK256_LENGTH as usize
            }
        }
    }
}

/// Helper function to calculate the cost of hashing storage trie keys if needed.
pub (crate) fn storage_trie_key_hash_cost(txn_version: TxnVersion, key: &[u8]) -> u64 {
    // protocol v0.4.0 (using TransactionV1) did not hash the key
    // protocol v0.5.0 (using TransactionV2) hashes the key if longer than or equal to 32 bytes
    match txn_version {
        TxnVersion::V1 => 0,
        TxnVersion::V2 => {
            storage_trie_key_hashing_cost(key.len())
        }
    }
}
