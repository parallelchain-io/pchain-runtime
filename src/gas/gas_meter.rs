/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Global [GasMeter] for tracking gas usage throughout the entire state transition.
//!
//! Acting as a facade, this GasMeter provides access to gas-related operations
//! outside of a Wasm environment through its exposed methods,
//! and serves as the authoritative source for the gas usage tracking.
//!
//! It integrates with the [WasmerGas](crate::gas::wasmer_gas) module for Wasm gas tracking,
//! with the latter's usage being recorded and then deducted from this GasMeter.
//!
//! The GasMeter is tied to its parent [TransitionContext](crate::context::TransitionContext)
//! and remains active for the full duration of the state transition.
//! Designed as a singleton, the GasMeter can be cloned for operational convenience,
//! yet there is always a single, authoritative instance in operation at any given time.

use std::cell::RefCell;

use crate::execution::cache::{CommandOutputCache, WorldStateCache};
use crate::{
    contract::{ContractModule, SmartContractContext},
    types::{CommandKind, CommandOutput, TxnVersion},
    TransitionError,
};
use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{NetworkAccountStorage, VersionProvider, DB, NETWORK_ADDRESS};

use super::{
    constants::{tx_inclusion_cost_v1, tx_inclusion_cost_v2},
    operations::{self, OperationReceipt},
    CostChange,
};

/// GasMeter contains both gas-accounting variables and data structures which involve chargeable operations.
#[derive(Clone)]
pub(crate) struct GasMeter<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// version of the transaction being processed
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

    /* ↓↓↓ Operations involving the following data structures are chargeable ↓↓↓ */
    /// stores all resulting outputs from executing the current command
    pub output_cache_of_current_command: CommandOutputCache,

    /// handles all operations involving World State
    pub ws_cache: WorldStateCache<'a, S, V>,
}

impl<'a, S, V> GasMeter<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    pub fn new(version: TxnVersion, ws_cache: WorldStateCache<'a, S, V>, gas_limit: u64) -> Self {
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

    /* ↓↓↓ Gas accounting methods ↓↓↓ */

