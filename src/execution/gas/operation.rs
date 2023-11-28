use ed25519_dalek::Verifier;
use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::{VersionProvider, DB};
use ripemd::Ripemd160;
use sha2::{Digest as sha256_digest, Sha256};
use tiny_keccak::{Hasher as _, Keccak};

use crate::{
    contract::{wasmer::memory::MemoryContext, ContractModule, SmartContractContext},
    execution::cache::{CacheValue, WorldStateCache},
    gas::{self, get_cost_read, get_cost_traverse, KECCAK256_LENGTH},
    types::TxnVersion,
};

use super::CostChange;

pub(crate) type OperationReceipt<T> = (T, CostChange);

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
    let (old_val_len, get_cost) = ws_get_storage_data(txn_version, ws_cache, address, key);
    let old_val_len = old_val_len.as_ref().map_or(0, CacheValue::len);
    
    ws_cache.set_storage_data(address, key, value);

    let traversed_key_len = storage_trie_traversed_key_len(txn_version, &address, key);
    let cost = 
        // steps 1 and 2, note the hashing cost is included in get cost
        get_cost
        // step 3   
        + CostChange::reward(gas::set_cost_delete_old_value(
            traversed_key_len,
            old_val_len,
            new_val_len))
        // step 4 
        + CostChange::deduct(gas::set_cost_write_new_value(new_val_len))
        // step 5    
        + CostChange::deduct(gas::set_cost_rehash(traversed_key_len));

    ((), cost)
}

pub(crate) fn ws_set_balance<S, V>(
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    balance: u64,
) -> OperationReceipt<()>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let key_len = gas::ACCOUNT_TRIE_KEY_LENGTH;
    let new_val_len = balance.len();
    let (old_val_len, get_cost) = ws_get_balance(ws_cache, &address);
    let old_val_len = old_val_len.len();

    // old_val_len is obtained from Get so the cost of reading the key is already charged
    let set_cost = CostChange::reward(gas::set_cost_delete_old_value(
        key_len,
        old_val_len,
        new_val_len,
    )) + CostChange::deduct(gas::set_cost_write_new_value(new_val_len))
        + CostChange::deduct(gas::set_cost_rehash(key_len));

    ws_cache.set_balance(address, balance);

    ((), get_cost + set_cost)
}

pub(crate) fn ws_set_cbi_version<S, V>(
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    version: u32,
) -> OperationReceipt<()>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let key_len = gas::ACCOUNT_TRIE_KEY_LENGTH;
    let new_val_len = version.len();
    let (old_val_len, get_cost) = ws_get_cbi_version(ws_cache, &address);
    let old_val_len = old_val_len.as_ref().map_or(0, CacheValue::len);

    // old_val_len is obtained from Get so the cost of reading the key is already charged
    let set_cost = CostChange::reward(gas::set_cost_delete_old_value(
        key_len,
        old_val_len,
        new_val_len,
    )) + CostChange::deduct(gas::set_cost_write_new_value(new_val_len))
        + CostChange::deduct(gas::set_cost_rehash(key_len));

    ws_cache.set_cbi_version(address, version);

    ((), get_cost + set_cost)
}

pub(crate) fn ws_set_contract_code<S, V>(
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    code: Vec<u8>,
) -> OperationReceipt<()>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let key_len = gas::ACCOUNT_TRIE_KEY_LENGTH;
    let new_val_len = CacheValue::len(&code);
    let (old_val_len, get_cost) = ws_get_contract_code(ws_cache, &address);
    let old_val_len = old_val_len.as_ref().map_or(0, CacheValue::len);

    // old_val_len is obtained from Get so the cost of reading the key is already charged
    let set_cost = CostChange::reward(gas::set_cost_delete_old_value(
        key_len,
        old_val_len,
        new_val_len,
    )) + CostChange::deduct(gas::set_cost_write_new_value(new_val_len))
        + CostChange::deduct(gas::set_cost_rehash(key_len));

    ws_cache.set_contract_code(address, code);

    ((), get_cost + set_cost)
}

