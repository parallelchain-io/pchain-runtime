/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! transition defines the formulation of state transition.

use std::{
    collections::HashMap, 
    cell::RefCell, ops::{Deref, DerefMut}
};
use pchain_types::{PublicAddress, CommandReceipt, ExitStatus, Command, Serializable};
use pchain_world_state::{
    storage::WorldStateStorage, 
    states::{WorldState, AccountState}, 
    keys::AppKey
};
use wasmer::Store;

use crate::{
    transactions::{phase::StateInTransit, execute}, 
    types::{BaseTx, DeferredCommand}, 
    wasmer::cache::Cache, 
    TransitionError, gas::{CostChange, self}, contract::{Module, SmartContractContext, self}, BlockchainParams
};

/// Contract Binary Version of Runtime.
#[inline]
pub const fn cbi_version() -> u32 {
    crate::contract::CBI_VERSION
}

/// Runtime defines a virtual machine for state transition.
pub struct Runtime {
    /// Smart Contract Cache
    sc_cache: Option<Cache>,
    /// Memory limit to wasm linear memory in contract execution
    sc_memory_limit: Option<usize>
}

impl Runtime {

    /// Instantiate Runtime.
    pub fn new() -> Self {
        Self { sc_cache: None, sc_memory_limit: None }
    }

    /// specify smart contract cache to improve performance for contract code compilation.
    pub fn set_smart_contract_cache(mut self, sc_cache: Cache) -> Self {
        self.sc_cache = Some(sc_cache);
        self
    }

    /// specify the limit to wasm linear memory in contract execution.
    /// It is a tunable maximum guest memory limit that is made available to the VM
    pub fn set_smart_contract_memory_limit(mut self, memory_limit: usize) -> Self {
        self.sc_memory_limit = Some(memory_limit);
        self
    }

    /// transition performs state transition of world state (WS) from transaction (tx) and blockchain data (bd)as inputs.
    pub fn transition<S: WorldStateStorage + Send + Sync + Clone + 'static>(
        &self, 
        ws: WorldState<S>, 
        tx: pchain_types::Transaction, 
        bd: BlockchainParams
    ) -> TransitionResult<S> {
        // create transition context from world state
        let mut ctx = TransitionContext::new(ws);
        if let Some(cache) = &self.sc_cache {
            ctx.sc_context.cache = Some(cache.clone());
        }

        // transaction inputs
        let tx_size = tx.serialize().len();
        let base_tx = BaseTx::from(&tx);
        let commands = tx.commands;

        // initial state for transition
        let state = StateInTransit { tx: base_tx, tx_size, commands_len: commands.len(), ctx, bd };

        // initiate command execution
        if commands.iter().any(|c| matches!(c, Command::NextEpoch)) {
            execute::execute_next_epoch_command(state, commands)
        } else {
            execute::execute_commands(state, commands)
        }
    }
    
    /// view performs view call to a target contract
    pub fn view<S: WorldStateStorage + Send + Sync + Clone + 'static>(
        &self, 
        ws: WorldState<S>, 
        gas_limit: u64,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>
    ) -> (CommandReceipt, Option<TransitionError>)  {
        // create transition context from world state
        let mut ctx = TransitionContext::new(ws);
        if let Some(cache) = &self.sc_cache {
            ctx.sc_context.cache = Some(cache.clone());
        }
        
        // create a dummy transaction
        let dummy_tx = BaseTx {
            gas_limit, 
            ..Default::default()
        };

        let dummy_bd = BlockchainParams::default();

        // initialize state for executing view call
        let state = StateInTransit { 
            tx: dummy_tx, 
            bd: dummy_bd,
            ctx, 
            // the below fields are not cared in view call
            tx_size: 0, commands_len: 0
        };

        // execute view
        execute::execute_view(state, target, method, arguments)
    }

}

/// Result of state transition. It is the return type of `pchain_runtime::Runtime::transition`.
#[derive(Clone)]
pub struct TransitionResult<S> 
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    /// New world state (ws') after state transition
    pub new_state: WorldState<S>,
    /// Transaction receipt. None if the transition receipt is not includable in the block
    pub receipt: Option<pchain_types::Receipt>,
    /// Transition error. None if no error.
    pub error: Option<TransitionError>,
    /// Changes in validate set. 
    /// It is specific to [pchain_types::Command::NextEpoch]. None for other commands.
    pub validator_changes: Option<ValidatorChanges>,
}

