/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct that serves as a cache layer on top of World State.
//!
//! There are two data caches:
//! - `reads` (first-hand data obtained from world state)
//! - `writes` (the data pended to commit to world state)
//!
//! The cache layer also measures gas consumption for the read-write operations.
//!
//! In Read Operation, `writes` is accessed first. If data is not found, search `reads`. If it fails in both Sets,
//! then finally World State is accessed. The result will then be cached to `reads`.
//!
//! In Write Operation, it first performs a Read Operation, and then updates the `writes` with the newest data.
//!
//! At the end of state transition, if it succeeds, the data in `writes` will be committed to World State. Otherwise,
//! `writes` is discarded without any changes to World State.

use std::{cell::RefCell, collections::HashMap};

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{
    keys::AppKey,
    states::{AccountStorageState, WorldState},
    storage::WorldStateStorage,
};
use wasmer::Store;

use crate::{
    contract::{self, Module, SmartContractContext},
    cost::CostChange,
    gas,
};

/// ReadWriteSet defines data cache for Read-Write opertaions during state transition.
#[derive(Clone)]
pub(crate) struct ReadWriteSet<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    /// World State services as the data source
    pub ws: WorldState<S>,
    /// writes stores key-value pairs for Write operations. It stores the data that is pending to store into world state
    pub writes: HashMap<CacheKey, CacheValue>,
    /// reads stores key-value pairs from Read operations. It is de facto the original data read from world state.
    pub reads: RefCell<HashMap<CacheKey, Option<CacheValue>>>,
    /// write_gas is protocol defined cost that to-be-charged write operation has been executed
    pub write_gas: CostChange,
    /// read_gas is protocol defined cost that to-be-charged read operation has been executed
    pub read_gas: RefCell<CostChange>,
}