pub(crate) fn ws_get_storage_data<S, V>(
    txn_version: TxnVersion,
    ws_cache: &mut WorldStateCache<S, V>,
    address: PublicAddress,
    key: &[u8],
) -> OperationReceipt<Option<Vec<u8>>>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.get_storage_data(address, key);
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

pub(crate) fn ws_get_balance<S, V>(
    ws_cache: &WorldStateCache<S, V>,
    address: &PublicAddress,
) -> OperationReceipt<u64>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.get_balance(address);
    let get_cost = CostChange::deduct(
        // step 1
        get_cost_traverse(gas::ACCOUNT_TRIE_KEY_LENGTH).saturating_add
            // step 2
            (get_cost_read(value.len())),
    );
    (value, get_cost)
}

pub(crate) fn ws_get_cbi_version<S, V>(
    ws_cache: &WorldStateCache<S, V>,
    address: &PublicAddress,
) -> OperationReceipt<Option<u32>>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.get_cbi_version(address);
    let get_cost = CostChange::deduct(
        // step 1
        get_cost_traverse(gas::ACCOUNT_TRIE_KEY_LENGTH)
            // step 2
            .saturating_add(get_cost_read(value.as_ref().map_or(0, |v| v.len()))),
    );
    (value, get_cost)
}

pub(crate) fn ws_get_contract_code<S, V>(
    ws_cache: &WorldStateCache<S, V>,
    address: &PublicAddress,
) -> OperationReceipt<Option<Vec<u8>>>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let value = ws_cache.get_contract_code(address);
    let get_cost = CostChange::deduct(gas::discount_code_read(
        // step 1
        get_cost_traverse(gas::ACCOUNT_TRIE_KEY_LENGTH)
            // step 2
            .saturating_add(get_cost_read(value.as_ref().map_or(0, CacheValue::len))),
    ));

    (value, get_cost)
}

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

pub(crate) fn ws_get_cached_contract<S, V>(
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
        let contract_get_cost = CostChange::deduct(gas::discount_code_read(
            // step 1
            get_cost_traverse(gas::ACCOUNT_TRIE_KEY_LENGTH)
                // step 2
                .saturating_add(get_cost_read(contract_module.bytes_length())),
        ));

        return (Some(contract_module), contract_get_cost);
    }

    // else check ws and charge
    let (value, contract_get_cost) = ws_get_contract_code(ws_cache, &address);
    let contract_code = match value {
        Some(value) => value,
        None => return (None, contract_get_cost),
    };

    match ContractModule::from_contract_code_unchecked(address, &contract_code, sc_context) {
        Some(contract_module) => (Some(contract_module), contract_get_cost),
        None => (None, contract_get_cost),
    }
}

/// write the data to memory, charge the write cost and return the length
pub(crate) fn write_bytes<M: MemoryContext>(
    memory_ctx: &M,
    value: Vec<u8>,
    val_ptr_ptr: u32,
) -> OperationReceipt<Result<u32, anyhow::Error>> {
    let write_cost: u64 = gas::wasm_memory_write_cost(value.len());
    let ret = MemoryContext::write_bytes_to_memory(memory_ctx, value, val_ptr_ptr);
    (ret, CostChange::deduct(write_cost))
}

/// read data from memory and charge the read cost
pub(crate) fn read_bytes<M: MemoryContext>(
    memory_ctx: &M,
    offset: u32,
    len: u32,
) -> OperationReceipt<Result<Vec<u8>, anyhow::Error>> {
    let read_cost = gas::wasm_memory_read_cost(len as usize);
    let ret = MemoryContext::read_bytes_from_memory(memory_ctx, offset, len);
    (ret, CostChange::deduct(read_cost))
}

pub(crate) fn command_output_append_log(logs: &mut Vec<Log>, log: Log) -> OperationReceipt<()> {
    let cost = CostChange::deduct(gas::blockchain_log_cost(log.topic.len(), log.value.len()));
    logs.push(log);
    ((), cost)
}

pub(crate) fn command_output_set_return_value(
    command_output_return_value: &mut Vec<u8>,
    return_value: Vec<u8>,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(gas::blockchain_return_value_cost(return_value.len()));
    *command_output_return_value = return_value;
    ((), cost)
}

