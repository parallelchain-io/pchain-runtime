use core::panic;
use std::{cell::RefCell, mem::MaybeUninit};

use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::{
    keys::AppKey,
    network::{constants::NETWORK_ADDRESS, network_account::NetworkAccountStorage},
    storage::WorldStateStorage
};

use wasmer::Global;

use crate::{
    contract::{self, SmartContractContext},
    cost::CostChange,
    gas,
    world_state_cache::{CacheKey, CacheValue, WorldStateCache},
    TransitionError, wasmer::wasmer_memory::MemoryContext,
};

use self::operation::OperationReceipt;

#[derive(Clone, Default)]
pub(crate) struct GasUsed {
    cost_change: RefCell<CostChange>,
}

impl GasUsed {

    pub fn chargeable_cost(&self) -> u64 {
        self.cost_change.borrow().values().0
    }

    pub fn charge(&self, cost_change: CostChange) {
        *self.cost_change.borrow_mut() += cost_change;
    }

    pub fn reset(&mut self) {
        *self.cost_change.borrow_mut() = CostChange::default();
    }
}

/// GasMeter is a global struct that keeps track of gas used from operations OUTSIDE of a Wasmer guest instance (compute and memory access).
/// It implements a facade for all chargeable methods.
#[derive(Clone)]
pub(crate) struct GasMeter<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// gas limit of the entire txn
    pub gas_limit: u64,

    /// stores txn inclusion gas separately because it is not considered to belong to a single command
    txn_inclusion_gas_used: u64,

    /// cumulative gas used for all executed commands
    total_command_gas_used: u64,

    /// stores the gas used by current command,
    /// finalized and reset at the end of each command
    current_command_gas_used: GasUsed,

    pub current_command_output_cache: CommandOutputCache,

    pub ws_cache: WorldStateCache<S>,
}

