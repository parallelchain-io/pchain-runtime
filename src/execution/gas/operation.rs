use ed25519_dalek::Verifier;
use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::storage::WorldStateStorage;
use ripemd::Ripemd160;
use sha2::{Digest as sha256_digest, Sha256};
use tiny_keccak::{Hasher as _, Keccak};

use crate::{
    contract::{wasmer::memory::MemoryContext, ContractModule, SmartContractContext},
    execution::cache::{CacheKey, CacheValue, WorldStateCache},
    gas, types::TxnVersion,
};

use super::CostChange;

pub(crate) type OperationReceipt<T> = (T, CostChange);

pub(crate) fn ws_set<S>(
    version: TxnVersion,
    ws_cache: &mut WorldStateCache<S>,
    key: CacheKey,
    value: CacheValue,
) -> OperationReceipt<()>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    let key_len = match version {
        TxnVersion::V1 => key.len_v1(),
        TxnVersion::V2 => key.len_v2()
    };

    let new_val_len = value.len();
    let (old_val_len, get_cost) = ws_get(ws_cache, key.clone());
    let old_val_len = old_val_len.map_or(0, |v| v.len());

    // old_val_len is obtained from Get so the cost of reading the key is already charged
    let set_cost = CostChange::reward(gas::set_cost_delete_old_value(
        key_len,
        old_val_len,
        new_val_len,
    )) + CostChange::deduct(gas::set_cost_write_new_value(new_val_len))
        + CostChange::deduct(gas::set_cost_rehash(key_len));

    ws_cache.set(key, value);

    ((), get_cost + set_cost)
}

pub(crate) fn ws_get<S>(
    ws_cache: &WorldStateCache<S>,
    key: CacheKey,
) -> OperationReceipt<Option<CacheValue>>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    let value = ws_cache.get(&key);

    let get_cost = match key {
        CacheKey::ContractCode(_) => {
            CostChange::deduct(gas::get_code_cost(value.as_ref().map_or(0, |v| v.len())))
        }
        _ => CostChange::deduct(gas::get_cost(
            key.len_v1(),
            value.as_ref().map_or(0, |v| v.len()),
        )),
    };
    (value, get_cost)
}

pub(crate) fn ws_contains<S>(
    ws_cache: &WorldStateCache<S>,
    key: &CacheKey,
) -> OperationReceipt<bool>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    let cost_change = CostChange::deduct(gas::contains_cost(key.len_v1()));
    let ret = ws_cache.contains(key);
    (ret, cost_change)
}

pub(crate) fn ws_get_cached_contract<S>(
    ws_cache: &WorldStateCache<S>,
    sc_context: &SmartContractContext,
    address: PublicAddress,
) -> OperationReceipt<Option<ContractModule>>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    // check smart contract cache
    if let Some(contract_module) = ContractModule::from_cache(address, sc_context) {
        let contract_get_cost =
            CostChange::deduct(gas::get_code_cost(contract_module.bytes_length()));
        return (Some(contract_module), contract_get_cost);
    }

    // else check ws and charge
    let (value, contract_get_cost) = ws_get(ws_cache, CacheKey::ContractCode(address));
    let contract_code = match value {
        Some(CacheValue::ContractCode(value)) => value,
        None => return (None, contract_get_cost),
        _ => panic!("Retrieved data not of ContractCode variant"),
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

pub(crate) fn command_output_set_return_values(
    command_output_return_values: &mut Option<Vec<u8>>,
    return_values: Vec<u8>,
) -> OperationReceipt<()> {
    let cost = CostChange::deduct(gas::blockchain_return_values_cost(return_values.len()));
    *command_output_return_values = Some(return_values);
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