pub(crate) struct StateChangesResult<S> 
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    /// resulting state in transit
    pub state: StateInTransit<S>,
    /// transition error
    pub error: Option<TransitionError>
}

impl<S> StateChangesResult<S>
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    pub(crate) fn new(state: StateInTransit<S>, transition_error: Option<TransitionError>) -> StateChangesResult<S> {
        Self {
            state,
            error: transition_error
        }
    }

    /// finalize generates TransitionResult
    pub(crate) fn finalize(self, command_receipts: Vec<CommandReceipt>) -> TransitionResult<S> {
        let error = self.error;
        let rw_set = self.state.ctx.rw_set;

        let new_state = rw_set.commit_to_world_state();
        
        TransitionResult { new_state, receipt: Some(command_receipts), error, validator_changes: None }
    }
}

/// ValidatorChanges includes
/// - the new validator set in list of tuple of operator address and power
/// - the list of address of operator who is removed from state
#[derive(Clone)]
pub struct ValidatorChanges {
    pub new_validator_set: Vec<(PublicAddress, u64)>,
    pub remove_validator_set: Vec<PublicAddress>
}

/// TransitionContext defines transiting data required for state transition.
#[derive(Clone)]
pub(crate) struct TransitionContext<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    /// Running data cache for Read-Write operations during state transition.
    pub rw_set: ReadWriteSet<S>,

    /// Smart contract context for execution
    pub sc_context: SmartContractContext,

    /// Commands that deferred from a Call Comamnd via host function specified in CBI.
    pub commands: Vec<DeferredCommand>,

    /// Gas consumed in transaction, no matter whether the transaction succeeds or fails.
    gas_used: u64,

    /// the gas charged for adding logs and setting return value in receipt.
    pub receipt_write_gas: CostChange,

    /// logs stores the list of events emitted by an execution ordered in the order of emission.
    pub logs: Vec<pchain_types::Log>,

    /// return_value is the value returned by a call transaction using the `return_value` SDK function. It is None if the
    /// execution has not/did not return anything.
    pub return_value: Option<Vec<u8>>,

}

impl<S> TransitionContext<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    pub fn new(ws: WorldState<S>) -> Self {
        Self {
            rw_set: ReadWriteSet::new(ws),
            sc_context: SmartContractContext { cache: None, memory_limit: None },
            receipt_write_gas: CostChange::default(),
            logs: Vec::new(),
            gas_used: 0,
            return_value: None,
            commands: Vec::new()
        }
    }

    pub fn gas_consumed(&self) -> u64 {
        self.gas_used
    }

    pub fn set_gas_consumed(&mut self, gas_used: u64) {
        self.gas_used = gas_used
    }
    
    /// It is equivalent to gas_consumed + chareable_gas. The chareable_gas consists of
    /// - write cost to storage
    /// - read cost to storage
    /// - write cost to receipt (blockchain data)
    pub fn total_gas_to_be_consumed(&self) -> u64 {
        // Gas incurred to be charged
        let chargeable_gas = (self.rw_set.write_gas + self.receipt_write_gas + *self.rw_set.read_gas.borrow()).values().0;
        self.gas_consumed().saturating_add(chargeable_gas)
    }

    /// Discard the changes to world state
    pub fn revert_changes(&mut self) {
        self.rw_set.reads.borrow_mut().clear();
        self.rw_set.writes.clear();
    }

    /// Output the CommandReceipt and clear the intermediate context for next command execution. 
    /// `prev_gas_used` will be needed for getting the intermediate gas consumption.
    pub fn extract(&mut self, prev_gas_used: u64, exit_status: ExitStatus) -> CommandReceipt {
        // 1. Create Command Receipt
        let ret = CommandReceipt { 
            exit_status,
            gas_used: self.gas_used.saturating_sub(prev_gas_used), 
            // Intentionally retain return_values and logs even if exit_status is failed
            return_values: self.return_value.clone().map_or(Vec::new(), std::convert::identity),
            logs: self.logs.clone()
        };
        // 2. Clear data for next command execution
        *self.rw_set.read_gas.borrow_mut() = CostChange::default();
        self.rw_set.write_gas = CostChange::default();
        self.receipt_write_gas = CostChange::default();
        self.logs.clear();
        self.return_value = None;
        self.commands.clear();
        ret
    }

    /// Pop commands from context. None if there is nothing to pop
    pub fn pop_commands(&mut self) -> Option<Vec<DeferredCommand>> {
        if self.commands.is_empty() { return None }
        let mut ret = Vec::new();
        ret.append(&mut self.commands);
        Some(ret)
    }
}