impl<'a, S> GasMeter<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub fn new(ws_cache: WorldStateCache<S>, gas_limit: u64) -> Self {
        Self {
            ws_cache,
            gas_limit,
            total_command_gas_used: 0,
            txn_inclusion_gas_used: 0,
            current_command_gas_used: GasUsed::default(),
            current_command_output_cache: CommandOutputCache::default()
        }
    }

    /// called after every command to reset command_gas_used
    pub fn take_command_receipt(&mut self) -> (u64, Vec<Log>, Vec<u8>) {
        let (logs, return_values) = self.current_command_output_cache.take();

        // sum to total_command_gas_used
        let gas_used_by_command = self.get_gas_used_for_current_command();
        self.total_command_gas_used = self
            .total_command_gas_used
            .saturating_add(gas_used_by_command);
        // reset command_gas_used
        self.current_command_gas_used.reset();

        (gas_used_by_command, logs, return_values)
    }

    //
    //
    // Gas Accounting
    //
    //

    /// method to bring in gas consumed in the Wasmer env due to
    /// 1) read and write to Wasmer memory,
    /// 2) compute cost
    pub fn reduce_gas(&mut self, gas: u64) {
        self.current_command_gas_used.charge(CostChange::deduct(gas));
    }

    fn charge<T>(&self, op_receipt: OperationReceipt<T>) -> T {
        self.current_command_gas_used.charge(op_receipt.1);
        op_receipt.0
    }

    /// returns gas that has been used so far
    /// will not exceed maximum
    pub fn get_gas_already_used(&self) -> u64 {
        let val = self
            .txn_inclusion_gas_used
            .saturating_add(self.total_command_gas_used);

        // TODO CLEAN probably can remove this sanity check, should not happen as we only consume gas up to the limit
        if self.gas_limit < val {
            panic!("Invariant violated, we are using more gas than the limit");
        } else {
            val
        }
    }

    /// returns the theoretical max gas used so far
    /// may exceed gas_limit
    pub fn get_gas_to_be_used_in_theory(&self) -> u64 {
        self.get_gas_already_used()
            .saturating_add(self.current_command_gas_used.chargeable_cost())
    }

    fn get_gas_used_for_current_command(&self) -> u64 {
        if self.gas_limit < self.get_gas_to_be_used_in_theory() {
            // consume only up to limit if exceeding
            return self.gas_limit.saturating_sub(self.get_gas_already_used());
        }
        self.current_command_gas_used.chargeable_cost()
    }


    //
    //
    // Facade methods for Transaction Storage methods that cost gas
    //
    //

    pub fn charge_txn_pre_exec_inclusion(
        &mut self,
        tx_size: usize,
        commands_len: usize,
    ) -> Result<(), TransitionError> {
        // stored separately because it is not considered to belong to a single command
        let required_cost = gas::tx_inclusion_cost(tx_size, commands_len);
        if required_cost > self.gas_limit {
            return Err(TransitionError::PreExecutionGasExhausted);
        } else {
            self.txn_inclusion_gas_used = required_cost;
        }
        Ok(())
    }

    pub fn command_output_set_return_values(&mut self, return_values: Vec<u8>) {
        let result = operation::command_output_set_return_values(
            &mut self.current_command_output_cache.return_values,
            return_values
        );
        self.charge(result)
    }

    //
    //
    // Facade methods for World State methods that cost gas
    //
    //
    //

    //
    // CONTAINS methods
    //
    /// Check if App key has non-empty data
    pub fn ws_contains_app_data(&self, address: PublicAddress, app_key: AppKey) -> bool {
        let result = operation::ws_contains(&self.ws_cache, &CacheKey::App(address, app_key.clone()));
        self.charge(result)
    }

    //
    // GET methods
    //
    /// Gets app data from the read-write set.
    pub fn ws_get_app_data(&self, address: PublicAddress, key: AppKey) -> Option<Vec<u8>> {
        let result = operation::ws_get(&self.ws_cache, CacheKey::App(address, key));
        let value = self.charge(result)?;
        
        match value {
            CacheValue::App(value) => (!value.is_empty()).then_some(value),
            _ => panic!("Retrieved data not of App variant"),
        }
    }

    /// Get the balance from read-write set. It balance is not found, gets from WS and caches it.
    pub fn ws_get_balance(&self, address: PublicAddress) -> u64 {
        let result = operation::ws_get(&self.ws_cache, CacheKey::Balance(address));
        let value = self.charge(result).expect("Balance must be some!");

        match value {
            CacheValue::Balance(value) => value,
            _ => panic!("Retrieved data not of Balance variant"),
        }
    }

    pub fn ws_get_cbi_version(&self, address: PublicAddress) -> Option<u32> {
        let result = operation::ws_get(&self.ws_cache, CacheKey::CBIVersion(address));
        let value = self.charge(result)?;
        match value {
            CacheValue::CBIVersion(value) => Some(value),
            _ => panic!("Retrieved data not of CBIVersion variant"),
        }
    }

    pub fn ws_get_cached_contract(
        &self,
        address: PublicAddress,
        sc_context: &SmartContractContext,
    ) -> Option<(contract::Module, wasmer::Store)> {
        self.charge(operation::ws_get_cached_contract(&self.ws_cache, sc_context, address))
    }

    //
    // SET methods
    //
    pub fn ws_set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        let result = operation::ws_set(&mut self.ws_cache,
            CacheKey::App(address, app_key),
            CacheValue::App(value)
        );
        self.charge(result)
    }

    /// Sets balance in the write set. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        let result = operation::ws_set(&mut self.ws_cache,
            CacheKey::Balance(address),
            CacheValue::Balance(value)
        );
        self.charge(result)
    }

    /// Sets CBI version in the write set. It does not write to WS immediately.
    pub fn ws_set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        let result = operation::ws_set(&mut self.ws_cache,
            CacheKey::CBIVersion(address),
            CacheValue::CBIVersion(cbi_version),
        );
        self.charge(result)
    }

    /// Sets contract bytecode in the write set. It does not write to WS immediately.
    pub fn ws_set_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        let result = operation::ws_set(&mut self.ws_cache,
            CacheKey::ContractCode(address),
            CacheValue::ContractCode(code)
        );
        self.charge(result)
    }
}

