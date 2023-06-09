use std::collections::HashMap;

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{keys::AppKey, states::WorldState, storage::WorldStateStorage};

#[derive(Clone)]
pub struct SimulateWorldState {
    pub world_state: WorldState<SimulateWorldStateStorage>,
}

pub type SimulateKey = Vec<u8>;

#[derive(Clone, Default)]
pub struct SimulateWorldStateStorage {
    inner: HashMap<SimulateKey, Vec<u8>>,
}

impl WorldStateStorage for SimulateWorldStateStorage {
    fn get(
        &self,
        key: &pchain_world_state::storage::Key,
    ) -> Option<pchain_world_state::storage::Value> {
        match self.inner.get(key) {
            Some(value) => Some(value.clone()),
            None => None,
        }
    }
}

impl SimulateWorldState {
    pub fn default() -> Self {
        Self {
            world_state: WorldState::initialize(SimulateWorldStateStorage::default()),
        }
    }

    pub fn get_storage_data(&self, address: PublicAddress, key: Vec<u8>) -> Option<Vec<u8>> {
        self.world_state.storage_value(&address, &AppKey::new(key))
    }

    pub fn set_storage_data(&mut self, address: PublicAddress, key: Vec<u8>, value: Vec<u8>) {
        self.world_state
            .with_commit()
            .set_storage_value(address, AppKey::new(key), value);
    }

    pub fn get_balance(&self, address: PublicAddress) -> u64 {
        self.world_state.balance(address)
    }

    pub fn set_balance(&mut self, address: PublicAddress, balance: u64) {
        self.world_state.with_commit().set_balance(address, balance);
    }

    pub fn get_nonce(&self, address: PublicAddress) -> u64 {
        self.world_state.nonce(address)
    }

    pub fn add_contract(
        &mut self,
        to_address: PublicAddress,
        wasm_bytes: Vec<u8>,
        cbi_version: u32,
    ) {
        self.world_state
            .with_commit()
            .set_code(to_address, wasm_bytes);
        self.world_state
            .with_commit()
            .set_cbi_version(to_address, cbi_version);
    }

    pub fn get_contract_code(&self, address: PublicAddress) -> Option<Vec<u8>> {
        self.world_state.code(address)
    }
}

impl From<WorldState<SimulateWorldStateStorage>> for SimulateWorldState {
    fn from(world_state: WorldState<SimulateWorldStateStorage>) -> Self {
        Self { world_state }
    }
}
