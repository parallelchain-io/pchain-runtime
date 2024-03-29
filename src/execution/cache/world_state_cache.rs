/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! A cache layer for efficient operations on the World State.
//!
//! Acting as a gatekeeper, the [WorldStateCache] centralizes access control, ensuring that all operations on the World State are channeled through it.
//!
//! Primarily utilized by the [GasMeter](crate::gas::GasMeter), it directs the persistence of specific data types to their respective
//! sections within the World State's various tries.
//!
//! It also leverages caching, and batching of updates, to improve read and write peformance.

use std::{cell::RefCell, collections::HashMap};

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{VersionProvider, WorldState, DB};

/// Unified container for different caches representing the various types of data
///
/// Each data type for an Account is held in its own sets of cache, excluding nonces
/// which are not mutated by command execution (only after).
///
/// Within each data category, there are two sets of caches:
/// - `reads` (data read first-hand from World State)
/// - `writes` (data pending to be written to World State)
///
/// Procedure of a Read operation: First, `writes` is checked. If data is not found, search in `reads`.
/// If data is still not found, access the World State. The result, if retrieved, will then be cached to `reads`.
///
/// Procedure of a Write operation: The `writes` cache is updated with the latest data.
/// Writing to the actual World State happens in a separate, consolidated step.
///
/// At the end of a successful state transition, the data in `writes` will be written to World State. Otherwise,
/// `writes` is discarded without any changes to World State.
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
    /// initialize a new WorldStateCache, at the start of a new transaction
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
        let balance = self.balance(&address);
        self.balances.writes.remove(&address);
        balance
    }

    /// reverts changes to all read-write caches
    pub fn revert(&mut self) {
        self.balances.revert();
        self.cbi_versions.revert();
        self.contract_codes.revert();
        self.storage_data.revert();
    }

    /// retrieve the balance of native tokens for a particular account
    /// ### panics
    /// panics on unexpected errors with the account trie, which might reflect an invalid World State
    pub fn balance(&self, address: &PublicAddress) -> u64 {
        self.balances
            .get(address, |key| self.ws.account_trie().balance(key).ok())
            .expect(&format!(
                "Account trie should get balance for {:?}",
                address
            ))
    }

    /// sets account balance to the balance cache, needs to be committed separately
    pub fn set_balance(&mut self, address: PublicAddress, balance: u64) {
        self.balances.set(address, balance);
    }

    /// retrieve cbi version for a particular contract account
    /// ### panics
    /// panics on unexpected errors with the account trie, which might reflect an invalid World State
    pub fn cbi_version(&self, address: &PublicAddress) -> Option<u32> {
        self.cbi_versions.get(address, |key| {
            self.ws.account_trie().cbi_version(key).expect(&format!(
                "Account trie should get CBI version for {:?}",
                address
            ))
        })
    }

    pub fn set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        self.cbi_versions.set(address, cbi_version);
    }

    /// retrieve contract code for a particular contract account
    /// ### panics
    /// panics on unexpected errors with the account trie, which might reflect an invalid World State
    pub fn contract_code(&self, address: &PublicAddress) -> Option<Vec<u8>> {
        self.contract_codes.get(address, |key| {
            self.ws.account_trie().code(key).expect(&format!(
                "Account trie should get contract code for {:?}",
                address
            ))
        })
    }

    /// stores contract code to the contract cache, needs to be committed separately
    pub fn set_contract_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        self.contract_codes.set(address, code);
    }

    /// check for existence of a value in the storage trie for a particular address
    /// # Panics
    ///  Will panic on unexpected errors with the storage trie, which reflects an invalid World State
    pub fn contains_storage_data(&mut self, address: PublicAddress, key: &[u8]) -> bool {
        self.storage_data
            .contains(&(address, key.to_vec()), |(addr, key)| -> bool {
                self.ws
                    .storage_trie(addr)
                    .expect(&format!("Storage trie should exist for {:?}", address))
                    .contains(key)
                    .expect(&format!(
                        "Storage trie should contain data for {:?}",
                        address
                    ))
            })
    }

    /// retrieves data from account storage
    /// # Panics
    /// Will panic on unexpected errors with the storage trie, which reflects an invalid World State
    pub fn storage_data(&mut self, address: PublicAddress, key: &[u8]) -> Option<Vec<u8>> {
        self.storage_data
            .get(&(address, key.to_vec()), |(addr, k)| {
                self.ws
                    .storage_trie(addr)
                    .expect(&format!("Storage trie should exist for {:?}", address))
                    .get(k)
                    .expect(&format!("Storage trie should get data for {:?}", address))
            })
    }

    /// sets key-value to account storage cache, needs to be committed separately
    pub fn set_storage_data(&mut self, address: PublicAddress, key: &[u8], value: Vec<u8>) {
        self.storage_data.set((address, key.to_vec()), value);
    }

    /// writes the actual values to the relevant data structures in the World State.
    /// this method is typically invoked at the end of every commmand's execution to persist the changes.
    /// ### Panics
    /// panics if any of the writes fail.
    pub fn commit_to_world_state(self) -> WorldState<'a, S, V> {
        let mut ws = self.ws;
        for (address, balance) in self.balances.writes.into_iter() {
            ws.account_trie_mut()
                .set_balance(&address, balance)
                .expect(&format!(
                    "Account trie should set balance for {:?}",
                    address
                ));
        }

        for (address, version) in self.cbi_versions.writes.into_iter() {
            ws.account_trie_mut()
                .set_cbi_version(&address, version)
                .expect(&format!(
                    "Account trie should set CBI version for {:?}",
                    address
                ));
        }

        for (address, code) in self.contract_codes.writes.into_iter() {
            ws.account_trie_mut()
                .set_code(&address, code)
                .expect(&format!(
                    "Account trie should set contract code for {:?}",
                    address
                ));
        }

        // optimisation: aggregate Storage writes in memory by address, to use StorageTrie .batch_set()
        // as calling .set() individually will be slower
        let mut aggregated_storage_writes = HashMap::with_capacity(self.storage_data.writes.len());
        for ((address, key), value) in self.storage_data.writes.into_iter() {
            aggregated_storage_writes
                .entry(address)
                .or_insert_with(HashMap::new)
                .insert(key, value);
        }
        for (address, kv_data) in aggregated_storage_writes {
            let trie = ws
                .storage_trie_mut(&address)
                .expect(&format!("Storage trie should exist for {:?}", address));
            trie.batch_set(&kv_data)
                .expect(&format!("Storage trie should set data for {:?}", address));
        }

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

/// Generic hashmap based cache for storing key-value pairs.
#[derive(Clone, Default)]
pub(crate) struct CacheData<K, V> {
    /// writes caches key-value pairs for Write operations before committing to World State.
    pub writes: HashMap<K, V>,
    /// reads caches key-value pairs from Read operations.
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

    /// reverts changes to both read and write caches
    pub fn revert(&mut self) {
        self.reads.borrow_mut().clear();
        self.writes.clear();
    }
}