impl<S> ReadWriteSet<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    pub fn new(ws: WorldState<S>) -> Self {
        Self {
            ws,
            writes: HashMap::new(),
            reads: RefCell::new(HashMap::new()),
            write_gas: CostChange::default(),
            read_gas: RefCell::new(CostChange::default()),
        }
    }

    // TODO remove
    /// get the balance from readwrite set. It key is not found, then get from world state and then cache it.
    pub fn balance(&self, address: PublicAddress) -> (u64, CostChange) {
        match self.get(CacheKey::Balance(address)) {
            (Some(CacheValue::Balance(value)), cost) => (value, cost),
            _ => panic!(),
        }
    }

    // TODO remove
    /// set balance to write set. This operation does not write to world state immediately
    pub fn set_balance(&mut self, address: PublicAddress, balance: u64) -> CostChange {
        self.set(CacheKey::Balance(address), CacheValue::Balance(balance))
    }

    /// remove cached writes and return the value
    pub fn purge_balance(&mut self, address: PublicAddress) -> u64 {
        let (balance, _) = self.balance(address);
        let key = CacheKey::Balance(address);
        self.writes.remove(&key);
        balance
    }

    /// get the contract code from readwrite set. It key is not found, then get from world state and then cache it.
    pub fn code(&self, address: PublicAddress) -> (Option<Vec<u8>>, CostChange) {
        match self.get(CacheKey::ContractCode(address)) {
            (Some(CacheValue::ContractCode(value)), cost) => (Some(value), cost),
            (None, cost) => (None, cost),
            _ => panic!(),
        }
    }

    /// get the contract code from smart contract cache. It it is not found, then get from read write set, i.e. code()
    pub fn code_from_sc_cache(
        &self,
        address: PublicAddress,
        sc_context: &SmartContractContext,
    ) -> (Option<(Module, Store)>, CostChange) {
        let wasmer_store = sc_context.instantiate_store();
        let cached_module = match &sc_context.cache {
            Some(sc_cache) => contract::Module::from_cache(address, sc_cache, &wasmer_store),
            None => None,
        };

        // found from Smart Contract Cache
        if let Some(module) = cached_module {
            let cost_change = CostChange::deduct(gas::get_code_cost(module.bytes_length()));
            *self.read_gas.borrow_mut() += cost_change;
            return (Some((module, wasmer_store)), cost_change);
        }

        // found from read write set or world state
        let (bytes, cost_change) = self.code(address);
        let contract_code = match bytes {
            Some(bs) => bs,
            None => return (None, cost_change),
        };

        // build module
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
            Err(_) => return (None, cost_change),
        };

        (Some((module, wasmer_store)), cost_change)
    }

    /// set contract bytecode. This operation does not write to world state immediately
    pub fn set_code(&mut self, address: PublicAddress, code: Vec<u8>) -> CostChange {
        self.set(
            CacheKey::ContractCode(address),
            CacheValue::ContractCode(code),
        )
    }

    /// get the CBI version of the contract
    pub fn cbi_version(&self, address: PublicAddress) -> (Option<u32>, CostChange) {
        match self.get(CacheKey::CBIVersion(address)) {
            (Some(CacheValue::CBIVersion(value)), cost) => (Some(value), cost),
            (None, cost) => (None, cost),
            _ => panic!(),
        }
    }

    /// set cbi version. This operation does not write to world state immediately
    pub fn set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) -> CostChange {
        self.set(
            CacheKey::CBIVersion(address),
            CacheValue::CBIVersion(cbi_version),
        )
    }

    // TODO remove
    /// get the contract storage from readwrite set. It key is not found, then get from world state and then cache it.
    pub fn app_data(
        &self,
        address: PublicAddress,
        app_key: AppKey,
    ) -> (Option<Vec<u8>>, CostChange) {
        match self.get(CacheKey::App(address, app_key)) {
            (Some(CacheValue::App(value)), cost) => {
                if value.is_empty() {
                    (None, cost)
                } else {
                    (Some(value), cost)
                }
            }
            (None, cost) => (None, cost),
            _ => panic!(),
        }
    }

    // TODO remove
    /// set value to contract storage. This operation does not write to world state immediately
    pub fn set_app_data(
        &mut self,
        address: PublicAddress,
        app_key: AppKey,
        value: Vec<u8>,
    ) -> CostChange {
        self.set(CacheKey::App(address, app_key), CacheValue::App(value))
    }

    /// set value to contract storage. This operation does not write to world state immediately.
    /// It is gas-free operation.
    pub fn set_app_data_uncharged(
        &mut self,
        address: PublicAddress,
        app_key: AppKey,
        value: Vec<u8>,
    ) {
        self.writes
            .insert(CacheKey::App(address, app_key), CacheValue::App(value));
    }

    pub fn contains_new(&self, key: &CacheKey) -> bool {
        self.writes.get(key).filter(|v| v.len() != 0).is_some()
            || self
                .reads
                .borrow()
                .get(key)
                .filter(|v| v.is_some())
                .is_some()
    }

    pub fn contains_in_storage_new(&self, address: PublicAddress, app_key: &AppKey) -> bool {
        self.ws.contains().storage_value(&address, app_key)
    }

    // TODO remove
    /// check if App Key already exists
    pub fn contains_app_data(&self, address: PublicAddress, app_key: AppKey) -> bool {
        let cache_key = CacheKey::App(address, app_key.clone());

        // charge gas for contains and charge gas
        *self.read_gas.borrow_mut() += CostChange::deduct(gas::contains_cost(cache_key.len()));

        // check from the value that was previously written/read
        self.writes
            .get(&cache_key)
            .filter(|v| v.len() != 0)
            .is_some()
            || self
                .reads
                .borrow()
                .get(&cache_key)
                .filter(|v| v.is_some())
                .is_some()
            || self.ws.contains().storage_value(&address, &app_key)
    }

    /// check if App Key already exists. It is gas-free operation.
    pub fn contains_app_data_from_account_storage_state(
        &self,
        account_storage_state: &AccountStorageState<S>,
        app_key: AppKey,
    ) -> bool {
        let address = account_storage_state.address();
        let cache_key = CacheKey::App(address, app_key.clone());

        // check from the value that was previously written/read
        self.writes
            .get(&cache_key)
            .filter(|v| v.len() != 0)
            .is_some()
            || self
                .reads
                .borrow()
                .get(&cache_key)
                .filter(|v| v.is_some())
                .is_some()
            || self
                .ws
                .contains()
                .storage_value_from_account_storage_state(account_storage_state, &app_key)
    }

    /// Get app data given a account storage state. It is gas-free operation.
    pub fn app_data_from_account_storage_state(
        &self,
        account_storage_state: &AccountStorageState<S>,
        app_key: AppKey,
    ) -> Option<Vec<u8>> {
        let address = account_storage_state.address();
        let cache_key = CacheKey::App(address, app_key.clone());

        match self.writes.get(&cache_key) {
            Some(CacheValue::App(value)) => return Some(value.clone()),
            Some(_) => panic!(),
            None => {}
        }

        match self.reads.borrow().get(&cache_key) {
            Some(Some(CacheValue::App(value))) => return Some(value.clone()),
            Some(None) => return None,
            Some(_) => panic!(),
            None => {}
        }

        self.ws
            .cached_get()
            .storage_value(account_storage_state.address(), &app_key)
            .or_else(|| account_storage_state.get(&app_key))
    }

    pub fn get_new(&self, key: &CacheKey) -> Option<CacheValue> {
        if let Some(value) = self.writes.get(key) {
            return Some(value.clone());
        }

        // 2. Return the value that was read eariler in the transaction
        if let Some(value) = self.reads.borrow().get(key) {
            return value.clone();
        }

        // 3. Get the value from world state
        let value = key.get_from_world_state(&self.ws);

        // 4. Cache to reads
        self.reads.borrow_mut().insert(key.clone(), value.clone());
        value
    }

    // TODO remove
    /// Lowest level of get operation. It gets latest value from readwrite set. It key is not found, then get from world state and then cache it.
    fn get(&self, key: CacheKey) -> (Option<CacheValue>, CostChange) {
        // 1. Return the value that was written earlier in the transaction ('read-your-write' semantics).
        if let Some(value) = self.writes.get(&key) {
            let cost_change = self.charge_read_cost(&key, Some(value));
            return (Some(value.clone()), cost_change);
        }

        // 2. Return the value that was read eariler in the transaction
        if let Some(value) = self.reads.borrow().get(&key) {
            let cost_change = self.charge_read_cost(&key, value.as_ref());
            return (value.clone(), cost_change);
        }

        // 3. Get the value from world state
        let value = key.get_from_world_state(&self.ws);
        let cost_change = self.charge_read_cost(&key, value.as_ref());

        // 4. Cache to reads
        self.reads.borrow_mut().insert(key, value.clone());

        (value, cost_change)
    }

    pub fn set_new(&mut self, key: CacheKey, value: CacheValue) {
        self.writes.insert(key, value);
    }

    /// lowest level of set operation. It inserts to Write Set and returns the gas cost for this set operation.
    fn set(&mut self, key: CacheKey, value: CacheValue) -> CostChange {
        let key_len = key.len();
        let new_val_len = value.len();

        // 1. Get the length of original value and Charge for read cost
        let old_val_len = self.get(key.clone()).0.map_or(0, |v| v.len());

        // 2. Insert to write set
        self.writes.insert(key, value);

        // 3. Calculate gas cost
        self.charge_write_cost(key_len, old_val_len, new_val_len)
    }

    // TODO remove
    fn charge_read_cost(&self, key: &CacheKey, value: Option<&CacheValue>) -> CostChange {
        let cost_change = match key {
            CacheKey::ContractCode(_) => {
                CostChange::deduct(gas::get_code_cost(value.as_ref().map_or(0, |v| v.len())))
            }
            _ => CostChange::deduct(gas::get_cost(
                key.len(),
                value.as_ref().map_or(0, |v| v.len()),
            )),
        };
        *self.read_gas.borrow_mut() += cost_change;
        cost_change
    }

    fn charge_write_cost(
        &mut self,
        key_len: usize,
        old_val_len: usize,
        new_val_len: usize,
    ) -> CostChange {
        let new_cost_change =
            // old_val_len is obtained from Get so the cost of reading the key is already charged
            CostChange::reward(gas::set_cost_delete_old_value(key_len, old_val_len, new_val_len)) +
            CostChange::deduct(gas::set_cost_write_new_value(new_val_len)) +
            CostChange::deduct(gas::set_cost_rehash(key_len));
        self.write_gas += new_cost_change;
        new_cost_change
    }

    pub fn commit_to_world_state(self) -> WorldState<S> {
        let mut ws = self.ws;
        // apply changes to world state
        self.writes.into_iter().for_each(|(cache_key, new_value)| {
            new_value.set_to_world_state(cache_key, &mut ws);
        });
        ws.commit();
        ws
    }
}

