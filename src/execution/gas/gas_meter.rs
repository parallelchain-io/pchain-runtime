/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use core::panic;
use std::cell::RefCell;

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{
    keys::AppKey,
    network::{constants::NETWORK_ADDRESS, network_account::NetworkAccountStorage},
    storage::WorldStateStorage,
};

use crate::{
    contract::{ContractModule, SmartContractContext},
    gas, TransitionError, types::{TxnVersion, CommandOutput},
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
    pub version: TxnVersion,

    /// gas limit of the entire txn
    pub gas_limit: u64,

    /// stores txn inclusion gas separately because it is not considered to belong to a single command
    gas_used_for_txn_inclusion: u64,

    /// cumulative gas used for all executed commands
    total_gas_used_for_executed_commands: u64,

    /// stores the gas used by current command,
    /// finalized and reset at the end of each command
    gas_used_for_current_command: GasUsed,

    /*** Mutation on the below Data should be charged. ***/
    pub output_cache_of_current_command: CommandOutputCache,

    pub ws_cache: WorldStateCache<S>,
}

impl<S> GasMeter<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub fn new(version: TxnVersion, ws_cache: WorldStateCache<S>, gas_limit: u64) -> Self {
        Self {
            version,
            ws_cache,
            gas_limit,
            total_gas_used_for_executed_commands: 0,
            gas_used_for_txn_inclusion: 0,
            gas_used_for_current_command: GasUsed::default(),
            output_cache_of_current_command: CommandOutputCache::default(),
        }
    }

    /// A checkpoint function to be called after every command execution. It returns the
    /// data for generating the command receipt, and updates the gas counter which is used
    /// at the end of transaction execution.
    pub fn take_current_command_result(&mut self) -> (u64, CommandOutput) {
        let command_output = self.output_cache_of_current_command.take();

        // check if the gas used for current command exceeds gas limit, and use the clamped value
        // as the field 'gas_used' in the command receipt.
        let gas_used = {
            let gas_used_for_current_command = self.gas_used_for_current_command.chargeable_cost();
            let max_allowable_gas_used_for_current_command = self
                .gas_limit
                .saturating_sub(self.total_gas_used_for_executed_commands());
            std::cmp::min(
                gas_used_for_current_command,
                max_allowable_gas_used_for_current_command,
            )
        };

        // update the total gas used
        self.total_gas_used_for_executed_commands = self
            .total_gas_used_for_executed_commands
            .saturating_add(gas_used);

        // reset gas counter which can be then used for next command execution
        self.gas_used_for_current_command.reset();

        (gas_used, command_output)
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
        self.gas_used_for_current_command
            .charge(CostChange::deduct(gas));
    }

    fn charge<T>(&self, op_receipt: OperationReceipt<T>) -> T {
        self.gas_used_for_current_command.charge(op_receipt.1);
        op_receipt.0
    }

    /// returns the theoretical max gas used so far
    /// may exceed gas_limit
    pub fn total_gas_used(&self) -> u64 {
        self.total_gas_used_for_executed_commands()
            .saturating_add(self.gas_used_for_current_command.chargeable_cost())
    }

    /// returns gas that has been used so far
    /// will not exceed maximum
    pub fn total_gas_used_for_executed_commands(&self) -> u64 {
        self.gas_used_for_txn_inclusion
            .saturating_add(self.total_gas_used_for_executed_commands)
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
        let required_cost = gas::tx_inclusion_cost_v1(tx_size, commands_len);
        if required_cost > self.gas_limit {
            return Err(TransitionError::PreExecutionGasExhausted);
        } else {
            self.gas_used_for_txn_inclusion = required_cost;
        }
        Ok(())
    }

    pub fn command_output_set_return_values(&mut self, return_values: Vec<u8>) {
        let result = operation::command_output_set_return_values(
            self.output_cache_of_current_command.return_values.as_mut(),
            return_values,
        );
        self.charge(result)
    }

    pub fn command_output_set_amount_withdrawn(&mut self, amount_withdrawn: u64) {
        let result = operation::command_output_set_amount_withdrawn(
            self.output_cache_of_current_command.amount_withdrawn.as_mut(),
            amount_withdrawn,
        );
        self.charge(result)
    }

    pub fn command_output_set_amount_staked(&mut self, amount_staked: u64) {
        let result = operation::command_output_set_amount_staked(
            self.output_cache_of_current_command.amount_staked.as_mut(),
            amount_staked,
        );
        self.charge(result)
    }

    pub fn command_output_set_amount_unstaked(&mut self, amount_unstaked: u64) {
        let result = operation::command_output_set_amount_unstaked(
            self.output_cache_of_current_command.amount_unstaked.as_mut(),
            amount_unstaked,
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
            self.version,
            &mut self.ws_cache,
            CacheKey::App(address, app_key),
            CacheValue::App(value),
        );
        self.charge(result)
    }

    /// Sets balance in the write set. It does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        let result = operation::ws_set(
            self.version,
            &mut self.ws_cache,
            CacheKey::Balance(address),
            CacheValue::Balance(value),
        );
        self.charge(result)
    }

    /// Sets CBI version in the write set. It does not write to WS immediately.
    pub fn ws_set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        let result = operation::ws_set(
            self.version,
            &mut self.ws_cache,
            CacheKey::CBIVersion(address),
            CacheValue::CBIVersion(cbi_version),
        );
        self.charge(result)
    }

    /// Sets contract bytecode in the write set. It does not write to WS immediately.
    pub fn ws_set_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        let result = operation::ws_set(
            self.version,
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