    /// Use with caution!! Method adds on gas consumed outside of calling GasMeter methods,
    /// which should be exceptional cases, not the norm, for accurate gas charging.
    ///
    /// Presently used only in the following scenarios:
    /// 1) Gas consumed in Wasm environment as they are not metered by GasMeter
    /// 2) Gas consumed for exhausting gas fully in WithdrawDeposit, StakeDeposit and UnstakeDeposit
    /// when the commands are executed with insufficient gas to write the return value
    pub fn manually_charge_gas(&mut self, gas: u64) {
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

    /* ↓↓↓ Facade methods for transaction storage operations ↓↓↓ */

    pub fn charge_txn_pre_exec_inclusion(
        &mut self,
        version: TxnVersion,
        tx_size: usize,
        tx_command_kinds: &Vec<CommandKind>,
    ) -> Result<(), TransitionError> {
        // stored separately because it is not considered to belong to a single command
        let required_cost = match version {
            TxnVersion::V1 => tx_inclusion_cost_v1(tx_size, tx_command_kinds),
            TxnVersion::V2 => tx_inclusion_cost_v2(tx_size, tx_command_kinds),
        };

        if required_cost > self.gas_limit {
            return Err(TransitionError::PreExecutionGasExhausted);
        } else {
            self.gas_used_for_txn_inclusion = required_cost;
        }
        Ok(())
    }

    pub fn command_output_set_return_value(&mut self, return_value: Vec<u8>) {
        let result = operations::command_output_set_return_value(
            self.output_cache_of_current_command.return_value.as_mut(),
            return_value,
        );
        self.charge(result)
    }

    pub fn command_output_set_amount_withdrawn(&mut self, amount_withdrawn: u64) {
        let result = operations::command_output_set_amount_withdrawn(
            self.output_cache_of_current_command
                .amount_withdrawn
                .as_mut(),
            amount_withdrawn,
        );
        self.charge(result)
    }

    pub fn command_output_set_amount_staked(&mut self, amount_staked: u64) {
        let result = operations::command_output_set_amount_staked(
            self.output_cache_of_current_command.amount_staked.as_mut(),
            amount_staked,
        );
        self.charge(result)
    }

    pub fn command_output_set_amount_unstaked(&mut self, amount_unstaked: u64) {
        let result = operations::command_output_set_amount_unstaked(
            self.output_cache_of_current_command
                .amount_unstaked
                .as_mut(),
            amount_unstaked,
        );
        self.charge(result)
    }

    /* ↓↓↓ Facade methods for World State operations ↓↓↓ */

    //
    // CONTAINS methods
    //
    pub fn ws_contains_storage_data(&mut self, address: PublicAddress, key: &[u8]) -> bool {
        let result =
            operations::ws_contains_storage_data(self.version, &mut self.ws_cache, address, key);
        self.charge(result)
    }

    //
    // GET methods
    //
    pub fn ws_storage_data(&mut self, address: PublicAddress, key: &[u8]) -> Option<Vec<u8>> {
        let result = operations::ws_storage_data(self.version, &mut self.ws_cache, address, key);
        let value = self.charge(result)?;
        (!value.is_empty()).then_some(value)
    }

    pub fn ws_balance(&self, address: PublicAddress) -> u64 {
        let result = operations::ws_balance(&self.ws_cache, &address);
        self.charge(result)
    }

    pub fn ws_cbi_version(&self, address: PublicAddress) -> Option<u32> {
        let result = operations::ws_cbi_version(&self.ws_cache, &address);
        self.charge(result)
    }

    pub fn ws_cached_contract(
        &self,
        address: PublicAddress,
        sc_context: &SmartContractContext,
    ) -> Option<ContractModule> {
        self.charge(operations::ws_cached_contract(
            &self.ws_cache,
            sc_context,
            address,
        ))
    }

    //
    // SET methods
    //
    pub fn ws_set_storage_data(&mut self, address: PublicAddress, key: &[u8], value: Vec<u8>) {
        let result =
            operations::ws_set_storage_data(self.version, &mut self.ws_cache, address, key, value);
        self.charge(result)
    }

    /// Sets balance in the write set, note it does not write to WS immediately.
    pub fn ws_set_balance(&mut self, address: PublicAddress, value: u64) {
        let result = operations::ws_set_balance(&mut self.ws_cache, address, value);
        self.charge(result)
    }

    /// Sets CBI version in the write set, note it does not write to WS immediately.
    pub fn ws_set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) {
        let result = operations::ws_set_cbi_version(&mut self.ws_cache, address, cbi_version);
        self.charge(result)
    }

    /// Sets contract bytecode in the write set, note it does not write to WS immediately.
    pub fn ws_set_code(&mut self, address: PublicAddress, code: Vec<u8>) {
        let result = operations::ws_set_contract_code(&mut self.ws_cache, address, code);
        self.charge(result)
    }
}

/// GasMeter implements NetworkAccountStorage to expose *chargeable* read-write operations to the
/// network account's storage, such as when contracts interact with it.
impl<'a, S, V> NetworkAccountStorage for GasMeter<'a, S, V>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    fn get(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        self.ws_storage_data(NETWORK_ADDRESS, key)
    }

    fn contains(&mut self, key: &[u8]) -> bool {
        self.ws_contains_storage_data(NETWORK_ADDRESS, key)
    }

    fn set(&mut self, key: &[u8], value: Vec<u8>) {
        self.ws_set_storage_data(NETWORK_ADDRESS, key, value)
    }

    fn delete(&mut self, key: &[u8]) {
        self.ws_set_storage_data(NETWORK_ADDRESS, key, Vec::new())
    }
}

/// Struct for recording gas used by a single command.
/// A `RefCell` is used here to enable interior mutability.
/// This design choice allows the struct to modify its `total` field
/// (the gas used) even when methods are called with only a read-only reference.
#[derive(Clone, Default)]
pub(crate) struct GasUsed {
    total: RefCell<CostChange>,
}

impl GasUsed {
    pub fn chargeable_cost(&self) -> u64 {
        self.total.borrow().net_cost().0
    }

    pub fn charge(&self, cost_change: CostChange) {
        *self.total.borrow_mut() += cost_change;
    }

    pub fn reset(&mut self) {
        *self.total.borrow_mut() = CostChange::default();
    }
}