impl<S> Deref for TransitionContext<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    type Target = ReadWriteSet<S>;

    fn deref(&self) -> &Self::Target {
        &self.rw_set    
    }
}

impl<S> DerefMut for TransitionContext<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rw_set    
    }
}

/// ReadWriteSet defines data cache for Read-Write opertaions during state transition.
#[derive(Clone)]
pub(crate) struct ReadWriteSet<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    /// World State services as the data source
    pub ws: WorldState<S>,
    /// writes stores key-value pairs for Write operations. It stores the data that is pending to store into world state
    pub writes: HashMap<CacheKey, CacheValue>,
    /// reads stores key-value pairs from Read operations. It is de facto the original data read from world state.
    pub reads: RefCell<HashMap<CacheKey, Option<CacheValue>>>,
    /// write_gas is protocol defined cost that to-be-charged write operation has been executed
    pub write_gas: CostChange,
    /// read_gas is protocol defined cost that to-be-charged read operation has been executed
    pub read_gas: RefCell<CostChange>
}

impl<S> ReadWriteSet<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{

    pub fn new(ws: WorldState<S>) -> Self {
        Self { 
            ws,
            writes: HashMap::new(), 
            reads: RefCell::new(HashMap::new()),
            write_gas: CostChange::default(),
            read_gas: RefCell::new(CostChange::default())
        }
    }

    /// get the balance from readwrite set. It key is not found, then get from world state and then cache it.
    pub fn balance(&self, address: PublicAddress) -> (u64, CostChange) {
        match self.get(CacheKey::Balance(address)) {
            (Some(CacheValue::Balance(value)), cost) => (value, cost),
            _ => panic!()
        }
    }

    /// set balance to write set. This operation does not write to world state immediately
    pub fn set_balance(&mut self, address: PublicAddress, balance: u64) -> CostChange {
        self.set(CacheKey::Balance(address), CacheValue::Balance(balance))
    }
    
    /// remove cached writes and return the value
    pub fn purge_balance(&mut self,  address: PublicAddress) -> u64 {
        let (balance, _) = self.balance(address);
        let key = CacheKey::Balance(address);
        self.writes.remove(&key);
        balance
    }

    /// get the contract code from readwrite set. It key is not found, then get from world state and then cache it.
    pub fn code(&self, address: PublicAddress) -> (Option<Vec<u8>>, CostChange) {
        match self.get(CacheKey::ContractCode(address)) {
            (Some(CacheValue::ContractCode(value)), cost) => (Some(value), cost),
            (None, cost) => (None, cost),
            _ => panic!()
        }
    }

    /// get the contract code from smart contract cache. It it is not found, then get from read write set, i.e. code()
    pub fn code_from_sc_cache(&self, address: PublicAddress, sc_context: &SmartContractContext) -> (Option<(Module, Store)>, CostChange) {
        let wasmer_store = sc_context.store();
        let cached_module = match &sc_context.cache {
            Some(sc_cache) => contract::Module::from_cache(address, sc_cache, &wasmer_store),
            None => None
        };

        // found from Smart Contract Cache
        if let Some(module) = cached_module {
            let cost_change = gas::read_code_cost(module.bytes_length());
            *self.read_gas.borrow_mut() += cost_change;
            return (Some((module, wasmer_store)), cost_change )
        }

        // found from read write set or world state
        let (bytes, cost_change) = self.code(address);
        let contract_code = match bytes {
            Some(bs) => bs,
            None => return (None, cost_change)
        };

        // build module
        let module = match contract::Module::from_wasm_bytecode_unchecked(contract::CBI_VERSION, &contract_code, &wasmer_store) {
            Ok(module) => {
                // cache to sc_cache
                if let Some(sc_cache) = &sc_context.cache {
                    module.cache_to(address, &mut sc_cache.clone());
                }
                module
            },
            Err(_) => return (None, cost_change)
        };

        (Some((module, wasmer_store)), cost_change)
    }