pub(crate) fn command_output_set_amount_withdrawn(
    command_output_amount_withdrawn: &mut u64,
    amount_withdrawn: u64,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(gas::blockchain_return_value_cost(std::mem::size_of::<u64>()));
    *command_output_amount_withdrawn = amount_withdrawn;
    ((), cost)
}

pub(crate) fn command_output_set_amount_staked(
    command_output_amount_staked: &mut u64,
    amount_staked: u64,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(gas::blockchain_return_value_cost(std::mem::size_of::<u64>()));
    *command_output_amount_staked = amount_staked;
    ((), cost)
}

pub(crate) fn command_output_set_amount_unstaked(
    command_output_amount_unstaked: &mut u64,
    amount_unstaked: u64,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(gas::blockchain_return_value_cost(std::mem::size_of::<u64>()));
    *command_output_amount_unstaked = amount_unstaked;
    ((), cost)
}

pub(crate) fn sha256(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
    let cost = CostChange::deduct(gas::CRYPTO_SHA256_PER_BYTE * input_bytes.len() as u64);
    let mut hasher = Sha256::new();
    hasher.update(input_bytes);
    let ret = hasher.finalize().to_vec();
    (ret, cost)
}

pub(crate) fn keccak256(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
    let cost = CostChange::deduct(gas::CRYPTO_KECCAK256_PER_BYTE * input_bytes.len() as u64);
    let mut output_bytes = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(&input_bytes);
    keccak.finalize(&mut output_bytes);
    let ret = output_bytes.to_vec();
    (ret, cost)
}

pub(crate) fn ripemd(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
    let cost = CostChange::deduct(gas::CRYPTO_RIPEMD160_PER_BYTE * input_bytes.len() as u64);
    let mut hasher = Ripemd160::new();
    hasher.update(&input_bytes);
    let ret = hasher.finalize().to_vec();
    (ret, cost)
}

pub(crate) fn verify_ed25519_signature(
    message: Vec<u8>,
    signature: [u8; 64],
    pub_key: [u8; 32],
) -> OperationReceipt<Result<i32, anyhow::Error>> {
    let cost = CostChange::deduct(gas::crypto_verify_ed25519_signature_cost(message.len()));
    let public_key = match ed25519_dalek::VerifyingKey::from_bytes(&pub_key) {
        Ok(public_key) => public_key,
        Err(e) => return (Err(e.into()), cost),
    };
    let dalek_signature = ed25519_dalek::Signature::from_bytes(&signature);
    let is_ok = public_key.verify(&message, &dalek_signature).is_ok();

    (Ok(is_ok as i32), cost)
}

/// Helper function to calculate the length of the storage trie key for gas charging purposes
pub (crate) fn storage_trie_traversed_key_len(
    version: TxnVersion,
    address: &PublicAddress,
    key: &[u8],
) -> usize {
    match version {
        // protocol v0.4.0 (using TransactionV1) included extra address length on top of Account Trie key length
        TxnVersion::V1 => gas::ACCOUNT_TRIE_KEY_LENGTH + address.len() + key.len(),
        // protocol v0.5.0 (using TransactionV2) removes extra address length,
        // and specifies that hashing is peformed on Storage Trie keys if longer than or equal to 32 bytes
        // due to updates in the World State implementation
        TxnVersion::V2 => {
            gas::ACCOUNT_TRIE_KEY_LENGTH
            + 
            if key.len() < 32 {
                key.len()
            } else {
                KECCAK256_LENGTH as usize
            }
        }
    }
}

pub (crate) fn storage_trie_key_hash_cost(txn_version: TxnVersion, key: &[u8]) -> u64 {
    // protocol v0.4.0 (using TransactionV1) did not hash the key
    // protocol v0.5.0 (using TransactionV2) hashes the key if longer than or equal to 32 bytes
    match txn_version {
        TxnVersion::V1 => 0,
        TxnVersion::V2 => {
            if key.len() < 32 {
                0
            } else {
                gas::CRYPTO_KECCAK256_PER_BYTE * key.len() as u64
            }
        }
    }
}