/// CacheKey is the key for state changes cache in Runtime. It is different with world state Key or App Key for
/// being useful in:
/// - data read write cache
/// - components in gas cost calculation
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) enum CacheKey {
    App(PublicAddress, AppKey),
    Balance(PublicAddress),
    ContractCode(PublicAddress),
    CBIVersion(PublicAddress),
}

impl CacheKey {
    /// length of the value as an input to gas calculation
    pub fn len(&self) -> usize {
        match self {
            CacheKey::App(address, key) => {
                gas::ACCOUNT_STATE_KEY_LENGTH + address.len() + key.len()
            }
            CacheKey::Balance(_) | CacheKey::ContractCode(_) | CacheKey::CBIVersion(_) => {
                gas::ACCOUNT_STATE_KEY_LENGTH
            }
        }
    }

    /// get_from_world_state gets value from world state according to CacheKey
    pub fn get_from_world_state<S>(&self, ws: &WorldState<S>) -> Option<CacheValue>
    where
        S: WorldStateStorage + Send + Sync + Clone,
    {
        match &self {
            CacheKey::App(address, app_key) => {
                ws.storage_value(address, app_key).map(CacheValue::App)
            }
            CacheKey::Balance(address) => Some(CacheValue::Balance(ws.balance(address.to_owned()))),
            CacheKey::ContractCode(address) => {
                ws.code(address.to_owned()).map(CacheValue::ContractCode)
            }
            CacheKey::CBIVersion(address) => ws
                .cbi_version(address.to_owned())
                .map(CacheValue::CBIVersion),
        }
    }
}