    /// set contract bytecode. This operation does not write to world state immediately
    pub fn set_code(&mut self, address: PublicAddress, code: Vec<u8>) -> CostChange {
        self.set(CacheKey::ContractCode(address), CacheValue::ContractCode(code))
    }

    /// get the CBI version of the contract
    pub fn cbi_version(&self, address: PublicAddress) -> (Option<u32>, CostChange) {
        match self.get(CacheKey::CBIVersion(address)) {
            (Some(CacheValue::CBIVersion(value)), cost) => (Some(value), cost),
            (None, cost) => (None, cost),
            _ => panic!()
        }
    }

    /// set cbi version. This operation does not write to world state immediately
    pub fn set_cbi_version(&mut self, address: PublicAddress, cbi_version: u32) -> CostChange {
        self.set(CacheKey::CBIVersion(address), CacheValue::CBIVersion(cbi_version))
    }

    /// get the contract storage from readwrite set. It key is not found, then get from world state and then cache it.
    pub fn app_data(&self, address: PublicAddress, app_key: AppKey) -> (Option<Vec<u8>>, CostChange) {
        match self.get(CacheKey::App(address, app_key)) {
            (Some(CacheValue::App(value)), cost) => {
                if value.is_empty() {
                    (None, cost)
                } else {
                    (Some(value), cost)
                }
            },
            (None, cost) => (None, cost),
            _=>panic!()
        }
    }
    
    /// set value to contract storage. This operation does not write to world state immediately
    pub fn set_app_data(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) -> CostChange {
        self.set(CacheKey::App(address, app_key), CacheValue::App(value))
    }

    /// set value to contract storage. This operation does not write to world state immediately.
    /// It is gas-free operation.
    pub fn set_app_data_uncharged(&mut self, address: PublicAddress, app_key: AppKey, value: Vec<u8>) {
        self.writes.insert(CacheKey::App(address, app_key), CacheValue::App(value));
    }

    /// check if App Key already exists
    pub fn contains_app_data(&self, address: PublicAddress, app_key: AppKey) -> bool {
        let cache_key = CacheKey::App(address, app_key.clone());
        
        // charge gas for contains and charge gas
        *self.read_gas.borrow_mut() += gas::contains_cost(cache_key.len());

        // check from the value that was previously written/read
        self.writes.get(&cache_key).filter(|v| v.len() != 0).is_some()
        || self.reads.borrow().get(&cache_key).filter(|v| v.is_some()).is_some()
        || self.ws.contains().storage_value(&address, &app_key)
    }

    /// check if App Key already exists. It is gas-free operation.
    pub fn contains_app_data_from_account_state(&self, account_state: &AccountState<S>, app_key: AppKey) -> bool {
        let address = account_state.address();
        let cache_key = CacheKey::App(address, app_key.clone());
        
        // check from the value that was previously written/read
        self.writes.get(&cache_key).filter(|v| v.len() != 0).is_some()
        || self.reads.borrow().get(&cache_key).filter(|v| v.is_some()).is_some()
        || self.ws.contains().storage_value_from_account_state(account_state, &app_key)
    }

    /// Get app data given a account state. It is gas-free operation.
    pub fn app_data_from_account_state(&self, account_state: &AccountState<S>, app_key: AppKey) -> Option<Vec<u8>> {
        let address = account_state.address();
        let cache_key = CacheKey::App(address, app_key.clone());

        match self.writes.get(&cache_key) {
            Some(CacheValue::App(value)) => return Some(value.clone()),
            Some(_)=>panic!(),
            None => {}
        }

        match self.reads.borrow().get(&cache_key) {
            Some(Some(CacheValue::App(value))) => return Some(value.clone()),
            Some(None) => return None,
            Some(_) => panic!(),
            None => {}
        }

        self.ws.cached_get().storage_value(account_state.address(), &app_key)
            .or_else(|| account_state.get(&app_key))
    }

