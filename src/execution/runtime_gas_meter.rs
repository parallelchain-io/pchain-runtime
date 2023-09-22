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

use crate::{
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
            Some(CacheValue::App(value)) => Some(value),
            None => None,
            _ => panic!("Retrieved data not of App variant"),
        }
    }

    /// Check if App key has non-empty data
    pub fn ws_contains_app_data(&self, address: PublicAddress, app_key: AppKey) -> bool {
        let cache_key = CacheKey::App(address, app_key.clone());
        // TODO consistent in charging
        self.ws_charge_contains_cost(&cache_key);

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
    fn ws_charge_contains_cost(&self, key: &CacheKey) {
        *self.command_gas_used.borrow_mut() += CostChange::deduct(gas::contains_cost(key.len()));
    }

    fn ws_charge_read_cost(&self, key: &CacheKey, value: Option<&CacheValue>) {
        let cost_change = match key {
            CacheKey::ContractCode(_) => {
                CostChange::deduct(gas::get_code_cost(value.as_ref().map_or(0, |v| v.len())))
            }
            _ => CostChange::deduct(gas::get_cost(
                key.len(),
                value.as_ref().map_or(0, |v| v.len()),
            )),
        };
        *self.command_gas_used.borrow_mut() += cost_change
    }

    fn ws_charge_write_cost(&self, key_len: usize, old_val_len: usize, new_val_len: usize) {
        let new_cost_change =
                // old_val_len is obtained from Get so the cost of reading the key is already charged
                CostChange::reward(gas::set_cost_delete_old_value(key_len, old_val_len, new_val_len)) +
                CostChange::deduct(gas::set_cost_write_new_value(new_val_len)) +
                CostChange::deduct(gas::set_cost_rehash(key_len));
        *self.command_gas_used.borrow_mut() += new_cost_change;
    }

    fn ws_get(&self, key: CacheKey) -> Option<CacheValue> {
        let rw_set = self.rw_set.lock().unwrap();
        let value = rw_set.get_new(&key);
        drop(rw_set);
        self.ws_charge_read_cost(&key, value.as_ref());
        value
    }

    fn ws_set(&mut self, key: CacheKey, value: CacheValue) {
        let key_len = key.len();
        let new_val_len = value.len();
        let old_val_len = self.ws_get(key.clone()).map_or(0, |v| v.len());
        self.ws_charge_write_cost(key_len, old_val_len, new_val_len);

        let mut rw_set = self.rw_set.lock().unwrap();
        rw_set.set_new(key, value);
        drop(rw_set);
    }

    fn ws_contains(&self, key: &CacheKey) -> bool {
        // TODO slightly inconsistent, not charging here
        let rw_set = self.rw_set.lock().unwrap();
        rw_set.contains_new(key)
    }
}