/// CacheValue is the cached write operations that are pending to be applied to world state.
/// It is used as
/// - intermediate data which could be dropped later.
/// - write information for gas calculation
#[derive(Clone, Debug)]
pub(crate) enum CacheValue {
    App(Vec<u8>),
    Balance(u64),
    ContractCode(Vec<u8>),
    CBIVersion(u32),
}

impl CacheValue {
    /// length of the value as an input to gas calculation
    pub fn len(&self) -> usize {
        match self {
            CacheValue::App(value) => value.len(),
            CacheValue::Balance(balance) => std::mem::size_of_val(balance),
            CacheValue::ContractCode(code) => code.len(),
            CacheValue::CBIVersion(cbi_version) => std::mem::size_of_val(cbi_version),
        }
    }

    /// set_all_to_world_state performs setting cache values to world state according to CacheKey
    fn set_to_world_state<S>(self, key: CacheKey, ws: &mut WorldState<S>)
    where
        S: WorldStateStorage + Send + Sync + Clone,
    {
        match self {
            CacheValue::App(value) => {
                if let CacheKey::App(address, app_key) = key {
                    ws.cached().set_storage_value(address, app_key, value);
                }
            }
            CacheValue::Balance(value) => {
                if let CacheKey::Balance(address) = key {
                    ws.cached().set_balance(address, value);
                }
            }
            CacheValue::ContractCode(value) => {
                if let CacheKey::ContractCode(address) = key {
                    ws.cached().set_code(address, value);
                }
            }
            CacheValue::CBIVersion(value) => {
                if let CacheKey::CBIVersion(address) = key {
                    ws.cached().set_cbi_version(address, value);
                }
            }
        }
    }
}
