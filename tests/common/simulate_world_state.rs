use std::collections::HashMap;

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{VersionProvider, WorldState, DB};

#[derive(Clone)]
pub struct SimulateWorldState<'a, V: VersionProvider + Send + Sync + Clone> {
    pub world_state: WorldState<'a, SimulateWorldStateStorage, V>,
}

pub type SimulateKey = Vec<u8>;

#[derive(Clone, Default)]
pub struct SimulateWorldStateStorage {
    inner: HashMap<SimulateKey, Vec<u8>>,
}

impl DB for SimulateWorldStateStorage {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.inner.get(key) {
            Some(value) => Some(value.clone()),
            None => None,
        }
    }
}

impl<'a, V> SimulateWorldState<'a, V>
where
    V: VersionProvider + Send + Sync + Clone,
{
    pub fn new(storage: &'a SimulateWorldStateStorage) -> Self {
        Self {
            world_state: WorldState::<SimulateWorldStateStorage, V>::new(storage),
        }
    }

    pub fn get_storage_data(&mut self, address: PublicAddress, key: Vec<u8>) -> Option<Vec<u8>> {
        self.world_state
            .storage_trie(&address)
            .unwrap()
            .get(&key)
            .unwrap()
    }

    pub fn set_storage_data(&mut self, address: PublicAddress, key: Vec<u8>, value: Vec<u8>) {
        self.world_state
            .storage_trie_mut(&address)
            .unwrap()
            .set(&key, value)
            .unwrap()
    }

    pub fn get_balance(&self, address: PublicAddress) -> u64 {
        self.world_state.account_trie().balance(&address).unwrap()
    }

    pub fn set_balance(&mut self, address: PublicAddress, balance: u64) {
        self.world_state
            .account_trie_mut()
            .set_balance(&address, balance)
            .unwrap()
    }

    pub fn get_nonce(&self, address: PublicAddress) -> u64 {
        self.world_state.account_trie().nonce(&address).unwrap()
    }

    pub fn add_contract(
        &mut self,
        to_address: PublicAddress,
        wasm_bytes: Vec<u8>,
        cbi_version: u32,
    ) {
        self.world_state
            .account_trie_mut()
            .set_code(&to_address, wasm_bytes)
            .unwrap();

        self.world_state
            .account_trie_mut()
            .set_cbi_version(&to_address, cbi_version)
            .unwrap();
    }

    pub fn get_contract_code(&self, address: PublicAddress) -> Option<Vec<u8>> {
        self.world_state.account_trie().code(&address).unwrap()
    }
}

impl<'a, V> From<WorldState<'a, SimulateWorldStateStorage, V>> for SimulateWorldState<'a, V>
where
    V: VersionProvider + Send + Sync + Clone,
{
    fn from(world_state: WorldState<'a, SimulateWorldStateStorage, V>) -> Self {
        Self { world_state }
    }
}