/// GasMeter implements NetworkAccountStorage with charegable read-write operations to world state
impl<'a, S> NetworkAccountStorage for GasMeter<S>
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

#[derive(Clone, Default)]
pub(crate) struct CommandOutputCache {
    /// stores the list of events from exeuting a command, ordered by the sequence of emission
    logs: Vec<Log>,

    /// value returned by a call transaction using the `return_value` SDK function.
    /// It is None if the execution has not/did not return anything.
    return_values: Option<Vec<u8>>,
}

impl CommandOutputCache {
    pub fn take(&mut self) -> (Vec<Log>, Vec<u8>) {
        let logs = self.take_logs();
        let return_values = self.take_return_values().map_or(Vec::new(), std::convert::identity);
        (logs, return_values)
    }

    pub fn take_logs(&mut self) -> Vec<Log> {
        std::mem::take(&mut self.logs)
    }

    pub fn take_return_values(&mut self) -> Option<Vec<u8>> {
        self.return_values.take()
    }
}


/// Tracks the webassemby global instance which represents the remaining gas
/// during wasmer execution.
pub(crate) struct WasmerRemainingGas {
    /// global vaiable of wasmer_middlewares::metering remaining points.
    wasmer_gas: MaybeUninit<Global>,
}

impl WasmerRemainingGas {

    pub fn new() -> Self {
        Self {
            wasmer_gas: MaybeUninit::uninit()
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
        unsafe {
            self.wasmer_gas.assume_init_ref().get().try_into().unwrap()
        }
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
    M: MemoryContext
{
    memory_ctx: &'a M,
    wasmer_remaining_gas: &'a WasmerRemainingGas,
    command_output_cache: &'a mut CommandOutputCache,
    ws_cache: &'a mut WorldStateCache<S>,
}

impl<'a, S, M> WasmerGasMeter<'a, S, M>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
    M: MemoryContext
{
    pub fn new(memory_ctx: &'a M, wasmer_remaining_gas: &'a WasmerRemainingGas, gas_meter: &'a mut GasMeter<S>) -> Self {
        Self { memory_ctx, wasmer_remaining_gas, ws_cache: &mut gas_meter.ws_cache, command_output_cache: &mut gas_meter.current_command_output_cache }
    }

    pub fn remaining_gas(&self) -> u64 {
        self.wasmer_remaining_gas.gas()
    }

    pub fn reduce_gas(&self, amount: u64) -> u64 {
        self.wasmer_remaining_gas.substract(amount)
    }

    pub fn command_output_cache(&mut self) -> &mut CommandOutputCache {
        &mut self.command_output_cache
    }

    pub fn ws_get_app_data(&self, address: PublicAddress, key: AppKey) -> Option<Vec<u8>> {
        let result = operation::ws_get(&self.ws_cache, CacheKey::App(address, key));
        let value = self.charge(result);
        match value {
            Some(CacheValue::App(value)) => {
                (!value.is_empty()).then_some(value)
            }
            None => None,
            _ => panic!("Retrieved data not of App variant"),
        }
    }

    /// Get the balance from read-write set. It balance is not found, gets from WS and caches it.
    pub fn ws_get_balance(&self, address: PublicAddress) -> u64 {
        let result = operation::ws_get(&self.ws_cache, CacheKey::Balance(address));
        let value = self.charge(result);

        match value {
            Some(CacheValue::Balance(value)) => value,
            _ => panic!(),
        }
    }

    pub fn ws_set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        let result = operation::ws_set(&mut self.ws_cache,
            CacheKey::App(address, app_key),
            CacheValue::App(value)
        );
        self.charge(result);
    }

    /// Sets balance in the write set. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        let result = operation::ws_set(&mut self.ws_cache,
            CacheKey::Balance(address),
            CacheValue::Balance(value)
        );
        self.charge(result);
    }

    pub fn ws_get_cached_contract(
        &self,
        address: PublicAddress,
        sc_context: &SmartContractContext,
    ) -> Option<(contract::Module, wasmer::Store)> {
        let result = operation::ws_get_cached_contract(&self.ws_cache, sc_context, address);
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
        let result = operation::command_output_append_log(
            &mut self.command_output_cache.logs,
            log
        );
        self.charge(result)
    }

