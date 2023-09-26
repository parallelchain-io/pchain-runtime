use core::panic;
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::{
    keys::AppKey,
    network::{constants::NETWORK_ADDRESS, network_account::NetworkAccountStorage},
    storage::WorldStateStorage,
};

use ed25519_dalek::Verifier;
use ripemd::Ripemd160;
use sha2::{Digest as sha256_digest, Sha256};
use tiny_keccak::{Hasher as _, Keccak};

use crate::{
    contract::{self, FuncError, SmartContractContext},
    cost::CostChange,
    gas,
    read_write_set::{CacheKey, CacheValue, ReadWriteSet},
};

/// GasMeter is a global struct that keeps track of gas used from operations OUTSIDE of a Wasmer guest instance (compute and memory access).
/// It implements a facade for all chargeable methods.
#[derive(Clone)]
pub(crate) struct RuntimeGasMeter<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub gas_limit: u64,

    pub total_gas_used_clamped: u64, // TODO 4 - temp keeping the total_gas_used_clamped field, but should remove if no use

    /// cumulative gas used for all executed commands
    total_command_gas_used: u64,

    /// stores txn inclusion gas separately because it is not considered to belong to a single command
    txn_inclusion_gas: u64,

    /// stores the gas used by current command,
    /// finalized and reset at the end of each command
    pub command_gas_used: RefCell<CostChange>,

    /// stores the list of events from exeuting a command, ordered by the sequence of emission
    pub command_logs: Vec<Log>,

    /// value returned by a call transaction using the `return_value` SDK function.
    /// It is None if the execution has not/did not return anything.
    pub command_return_value: Option<Vec<u8>>,

    rw_set: Arc<Mutex<ReadWriteSet<S>>>,
}

/// GasMeter implements NetworkAccountStorage with charegable read-write operations to world state
impl<'a, S> NetworkAccountStorage for RuntimeGasMeter<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.ws_get_app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec()))
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.ws_contains_app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec()))
    }

    fn set(&mut self, key: &[u8], value: Vec<u8>) {
        self.ws_set_app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec()), value)
    }

    fn delete(&mut self, key: &[u8]) {
        self.ws_set_app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec()), Vec::new())
    }
}

