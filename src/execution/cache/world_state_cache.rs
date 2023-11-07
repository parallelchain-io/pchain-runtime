/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct that serves as a cache layer on top of World State.
//!
//!
//! The caches are split into different categories representing the types of data that
//! are stored in an Account
//! // TODO excluding nonce?
//!
//! Within each data category, there are two sets of caches
//! - `reads` (data read first-hand from world state)
//! - `writes` (data pending to be written to world state)
//!
//! Procedure of a Read operation: First, `writes` is checked. If data is not found, search in `reads`.
//! If data is still not found, access the World State. The result, if retrieved, will then be cached to `reads`.
//!
//! Procedure of a Write operation: The `writes` cache is updated with the latest data.
//!
//! At the end of a successful state transition, the data in `writes` will be written to World State. Otherwise,
//! `writes` is discarded without any changes to World State.

use std::{cell::RefCell, collections::HashMap};

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{VersionProvider, WorldState, DB};

// TODO - change to 'static

/// ReadWriteSet defines data cache for Read-Write opertaions during state transition.
#[derive(Clone)]
pub(crate) struct WorldStateCache<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// World State serves as the data source
    pub ws: WorldState<'a, S, V>,
    pub balances: CacheBalance,
    pub cbi_versions: CacheCBIVersion,
    pub contract_codes: CacheContractCode,
    pub storage_data: CacheStorageData,
}

impl<'a, S, V> WorldStateCache<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    pub fn new(ws: WorldState<'a, S, V>) -> Self {
        Self {
            ws,
            balances: Default::default(),
            cbi_versions: Default::default(),
            contract_codes: Default::default(),
            storage_data: CacheStorageData {
                reads: RefCell::new(HashMap::new()),
                writes: HashMap::new(),
            },
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
        self.cbi_versions.revert();
        self.contract_codes.revert();
        self.storage_data.revert();
    }

    // TODO 90 does ws.storage_trie need &mut?
    // TODO 92 collapose with contains_app_data
    /// check if a key exists in storage data for a particular address
    pub fn contains_storage_data(&mut self, address: &PublicAddress, key: &[u8]) -> bool {
        self.storage_data
            // TODO
            .contains(&(address.clone(), key.to_vec()), |(_, key)| -> bool {
                // TODO remove unwrap()
                self.ws
                    .storage_trie(address)
                    .unwrap()
                    .contains(key)
                    .unwrap()
            })
    }

    // TODO 90 does ws.storage_trie need &mut?
    // TODO 92 collapose with get_storage_data
    /// get data for a key in storage data for a particular address
    pub fn storage_data(&mut self, address: &PublicAddress, key: &[u8]) -> Option<Vec<u8>> {
        self.storage_data
            .get(&(address.clone(), key.to_vec()), |(address, key)| {
                // TODO remove unwrap()
                self.ws.storage_trie(address).unwrap().get(key).unwrap()
            })
    }

    pub fn get_balance(&self, address: &PublicAddress) -> u64 {
        self.balances
            .get(address, |key| self.ws.account_trie().balance(key).ok())
            // TODO remove unwrap()
            .unwrap()
    }

    pub fn set_balance(&mut self, address: PublicAddress, balance: u64) {
        self.balances.set(address, balance);
    }

    pub fn get_cbi_version(&self, address: &PublicAddress) -> Option<u32> {
        self.cbi_versions
            // TODO remove ok()
            .get(address, |key| self.ws.account_trie().cbi_version(key).ok())
    }

    pub fn set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        self.cbi_versions.set(address, cbi_version);
    }

    pub fn get_contract_code(&self, address: &PublicAddress) -> Option<Vec<u8>> {
        self.contract_codes.get(address, |key| {
            // TODO remove unwrap
            self.ws.account_trie().code(key).ok().unwrap()
        })
    }

    pub fn set_contract_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        self.contract_codes.set(address, code);
    }

    pub fn get_storage_data(&mut self, address: PublicAddress, key: &[u8]) -> Option<Vec<u8>> {
        self.storage_data
            .get(&(address, key.to_vec()), |(address, key)| {
                // TODO remove unwrap()
                self.ws.storage_trie(address).unwrap().get(key).unwrap()
            })
    }

    /// set key-value to storage for a particular address
    pub fn set_storage_data(&mut self, address: PublicAddress, key: &[u8], value: Vec<u8>) {
        self.storage_data.set((address, key.to_vec()), value);
    }

    // TODO 92 collapose with contains_app_data
    pub fn contains_app_data(&mut self, address: PublicAddress, key: &[u8]) -> bool {
        self.storage_data
            .contains(&(address, key.to_vec()), |(address, key)| {
                // TODO remove unwrap()
                self.ws
                    .storage_trie(address)
                    .unwrap()
                    .contains(key)
                    .unwrap()
            })
    }

    pub fn commit_to_world_state(self) -> WorldState<'a, S, V> {
        let mut ws = self.ws;
        // apply changes to world state
        self.balances
            .writes
            .into_iter()
            .for_each(|(address, balance)| {
                ws.account_trie_mut().set_balance(&address, balance);
            });
        self.cbi_versions
            .writes
            .into_iter()
            .for_each(|(address, version)| {
                ws.account_trie_mut().set_cbi_version(&address, version);
            });
        self.contract_codes
            .writes
            .into_iter()
            .for_each(|(address, code)| {
                ws.account_trie_mut().set_code(&address, code);
            });
        self.storage_data
            .writes
            .into_iter()
            .for_each(|((address, key), value)| {
                ws.storage_trie_mut(&address)
                    .unwrap()
                    .set(&key, value)
                    .unwrap()
            });
        ws
    }
}

type CacheBalance = CacheData<PublicAddress, u64>;
type CacheCBIVersion = CacheData<PublicAddress, u32>;
type CacheContractCode = CacheData<PublicAddress, Vec<u8>>;
type CacheStorageData = CacheData<(PublicAddress, Vec<u8>), Vec<u8>>;

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
    V: CacheValue + Clone,
{
    /// Get latest value from readwrite set. If not found, get from world state and then cache it.
    pub fn get<WS: FnOnce(&K) -> Option<V>>(&self, key: &K, ws_get: WS) -> Option<V> {
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
    pub fn contains<WS: FnOnce(&K) -> bool>(&self, key: &K, ws_contains: WS) -> bool {
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
