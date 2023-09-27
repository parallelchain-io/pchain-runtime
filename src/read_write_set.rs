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

use crate::gas;

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
        }
    }

    /// remove cached writes and return the value,
    /// gas free operation, only used for accouting during charge phase
    pub fn purge_balance(&mut self, address: PublicAddress) -> u64 {
        let balance = match self.get_uncharged(&CacheKey::Balance(address)) {
            Some(CacheValue::Balance(value)) => value,
            _ => panic!(),
        };
        let key = CacheKey::Balance(address);
        self.writes.remove(&key);
        balance
    }

    /// reverts changes to read-write set
    pub fn revert(&mut self) {
        self.reads.borrow_mut().clear();
        self.writes.clear();
    }

    /// set value to contract storage. This operation does not write to world state immediately.
    /// It is gas-free operation.
    pub fn set_app_data_uncharged(
        &mut self,
        address: PublicAddress,
        app_key: AppKey,
        value: Vec<u8>,
    ) {
        self.set_uncharged(CacheKey::App(address, app_key), CacheValue::App(value));
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

    // Low Level Operations
    /// Check if readwrite set contains this key.
    /// Not charged here as all charging is performed by RuntimeGasMeter.
    pub fn contains_uncharged(&self, key: &CacheKey) -> bool {
        self.writes.get(key).filter(|v| v.len() != 0).is_some()
            || self
                .reads
                .borrow()
                .get(key)
                .filter(|v| v.is_some())
                .is_some()
    }

    /// Check if storage contains this key.
    /// Not charged here as all charging is performed by RuntimeGasMeter.
    pub fn contains_in_storage_uncharged(&self, address: PublicAddress, app_key: &AppKey) -> bool {
        self.ws.contains().storage_value(&address, app_key)
    }

    /// Get latest value from readwrite set. If not found, get from world state and then cache it.
    /// Not charged here as all charging is performed by RuntimeGasMeter.
    pub fn get_uncharged(&self, key: &CacheKey) -> Option<CacheValue> {
        // 1. Return the value that was written earlier in the transaction ('read-your-write' semantics)
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

    /// Insert to write set.
    /// Not charged here as all charging is performed by RuntimeGasMeter.
    pub fn set_uncharged(&mut self, key: CacheKey, value: CacheValue) {
        self.writes.insert(key, value);
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

// TODO CLEAN ideally move this elsewhere
// has more to do with Gas calculation than actual RWSet functions
//
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