    /// Lowest level of get operation. It gets latest value from readwrite set. It key is not found, then get from world state and then cache it.
    fn get(&self, key: CacheKey) -> (Option<CacheValue>, CostChange) {
        // 1. Return the value that was written earlier in the transaction ('read-your-write' semantics).
        if let Some(value) = self.writes.get(&key) {
            let cost_change = self.charge_read_cost(&key, Some(value));
            return (Some(value.clone()), cost_change);
        }
        
        // 2. Return the value that was read eariler in the transaction
        if let Some(value) = self.reads.borrow().get(&key) {
            let cost_change = self.charge_read_cost(&key, value.as_ref());
            return (value.clone(), cost_change);
        }

        // 3. Get the value from world state
        let value = key.get_from_world_state(&self.ws);
        let cost_change = self.charge_read_cost(&key, value.as_ref());

        // 4. Cache to reads
        self.reads.borrow_mut().insert(key, value.clone());
        
        (value, cost_change)
    }

    /// lowest level of set operation. It inserts to Write Set and returns the gas cost for this set operation.
    fn set(&mut self, key: CacheKey, value: CacheValue) -> CostChange {
        let key_len = key.len();
        let new_val_len = value.len();

        // 1. Get the length of original value and Charge for read cost
        let old_val_len = self.get(key.clone()).0.map_or(0, |v| v.len());

        // 2. Insert to write set
        self.writes.insert(key, value);

        // 3. Calculate gas cost
        self.charge_write_cost(key_len, old_val_len, new_val_len)
    }

    fn charge_read_cost(&self, key: &CacheKey, value: Option<&CacheValue>) -> CostChange {
        let cost_change = match key {
            CacheKey::ContractCode(_) =>
                gas::read_code_cost(value.as_ref().map_or(0, |v| v.len())),
            _=> 
                gas::read_cost(key.len(), value.as_ref().map_or(0, |v| v.len()))
        };
        *self.read_gas.borrow_mut() += cost_change;
        cost_change
    }

    fn charge_write_cost(&mut self, key_len: usize, old_val_len: usize, new_val_len: usize) -> CostChange {
        let new_cost_change = gas::write_cost(key_len, old_val_len, new_val_len);
        self.write_gas += new_cost_change;
        new_cost_change
    }

    pub fn commit_to_world_state(self) -> WorldState<S> {
        let mut ws = self.ws;
        // apply changes to world state
        self.writes.into_iter().for_each(|(cache_key, new_value)|{
            new_value.set_to_world_state(cache_key, &mut ws);
        });
        ws.commit();
        ws
    }
}

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
            CacheKey::App(address, key) => 
                gas::ACCOUNT_STATE_KEY_LENGTH as usize + address.len() + key.len(),
            CacheKey::Balance(_) |
            CacheKey::ContractCode(_) |
            CacheKey::CBIVersion(_) => 
                gas::ACCOUNT_STATE_KEY_LENGTH as usize,
        }
    }

    /// get_from_world_state gets value from world state according to CacheKey
    fn get_from_world_state<S>(&self, ws: &WorldState<S>) -> Option<CacheValue>
        where S: WorldStateStorage + Send + Sync + Clone
    {
        match &self {
            CacheKey::App(address, app_key) => {
                ws.storage_value(address, app_key).map(|value|{
                    CacheValue::App(value)
                })
            },
            CacheKey::Balance(address) => {
                Some(CacheValue::Balance(ws.balance(address.to_owned())))
            },
            CacheKey::ContractCode(address) => {
                ws.code(address.to_owned()).map(|value|{
                    CacheValue::ContractCode(value)
                })
            },
            CacheKey::CBIVersion(address) => {
                ws.cbi_version(address.to_owned()).map(|value|{
                    CacheValue::CBIVersion(value)
                })
            }
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
        where S: WorldStateStorage + Send + Sync + Clone
    {
        match self {
            CacheValue::App(value) => {
                if let CacheKey::App(address, app_key) = key {
                    ws.cached().set_storage_value(address, app_key, value);
                }
            },
            CacheValue::Balance(value) => {
                if let CacheKey::Balance(address) = key {
                    ws.cached().set_balance(address, value);
                }
            },
            CacheValue::ContractCode(value) => {
                if let CacheKey::ContractCode(address) = key {
                    ws.cached().set_code(address, value);
                }
            },
            CacheValue::CBIVersion(value) => {
                if let CacheKey::CBIVersion(address) = key {
                    ws.cached().set_cbi_version(address, value);
                }
            }
        }
    }
}
