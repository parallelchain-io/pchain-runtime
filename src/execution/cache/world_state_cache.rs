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

/// ReadWriteSet defines data cache for Read-Write opertaions during state transition.
#[derive(Clone)]
pub(crate) struct WorldStateCache<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    /// World State services as the data source
    pub ws: WorldState<S>,

    pub balances: CacheBalance,
    pub cbi_verions: CacheCBIVersion,
    pub contract_codes: CacheContractCode,
    pub app_data: CacheAppData,
}

impl<S> WorldStateCache<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    pub fn new(ws: WorldState<S>) -> Self {
        Self {
            ws,
            balances: Default::default(),
            cbi_verions: Default::default(),
            contract_codes: Default::default(),
            app_data: CacheAppData { reads: RefCell::new(HashMap::new()), writes: HashMap::new() },
        }
    }

    /// remove cached writes and return the value,
    /// gas free operation, only used for accounting during charge phase
    pub fn purge_balance(&mut self, address: PublicAddress) -> u64 {
        let balance = self.get_balance(&address);
        self.balances.writes.remove(&address);
        balance
    }

    /// reverts changes to read-write set
    pub fn revert(&mut self) {
        self.balances.revert();
        self.cbi_verions.revert();
        self.contract_codes.revert();
        self.app_data.revert();
    }

    /// check if App Key already exists. It is gas-free operation.
    pub fn contains_app_data_from_account_storage_state(
        &self,
        account_storage_state: &AccountStorageState<S>,
        app_key: AppKey,
    ) -> bool {
        let address = account_storage_state.address();
        self.app_data.contains(&(address, app_key), |(_, app_key)| {
            self.ws
                .contains()
                .storage_value_from_account_storage_state(account_storage_state, app_key)
        })
    }

    /// Get app data given a account storage state. It is gas-free operation.
    pub fn app_data_from_account_storage_state(
        &self,
        account_storage_state: &AccountStorageState<S>,
        app_key: AppKey,
    ) -> Option<Vec<u8>> {
        let address = account_storage_state.address();
        self.app_data.get(&(address, app_key), |(address, app_key)| {
            self.ws
                .cached_get()
                .storage_value(*address, app_key)
                .or_else(|| account_storage_state.get(app_key))
        })
    }

    pub fn get_balance(&self, address: &PublicAddress) -> u64 {
        self.balances.get(address, |key| Some(self.ws.balance(*key))).unwrap()
    }

    pub fn set_balance(&mut self, address: PublicAddress, balance: u64) {
        self.balances.set(address, balance);
    }

    pub fn get_cbi_version(&self, address: &PublicAddress) -> Option<u32> {
        self.cbi_verions.get(address, |key| self.ws.cbi_version(*key))
    }

    pub fn set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        self.cbi_verions.set(address, cbi_version);
    }

    pub fn get_contract_code(&self, address: &PublicAddress) -> Option<Vec<u8>> {
        self.contract_codes.get(address, |key| self.ws.code(*key) )
    }

    pub fn set_contract_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        self.contract_codes.set(address, code);
    }

    pub fn get_app_data(&self, address: PublicAddress, app_key: AppKey) -> Option<Vec<u8>> {
        self.app_data.get(&(address, app_key), |(address, app_key)| self.ws.storage_value(address, app_key))
    }

    /// set value to contract storage. This operation does not write to world state immediately.
    /// It is gas-free operation.
    pub fn set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        self.app_data.set((address, app_key), value);
    }

    pub fn contains_app_data(&self, address: PublicAddress, app_key: AppKey) -> bool {
        self.app_data.contains(&(address, app_key), |(address, app_key)| {
            self.ws.contains().storage_value(address, app_key)
        })
    }

    pub fn commit_to_world_state(self) -> WorldState<S> {
        let mut ws = self.ws;
        // apply changes to world state
        self.balances.writes.into_iter().for_each(|(address, balance)| {
            ws.cached().set_balance(address, balance);
        });
        self.cbi_verions.writes.into_iter().for_each(|(address, version)| {
            ws.cached().set_cbi_version(address, version);
        });
        self.contract_codes.writes.into_iter().for_each(|(address, code)| {
            ws.cached().set_code(address, code);
        });
        self.app_data.writes.into_iter().for_each(|((address, app_key), value)| {
            ws.cached().set_storage_value(address, app_key, value);
        });

        ws.commit();
        ws
    }
}

type CacheBalance = CacheData<PublicAddress, u64>;
type CacheCBIVersion = CacheData<PublicAddress, u32>;
type CacheContractCode = CacheData<PublicAddress, Vec<u8>>;
type CacheAppData = CacheData<(PublicAddress, AppKey), Vec<u8>>;

pub(crate) trait CacheValue {
    fn len(&self) -> usize;
}

impl CacheValue for u64 {
    fn len(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

impl CacheValue for u32 {
    fn len(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

impl CacheValue for Vec<u8> {
    fn len(&self) -> usize {
        self.len()
    }
}

#[derive(Clone, Default)]
pub(crate) struct CacheData<K, V> {
    /// writes stores key-value pairs for Write operations. It stores the data that is pending to store into world state
    pub writes: HashMap<K, V>,
    /// reads stores key-value pairs from Read operations. It is de facto the original data read from world state.
    pub reads: RefCell<HashMap<K, Option<V>>>,
}

impl<K, V> CacheData<K, V>
where
    K: PartialEq + Eq + std::hash::Hash + Clone,
    V: CacheValue + Clone
{
    /// Get latest value from readwrite set. If not found, get from world state and then cache it.
    pub fn get<WS: FnOnce(&K)->Option<V>>(&self, key: &K, ws_get: WS) -> Option<V> {
        // 1. Return the value that was written earlier in the transaction ('read-your-write' semantics)
        if let Some(value) = self.writes.get(key) {
            return Some(value.clone());
        }

        // 2. Return the value that was read eariler in the transaction
        if let Some(value) = self.reads.borrow().get(key) {
            return value.clone();
        }

        // 3. Get the value from world state
        let value = ws_get(key);

        // 4. Cache to reads
        self.reads.borrow_mut().insert(key.clone(), value.clone());
        value
    }

    /// Insert to write set.
    pub fn set(&mut self, key: K, value: V) {
        self.writes.insert(key, value);
    }

    // Low Level Operations
    /// Check if this key is set before.
    pub fn contains<WS: FnOnce(&K)->bool>(&self, key: &K, ws_contains: WS) -> bool {
        // Check if readwrite set contains this key.
        self.writes.get(key).filter(|v| v.len() != 0).is_some()
        || self
            .reads
            .borrow()
            .get(key)
            .filter(|v| v.is_some())
            .is_some()
        // Check if world state contains this key.
        || ws_contains(key)
    }

    /// reverts changes to read-write set
    pub fn revert(&mut self) {
        self.reads.borrow_mut().clear();
        self.writes.clear();
    }
}