/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use core::panic;
use std::cell::RefCell;

use pchain_types::{blockchain::Log, cryptography::PublicAddress};
use pchain_world_state::{
    keys::AppKey,
    network::{constants::NETWORK_ADDRESS, network_account::NetworkAccountStorage},
    storage::WorldStateStorage,
};

use crate::{
    contract::{ContractModule, SmartContractContext},
    gas, TransitionError,
};

use super::{
    operation::{self, OperationReceipt},
    CostChange,
};

use crate::execution::cache::{CacheKey, CacheValue, CommandOutputCache, WorldStateCache};

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

impl<S> GasMeter<S>
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
            current_command_output_cache: CommandOutputCache::default(),
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
        self.current_command_gas_used
            .charge(CostChange::deduct(gas));
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
            return_values,
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
        let result =
            operation::ws_contains(&self.ws_cache, &CacheKey::App(address, app_key.clone()));
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
    ) -> Option<ContractModule> {
        self.charge(operation::ws_get_cached_contract(
            &self.ws_cache,
            sc_context,
            address,
        ))
    }

    //
    // SET methods
    //
    pub fn ws_set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        let result = operation::ws_set(
            &mut self.ws_cache,
            CacheKey::App(address, app_key),
            CacheValue::App(value),
        );
        self.charge(result)
    }

    /// Sets balance in the write set. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        let result = operation::ws_set(
            &mut self.ws_cache,
            CacheKey::Balance(address),
            CacheValue::Balance(value),
        );
        self.charge(result)
    }

    /// Sets CBI version in the write set. It does not write to WS immediately.
    pub fn ws_set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        let result = operation::ws_set(
            &mut self.ws_cache,
            CacheKey::CBIVersion(address),
            CacheValue::CBIVersion(cbi_version),
        );
        self.charge(result)
    }

    /// Sets contract bytecode in the write set. It does not write to WS immediately.
    pub fn ws_set_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        let result = operation::ws_set(
            &mut self.ws_cache,
            CacheKey::ContractCode(address),
            CacheValue::ContractCode(code),
        );
        self.charge(result)
    }
}

/// GasMeter implements NetworkAccountStorage with charegable read-write operations to world state
impl<S> NetworkAccountStorage for GasMeter<S>
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