impl<'a, S> RuntimeGasMeter<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    // TODO CLEAN consider whether Arc/Mutex is really needed, or should the Arc/Mutex really be on TransitionContext parent
    pub fn new(rw_set: Arc<Mutex<ReadWriteSet<S>>>, gas_limit: u64) -> Self {
        Self {
            rw_set,
            gas_limit,
            // TODO 6 - check GasExhausted against centralized GasMeter `gas_limit` field instead of other fields
            total_command_gas_used: 0,
            txn_inclusion_gas: 0,
            total_gas_used_clamped: 0,
            command_gas_used: RefCell::new(CostChange::default()),
            command_logs: Vec::new(),
            command_return_value: None,
        }
    }

    //
    // Gas Accounting
    //

    /// method to bring in gas consumed in the Wasmer env due to
    /// 1) read and write to Wasmer memory,
    /// 2) compute cost
    pub fn charge_wasmer_gas(&mut self, gas: u64) {
        self.charge(CostChange::deduct(gas));
    }

    /// called after every command to reset command_gas_used
    pub fn finalize_command_gas(&mut self) {
        let net_cmd_gas_used = {
            let mut command_gas_ptr = self.command_gas_used.borrow_mut();
            let net_gas = command_gas_ptr.values().0;
            *command_gas_ptr = CostChange::default();
            net_gas
        };

        self.total_command_gas_used = self.total_command_gas_used.saturating_add(net_cmd_gas_used);

        // TODO CLEAN is this doing too much? Better to separate?
        self.command_logs.clear();
        self.command_return_value = None;
    }

    /// returns total gas for the transaction
    pub fn get_total_gas_used(&self) -> u64 {
        let pending_command_gas = (*self.command_gas_used.borrow()).values().0;
        self.txn_inclusion_gas
            .saturating_add(self.total_command_gas_used)
            .saturating_add(pending_command_gas)
    }

    pub fn get_max_remaining_gas(&self) -> u64 {
        self.gas_limit.saturating_sub(self.get_total_gas_used())
    }

    //
    // Facade methods for cryptographic operations on host machine callable by contracts
    //
    pub fn host_sha256(&self, input_bytes: Vec<u8>) -> Vec<u8> {
        self.charge(CostChange::deduct(
            gas::CRYPTO_SHA256_PER_BYTE * input_bytes.len() as u64,
        ));
        let mut hasher = Sha256::new();
        hasher.update(input_bytes);
        hasher.finalize().to_vec()
    }

    pub fn host_keccak256(&self, input_bytes: Vec<u8>) -> Vec<u8> {
        self.charge(CostChange::deduct(
            gas::CRYPTO_KECCAK256_PER_BYTE * input_bytes.len() as u64,
        ));

        let mut output_bytes = [0u8; 32];
        let mut keccak = Keccak::v256();
        keccak.update(&input_bytes);
        keccak.finalize(&mut output_bytes);
        output_bytes.to_vec()
    }

    pub fn host_ripemd(&self, input_bytes: Vec<u8>) -> Vec<u8> {
        self.charge(CostChange::deduct(
            gas::CRYPTO_RIPEMD160_PER_BYTE * input_bytes.len() as u64,
        ));
        let mut hasher = Ripemd160::new();
        hasher.update(&input_bytes);
        hasher.finalize().to_vec()
    }

    pub fn host_verify_ed25519_signature(
        &self,
        message: Vec<u8>,
        signature: Vec<u8>,
        pub_key: Vec<u8>,
    ) -> Result<i32, FuncError> {
        self.charge(CostChange::deduct(
            gas::crypto_verify_ed25519_signature_cost(message.len()),
        ));

        let public_key =
            ed25519_dalek::PublicKey::from_bytes(&pub_key).map_err(|_| FuncError::Internal)?;
        let dalek_signature =
            ed25519_dalek::Signature::from_bytes(&signature).map_err(|_| FuncError::Internal)?;
        let is_ok = public_key.verify(&message, &dalek_signature).is_ok();

        Ok(is_ok as i32)
    }

    //
    // Facade methods for Transaction Storage methods that cost gas
    //
    pub fn store_txn_pre_exec_inclusion_cost(&mut self, tx_size: usize, commands_len: usize) {
        // stored separately because it is not considered to belong to a single command
        self.txn_inclusion_gas = gas::tx_inclusion_cost(tx_size, commands_len);
    }

    pub fn store_txn_post_exec_log(&mut self, log_to_store: Log) {
        self.charge(CostChange::deduct(gas::blockchain_log_cost(
            log_to_store.topic.len(),
            log_to_store.value.len(),
        )));

        // TODO 1 - Wasm method caller needs to check on GasExhaustion
        // old code
        // env.consume_non_wasm_gas(cost_change);
        // if env.get_wasmer_remaining_points() == 0 {
        //     return Err(FuncError::GasExhaustionError);
        // }

        self.command_logs.push(log_to_store);
    }

    pub fn store_txn_post_exec_return_value(&mut self, ret_val: Vec<u8>) {
        self.charge(CostChange::deduct(gas::blockchain_return_values_cost(
            ret_val.len(),
        )));
        // TODO 2 - Wasm method caller needs to check on GasExhaustion

        // FYI callers from staking.rs check by calling phase::finalize_gas_consumption right after
        // old code
        // env.consume_non_wasm_gas(cost_change);
        // if env.get_wasmer_remaining_points() == 0 {
        //     return Err(FuncError::GasExhaustionError);
        // }

        self.command_return_value = Some(ret_val);
    }

    //
    // Facade methods for World State methods that cost gas
    //

    //
    // CONTAINS methods
    //
    /// Check if App key has non-empty data
    pub fn ws_contains_app_data(&self, address: PublicAddress, app_key: AppKey) -> bool {
        let cache_key = CacheKey::App(address, app_key.clone());

        // check from RW set first
        self.ws_contains(&cache_key) || {
            // if not found, check from storage
            let rw_set = self.rw_set.lock().unwrap();
            rw_set.contains_in_storage_uncharged(address, &app_key)
        }
    }

    //
    // GET methods
    //
    /// Gets app data from the read-write set.
    pub fn ws_get_app_data(&self, address: PublicAddress, key: AppKey) -> Option<Vec<u8>> {
        match self.ws_get(CacheKey::App(address, key)) {
            Some(CacheValue::App(value)) => {
                if value.is_empty() {
                    None
                } else {
                    Some(value)
                }
            }
            None => None,
            _ => panic!("Retrieved data not of App variant"),
        }
    }

    /// Get the balance from read-write set. It balance is not found, gets from WS and caches it.
    pub fn ws_get_balance(&self, address: PublicAddress) -> u64 {
        match self.ws_get(CacheKey::Balance(address)) {
            Some(CacheValue::Balance(value)) => value,
            _ => panic!(),
        }
    }

    pub fn ws_get_cached_contract(
        &self,
        address: PublicAddress,
        sc_context: &SmartContractContext,
    ) -> Option<(contract::Module, wasmer::Store)> {
        // charge contract cache and charge
        let wasmer_store = sc_context.instantiate_store();
        let cached_module = match &sc_context.cache {
            Some(sc_cache) => contract::Module::from_cache(address, sc_cache, &wasmer_store),
            None => None,
        };
        if let Some(module) = cached_module {
            let contract_get_cost = CostChange::deduct(gas::get_code_cost(module.bytes_length()));
            self.charge(contract_get_cost);
            return Some((module, wasmer_store));
        }

        // else check ws and charge
        let contract_code = match self.ws_get(CacheKey::ContractCode(address)) {
            Some(CacheValue::ContractCode(value)) => value,
            None => return None,
            _ => panic!("Retrieved data not of ContractCode variant"),
        };
        let module = match contract::Module::from_wasm_bytecode_unchecked(
            contract::CBI_VERSION,
            &contract_code,
            &wasmer_store,
        ) {
            Ok(module) => {
                // cache to sc_cache
                if let Some(sc_cache) = &sc_context.cache {
                    module.cache_to(address, &mut sc_cache.clone());
                }
                module
            }
            Err(_) => return None,
        };
        Some((module, wasmer_store))
    }

    pub fn ws_get_cbi_version(&self, address: PublicAddress) -> Option<u32> {
        match self.ws_get(CacheKey::CBIVersion(address)) {
            Some(CacheValue::CBIVersion(value)) => Some(value),
            None => None,
            _ => panic!(),
        }
    }

    //
    // SET methods
    //
    pub fn ws_set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        self.ws_set(CacheKey::App(address, app_key), CacheValue::App(value))
    }

    /// Sets balance in the write set. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        self.ws_set(CacheKey::Balance(address), CacheValue::Balance(value))
    }

    /// Sets CBI version in the write set. It does not write to WS immediately.
    pub fn ws_set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        self.ws_set(
            CacheKey::CBIVersion(address),
            CacheValue::CBIVersion(cbi_version),
        )
    }

    /// Sets contract bytecode in the write set. It does not write to WS immediately.
    pub fn ws_set_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        self.ws_set(
            CacheKey::ContractCode(address),
            CacheValue::ContractCode(code),
        )
    }

    //
    // Private Helpers
    //
    fn charge(&self, cost_change: CostChange) {
        *self.command_gas_used.borrow_mut() += cost_change;
    }

    fn ws_get(&self, key: CacheKey) -> Option<CacheValue> {
        let rw_set = self.rw_set.lock().unwrap();
        let value = rw_set.get_uncharged(&key);
        drop(rw_set);

        let get_cost = match key {
            CacheKey::ContractCode(_) => {
                CostChange::deduct(gas::get_code_cost(value.as_ref().map_or(0, |v| v.len())))
            }
            _ => CostChange::deduct(gas::get_cost(
                key.len(),
                value.as_ref().map_or(0, |v| v.len()),
            )),
        };
        self.charge(get_cost);
        value
    }

    fn ws_set(&mut self, key: CacheKey, value: CacheValue) {
        let key_len = key.len();

        let new_val_len = value.len();
        let old_val_len = self.ws_get(key.clone()).map_or(0, |v| v.len());
        // old_val_len is obtained from Get so the cost of reading the key is already charged

        let set_cost = CostChange::reward(gas::set_cost_delete_old_value(
            key_len,
            old_val_len,
            new_val_len,
        )) + CostChange::deduct(gas::set_cost_write_new_value(new_val_len))
            + CostChange::deduct(gas::set_cost_rehash(key_len));
        self.charge(set_cost);

        let mut rw_set = self.rw_set.lock().unwrap();
        rw_set.set_uncharged(key, value);
        drop(rw_set);
    }

    fn ws_contains(&self, key: &CacheKey) -> bool {
        self.charge(CostChange::deduct(gas::contains_cost(key.len())));
        let rw_set = self.rw_set.lock().unwrap();
        rw_set.contains_uncharged(key)
    }
}
