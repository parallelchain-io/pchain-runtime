use core::panic;
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use pchain_types::cryptography::PublicAddress;
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
    contract::FuncError,
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
    pub total_gas_used: RefCell<CostChange>,
    pub command_gas_used: RefCell<CostChange>,
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
    // TODO consider whether Arc is really needed, or can it removed after refactoring
    pub fn new(rw_set: Arc<Mutex<ReadWriteSet<S>>>) -> Self {
        Self {
            rw_set,
            // TODO remove hardcode, we are not chcecking against this limit now
            gas_limit: 1_000_000_000_u64,
            total_gas_used: RefCell::new(CostChange::default()),
            command_gas_used: RefCell::new(CostChange::default()),
        }
    }

    //
    // Gas Accounting
    //

    pub fn finalize_command_gas(&mut self) {
        let mut command_gas_used = self.command_gas_used.borrow_mut();
        let mut total_gas_used = self.total_gas_used.borrow_mut();
        *total_gas_used += *command_gas_used;
        *command_gas_used = CostChange::default();
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
    // Facade methods for World State methods that cost gas
    //

    /// Get the balance from read-write set. It balance is not found, gets from WS and caches it.
    pub fn ws_get_balance(&self, address: PublicAddress) -> u64 {
        match self.ws_get(CacheKey::Balance(address)) {
            Some(CacheValue::Balance(value)) => value,
            _ => panic!(),
        }
    }

    /// Gets contract storage (TODO, app_data?) from the read-write set.
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

    /// Check if App key has non-empty data
    pub fn ws_contains_app_data(&self, address: PublicAddress, app_key: AppKey) -> bool {
        let cache_key = CacheKey::App(address, app_key.clone());

        // check from RW set first
        self.ws_contains(&cache_key) || {
            // TODO this should be consistently wrapped?
            // if not found, check from storage
            let rw_set = self.rw_set.lock().unwrap();
            rw_set.contains_in_storage_new(address, &app_key)
        }
    }

    pub fn ws_set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        self.ws_set(CacheKey::App(address, app_key), CacheValue::App(value))
    }

    /// Sets balance in the write set. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        self.ws_set(CacheKey::Balance(address), CacheValue::Balance(value))
    }

    //
    // Private Helpers
    //
    fn charge(&self, cost_change: CostChange) {
        *self.command_gas_used.borrow_mut() += cost_change;
    }

    fn ws_get(&self, key: CacheKey) -> Option<CacheValue> {
        let rw_set = self.rw_set.lock().unwrap();
        let value = rw_set.get_new(&key);
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
        rw_set.set_new(key, value);
        drop(rw_set);
    }

    fn ws_contains(&self, key: &CacheKey) -> bool {
        self.charge(CostChange::deduct(gas::contains_cost(key.len())));
        let rw_set = self.rw_set.lock().unwrap();
        rw_set.contains_new(key)
    }
}
