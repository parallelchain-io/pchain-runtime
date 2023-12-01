use core::panic;
use std::mem::MaybeUninit;

use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::{VersionProvider, DB};
use wasmer::Global;

use crate::{
    contract::{wasmer::memory::MemoryContext, ContractModule, SmartContractContext},
    execution::cache::{CommandOutputCache, WorldStateCache},
    types::TxnVersion,
};

use super::{
    operation::{self, OperationReceipt},
    GasMeter,
};

/// Keeps track of total gas used when executing contract calls.
/// References the Wasmer global var tracking gas usage from Wasm opcode execution
/// and exposes method for deducting gas usage by host functions.
pub(crate) struct WasmerGasGlobal {
    /// global vaiable of wasmer_middlewares::metering remaining points.
    wasmer_gas: MaybeUninit<Global>,
    is_initialized: bool,
}

impl WasmerGasGlobal {
    pub fn new() -> Self {
        Self {
            wasmer_gas: MaybeUninit::uninit(),
            is_initialized: false,
        }
    }

    pub fn write(&mut self, global: Global) {
        self.wasmer_gas.write(global);
        self.is_initialized = true;
    }

    pub fn clear(&mut self) {
        unsafe {
            self.check_init();
            self.wasmer_gas.assume_init_drop();
        }
        self.is_initialized = false;
    }

    pub fn gas(&self) -> u64 {
        unsafe {
            self.check_init();
            self.wasmer_gas.assume_init_ref().get().try_into().unwrap()
        }
    }

    /// subtract amount from wasmer_gas
    pub fn subtract(&self, amount: u64) -> u64 {
        let current_remaining_points: u64 = self.gas();
        let new_remaining_points = current_remaining_points.saturating_sub(amount);
        unsafe {
            self.check_init();
            self.wasmer_gas
                .assume_init_ref()
                .set(new_remaining_points.into())
                .unwrap();
            new_remaining_points
        }
    }

    fn check_init(&self) {
        if !self.is_initialized {
            panic!("Can't access `wasmer_metering_remaining_points` as not initialized");
        }
    }
}

/// Implements a facade for all chargeable Wasm host functions,
/// and deducts the cost of each operation from WasmerGasGlobal.
pub(crate) struct HostFuncGasMeter<'a, 'b, S, M, V>
where
    S: DB + Send + Sync + Clone + 'static,
    M: MemoryContext,
    V: VersionProvider + Send + Sync + Clone,
{
    /// version of the transaction
    version: TxnVersion,
    /// reference to the outer Env struct, for memory operations
    memory_ctx: &'b M,
    /// mutable reference to the WasmerGasGlobal which lives for entire duration of Env (contract call)
    /// though not strictly needed, its mutability reflects the fact that it is modified by host functions
    wasmer_gas_global: &'b mut WasmerGasGlobal,
    /// mutable reference to CommandOutputCache from the global gas meter
    command_output_cache: &'b mut CommandOutputCache,
    /// mutable reference to WorldStateCache from the global gas meter
    ws_cache: &'b mut WorldStateCache<'a, S, V>,
}

impl<'a, 'b, S, M, V> HostFuncGasMeter<'a, 'b, S, M, V>
where
    S: DB + Send + Sync + Clone + 'static,
    M: MemoryContext,
    V: VersionProvider + Send + Sync + Clone,
{
    pub fn new(
        gas_meter: &'b mut GasMeter<'a, S, V>,
        wasmer_remaining_gas: &'b mut WasmerGasGlobal,
        memory_ctx: &'b M,
    ) -> Self {
        Self {
            version: gas_meter.version,
            memory_ctx,
            wasmer_gas_global: wasmer_remaining_gas,
            ws_cache: &mut gas_meter.ws_cache,
            command_output_cache: &mut gas_meter.output_cache_of_current_command,
        }
    }

    /// returns the remaining gas from WasmerRemainingGas global
    pub fn remaining_gas(&self) -> u64 {
        self.wasmer_gas_global.gas()
    }

    /// method for manual gas deduction from WasmerRemainingGas
    pub fn deduct_gas(&mut self, amount: u64) -> u64 {
        self.wasmer_gas_global.subtract(amount)
    }

    pub fn command_output_cache(&mut self) -> &mut CommandOutputCache {
        self.command_output_cache
    }

    pub fn ws_get_storage_data(&mut self, address: PublicAddress, key: &[u8]) -> Option<Vec<u8>> {
        let result = operation::ws_get_storage_data(self.version, self.ws_cache, address, key);
        self.charge(result).filter(|v| !v.is_empty())
    }

    /// Get the balance from read-write set. It balance is not found, gets from WS and caches it.
    pub fn ws_get_balance(&self, address: PublicAddress) -> u64 {
        let result = operation::ws_get_balance(self.ws_cache, &address);
        self.charge(result)
    }

    pub fn ws_set_storage_data(&mut self, address: PublicAddress, key: &[u8], value: Vec<u8>) {
        let result =
            operation::ws_set_storage_data(self.version, self.ws_cache, address, key, value);
        self.charge(result);
    }

    /// Sets balance in the WSCache. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        let result = operation::ws_set_balance(self.ws_cache, address, value);
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

    /// write data to linear memory, charge the write cost and return the length
    pub fn write_bytes(&self, value: Vec<u8>, val_ptr_ptr: u32) -> Result<u32, anyhow::Error> {
        let result = operation::write_bytes(self.memory_ctx, value, val_ptr_ptr);
        self.charge(result)
    }

    /// read data from linear memory and charge the read cost
    pub fn read_bytes(&self, offset: u32, len: u32) -> Result<Vec<u8>, anyhow::Error> {
        let result = operation::read_bytes(self.memory_ctx, offset, len);
        self.charge(result)
    }

    pub fn command_output_append_log(&mut self, log: Log) {
        let result =
            operation::command_output_append_log(self.command_output_cache.logs.as_mut(), log);
        self.charge(result)
    }

    pub fn command_output_set_return_value(&mut self, return_value: Vec<u8>) {
        let result = operation::command_output_set_return_value(
            self.command_output_cache.return_value.as_mut(),
            return_value,
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
        signature: [u8; 64],
        pub_key: [u8; 32],
    ) -> Result<i32, anyhow::Error> {
        let result = operation::verify_ed25519_signature(message, signature, pub_key);
        self.charge(result)
    }

    fn charge<T>(&self, op_receipt: OperationReceipt<T>) -> T {
        self.wasmer_gas_global.subtract(op_receipt.1.net_cost().0);
        op_receipt.0
    }
}
