use std::mem::MaybeUninit;

use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::{keys::AppKey, storage::WorldStateStorage};
use wasmer::Global;

use crate::{
    contract::{wasmer::memory::MemoryContext, ContractModule, SmartContractContext},
    execution::cache::{CacheKey, CacheValue, CommandOutputCache, WorldStateCache},
};

use super::{
    operation::{self, OperationReceipt},
    GasMeter,
};

/// Tracks the webassemby global instance which represents the remaining gas
/// during wasmer execution.
pub(crate) struct WasmerRemainingGas {
    /// global vaiable of wasmer_middlewares::metering remaining points.
    wasmer_gas: MaybeUninit<Global>,
}

impl WasmerRemainingGas {
    pub fn new() -> Self {
        Self {
            wasmer_gas: MaybeUninit::uninit(),
        }
    }

    pub fn write(&mut self, global: Global) {
        self.wasmer_gas.write(global);
    }

    pub fn clear(&mut self) {
        unsafe {
            self.wasmer_gas.assume_init_drop();
        }
    }

    pub fn gas(&self) -> u64 {
        unsafe { self.wasmer_gas.assume_init_ref().get().try_into().unwrap() }
    }

    /// substract amount from wasmer_gas
    pub fn substract(&self, amount: u64) -> u64 {
        unsafe {
            let current_remaining_points: u64 = self.gas();
            let new_remaining_points = current_remaining_points.saturating_sub(amount);
            self.wasmer_gas
                .assume_init_ref()
                .set(new_remaining_points.into())
                .expect("Can't subtract `wasmer_metering_remaining_points` in Env");
            new_remaining_points
        }
    }
}

pub(crate) struct WasmerGasMeter<'a, S, M>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
    M: MemoryContext,
{
    memory_ctx: &'a M,
    wasmer_remaining_gas: &'a WasmerRemainingGas,
    command_output_cache: &'a mut CommandOutputCache,
    ws_cache: &'a mut WorldStateCache<S>,
}

impl<'a, S, M> WasmerGasMeter<'a, S, M>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
    M: MemoryContext,
{
    pub fn new(
        memory_ctx: &'a M,
        wasmer_remaining_gas: &'a WasmerRemainingGas,
        gas_meter: &'a mut GasMeter<S>,
    ) -> Self {
        Self {
            memory_ctx,
            wasmer_remaining_gas,
            ws_cache: &mut gas_meter.ws_cache,
            command_output_cache: &mut gas_meter.output_cache_of_current_command,
        }
    }

    pub fn remaining_gas(&self) -> u64 {
        self.wasmer_remaining_gas.gas()
    }

    pub fn reduce_gas(&self, amount: u64) -> u64 {
        self.wasmer_remaining_gas.substract(amount)
    }

    pub fn command_output_cache(&mut self) -> &mut CommandOutputCache {
        self.command_output_cache
    }

    pub fn ws_get_app_data(&self, address: PublicAddress, key: AppKey) -> Option<Vec<u8>> {
        let result = operation::ws_get(self.ws_cache, CacheKey::App(address, key));
        let value = self.charge(result);
        match value {
            Some(CacheValue::App(value)) => (!value.is_empty()).then_some(value),
            None => None,
            _ => panic!("Retrieved data not of App variant"),
        }
    }

    /// Get the balance from read-write set. It balance is not found, gets from WS and caches it.
    pub fn ws_get_balance(&self, address: PublicAddress) -> u64 {
        let result = operation::ws_get(self.ws_cache, CacheKey::Balance(address));
        let value = self.charge(result);

        match value {
            Some(CacheValue::Balance(value)) => value,
            _ => panic!(),
        }
    }

    pub fn ws_set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        let result = operation::ws_set(
            self.ws_cache,
            CacheKey::App(address, app_key),
            CacheValue::App(value),
        );
        self.charge(result);
    }

    /// Sets balance in the write set. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        let result = operation::ws_set(
            self.ws_cache,
            CacheKey::Balance(address),
            CacheValue::Balance(value),
        );
        self.charge(result);
    }

    pub fn ws_get_cached_contract(
        &self,
        address: PublicAddress,
        sc_context: &SmartContractContext,
    ) -> Option<ContractModule> {
        let result = operation::ws_get_cached_contract(self.ws_cache, sc_context, address);
        self.charge(result)
    }

    /// write the data to memory, charge the write cost and return the length
    pub fn write_bytes(&self, value: Vec<u8>, val_ptr_ptr: u32) -> Result<u32, anyhow::Error> {
        let result = operation::write_bytes(self.memory_ctx, value, val_ptr_ptr);
        self.charge(result)
    }

    /// read data from memory and charge the read cost
    pub fn read_bytes(&self, offset: u32, len: u32) -> Result<Vec<u8>, anyhow::Error> {
        let result = operation::read_bytes(self.memory_ctx, offset, len);
        self.charge(result)
    }

    pub fn command_output_append_log(&mut self, log: Log) {
        let result = operation::command_output_append_log(&mut self.command_output_cache.logs, log);
        self.charge(result)
    }

    pub fn command_output_set_return_values(&mut self, return_values: Vec<u8>) {
        let result = operation::command_output_set_return_values(
            &mut self.command_output_cache.return_values,
            return_values,
        );
        self.charge(result)
    }

    //
    //
    // Facade methods for cryptographic operations on host machine callable by contracts
    //
    //

    pub fn sha256(&self, input_bytes: Vec<u8>) -> Vec<u8> {
        let result = operation::sha256(input_bytes);
        self.charge(result)
    }

    pub fn keccak256(&self, input_bytes: Vec<u8>) -> Vec<u8> {
        let result = operation::keccak256(input_bytes);
        self.charge(result)
    }

    pub fn ripemd(&self, input_bytes: Vec<u8>) -> Vec<u8> {
        let result = operation::ripemd(input_bytes);
        self.charge(result)
    }

    pub fn verify_ed25519_signature(
        &self,
        message: Vec<u8>,
        signature: Vec<u8>,
        pub_key: Vec<u8>,
    ) -> Result<i32, anyhow::Error> {
        let result = operation::verify_ed25519_signature(message, signature, pub_key);
        self.charge(result)
    }

    fn charge<T>(&self, op_receipt: OperationReceipt<T>) -> T {
        self.wasmer_remaining_gas.substract(op_receipt.1.values().0);
        op_receipt.0
    }
}