    pub fn command_output_set_return_values(&mut self, return_values: Vec<u8>) {
        let result = operation::command_output_set_return_values(
            &mut self.command_output_cache.return_values,
            return_values
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

    pub fn verify_ed25519_signature(&self, message: Vec<u8>, signature: Vec<u8>, pub_key: Vec<u8>) -> Result<i32, anyhow::Error> {
        let result = operation::verify_ed25519_signature(message, signature, pub_key);
        self.charge(result)
    }
    
    fn charge<T>(&self, op_receipt: OperationReceipt<T>) -> T {
        self.wasmer_remaining_gas.substract(op_receipt.1.values().0);
        op_receipt.0
    }
}

mod operation   
{
    use pchain_types::{cryptography::PublicAddress, blockchain::Log};
    use pchain_world_state::storage::WorldStateStorage;
    use ed25519_dalek::Verifier;
    use ripemd::Ripemd160;
    use sha2::{Digest as sha256_digest, Sha256};
    use tiny_keccak::{Hasher as _, Keccak};

    use crate::{world_state_cache::{WorldStateCache, CacheKey, CacheValue}, cost::CostChange, gas, contract::{SmartContractContext, self}, wasmer::wasmer_memory::MemoryContext};

    pub(crate) type OperationReceipt<T> = (T, CostChange);

    pub(crate) fn ws_set<S>(ws_cache: &mut WorldStateCache<S>, key: CacheKey, value: CacheValue) -> OperationReceipt<()>
        where S: WorldStateStorage + Send + Sync + Clone + 'static 
    {
        let key_len = key.len();

        let new_val_len = value.len();
        let (old_val_len, get_cost) = ws_get(&ws_cache, key.clone());
        let old_val_len = old_val_len.map_or(0, |v| v.len());

        // old_val_len is obtained from Get so the cost of reading the key is already charged
        let set_cost = CostChange::reward(gas::set_cost_delete_old_value(
            key_len,
            old_val_len,
            new_val_len,
        ))
        + CostChange::deduct(gas::set_cost_write_new_value(new_val_len))
        + CostChange::deduct(gas::set_cost_rehash(key_len));

        ws_cache.set(key, value);

        ((), get_cost + set_cost)
    }

    pub(crate) fn ws_get<S>(ws_cache: &WorldStateCache<S>, key: CacheKey) -> OperationReceipt<Option<CacheValue>>
        where S: WorldStateStorage + Send + Sync + Clone + 'static
    {
        let value = ws_cache.get(&key);

        let get_cost = match key {
            CacheKey::ContractCode(_) => {
                CostChange::deduct(gas::get_code_cost(value.as_ref().map_or(0, |v| v.len())))
            }
            _ => CostChange::deduct(gas::get_cost(
                key.len(),
                value.as_ref().map_or(0, |v| v.len()),
            )),
        };
        (value, get_cost)
    }

    pub(crate) fn ws_contains<S>(ws_cache: &WorldStateCache<S>, key: &CacheKey) -> OperationReceipt<bool>
        where S: WorldStateStorage + Send + Sync + Clone + 'static 
    {
        let cost_change = CostChange::deduct(gas::contains_cost(key.len()));
        let ret = ws_cache.contains(key);
        (ret, cost_change)
    }

    pub(crate) fn ws_get_cached_contract<S>(
        ws_cache: &WorldStateCache<S>,
        sc_context: &SmartContractContext,
        address: PublicAddress,
    ) -> OperationReceipt<Option<(contract::Module, wasmer::Store)>>
        where S: WorldStateStorage + Send + Sync + Clone + 'static 
    {
        let wasmer_store = sc_context.instantiate_store();
        if let Some(module) = sc_context.cache.as_ref().map_or(None, |cache|
            contract::Module::from_cache(address, cache, &wasmer_store)
        ) {
            let contract_get_cost = CostChange::deduct(gas::get_code_cost(module.bytes_length()));
            return (Some((module, wasmer_store)), contract_get_cost);
        }

        // else check ws and charge
        let (value, contract_get_cost) = ws_get(ws_cache, CacheKey::ContractCode(address));
        let contract_code = match value {
            Some(CacheValue::ContractCode(value)) => value,
            None => return (None, contract_get_cost),
            _ => panic!("Retrieved data not of ContractCode variant"),
        };
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
            Err(_) => return (None, contract_get_cost),
        };
        (Some((module, wasmer_store)), contract_get_cost)
    }

    /// write the data to memory, charge the write cost and return the length
    pub(crate) fn write_bytes<M: MemoryContext>(memory_ctx: &M, value: Vec<u8>, val_ptr_ptr: u32) -> OperationReceipt<Result<u32, anyhow::Error>> {
        let write_cost: u64 = gas::wasm_memory_write_cost(value.len());
        let ret = MemoryContext::write_bytes_to_memory(memory_ctx, value, val_ptr_ptr);
        (ret, CostChange::deduct(write_cost))
    }

    /// read data from memory and charge the read cost
    pub(crate) fn read_bytes<M: MemoryContext>(memory_ctx: &M, offset: u32, len: u32) -> OperationReceipt<Result<Vec<u8>, anyhow::Error>> {
        let read_cost = gas::wasm_memory_read_cost(len as usize);
        let ret = MemoryContext::read_bytes_from_memory(memory_ctx, offset, len);
        (ret, CostChange::deduct(read_cost))
    }

    pub(crate) fn command_output_append_log(logs: &mut Vec<Log>, log: Log) -> OperationReceipt<()> {
        let cost = CostChange::deduct(gas::blockchain_log_cost(
            log.topic.len(),
            log.value.len(),
        ));
        logs.push(log);
        ((), cost)
    }

    pub(crate) fn command_output_set_return_values(command_output_return_values: &mut Option<Vec<u8>>, return_values: Vec<u8>) -> OperationReceipt<()> {
        let cost = CostChange::deduct(gas::blockchain_return_values_cost(return_values.len()));
        *command_output_return_values = Some(return_values);
        ((), cost)
    }


    pub(crate) fn sha256(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
        let cost = CostChange::deduct(gas::CRYPTO_SHA256_PER_BYTE * input_bytes.len() as u64);
        let mut hasher = Sha256::new();
        hasher.update(input_bytes);
        let ret = hasher.finalize().to_vec();
        (ret, cost)
    }

    pub(crate) fn keccak256(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
        let cost = CostChange::deduct(gas::CRYPTO_KECCAK256_PER_BYTE * input_bytes.len() as u64);
        let mut output_bytes = [0u8; 32];
        let mut keccak = Keccak::v256();
        keccak.update(&input_bytes);
        keccak.finalize(&mut output_bytes);
        let ret = output_bytes.to_vec();
        (ret, cost)
    }

    pub(crate) fn ripemd(input_bytes: Vec<u8>) -> OperationReceipt<Vec<u8>> {
        let cost = CostChange::deduct(gas::CRYPTO_RIPEMD160_PER_BYTE * input_bytes.len() as u64);
        let mut hasher = Ripemd160::new();
        hasher.update(&input_bytes);
        let ret = hasher.finalize().to_vec();
        (ret, cost)
    }

    pub(crate) fn verify_ed25519_signature(
        message: Vec<u8>,
        signature: Vec<u8>,
        pub_key: Vec<u8>,
    ) -> OperationReceipt<Result<i32, anyhow::Error>> {
        let cost = CostChange::deduct(gas::crypto_verify_ed25519_signature_cost(message.len()));
        let public_key = match ed25519_dalek::PublicKey::from_bytes(&pub_key) {
            Ok(public_key) => public_key,
            Err(e) => return (Err(e.into()), cost)
        };
        let dalek_signature = match ed25519_dalek::Signature::from_bytes(&signature) {
            Ok(dalek_signature) => dalek_signature,
            Err(e) => return (Err(e.into()), cost)
        };
        let is_ok = public_key.verify(&message, &dalek_signature).is_ok();

        (Ok(is_ok as i32), cost)
    }
}
