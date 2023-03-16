/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Structures and functions useful in state transition across different phases.

use std::{sync::{Arc, Mutex}, ops::{Deref, DerefMut}};
use pchain_types::{PublicAddress, TREASURY_CUT_OF_BASE_FEE, TOTAL_BASE_FEE};
use pchain_world_state::{storage::WorldStateStorage, keys::AppKey, network::network_account::NetworkAccountStorage};
use wasmer::Store;

use crate::{
    transition::{TransitionContext, StateChangesResult, ReadWriteSet}, 
    contract::{MethodCallError, self, ModuleBuildError, ContractValidateError, SmartContractContext, ContractBinaryFunctions}, 
    wasmer::{wasmer_store, wasmer_env}, 
    Cache, 
    gas::{self, CostChange}, 
    TransitionError, 
    types::{CallTx, BaseTx}, BlockchainParams
};

/// StateInTransit is a collection of all useful information required to transit an state through Phases.
/// Methods defined in StateInTransit do not directly update data to world state, but associate with the
/// [crate::transition::ReadWriteSet] in [TransitionContext] which serves as a data cache in between runtime and world state.
pub(crate) struct StateInTransit<S> 
    where S: WorldStateStorage + Send + Sync + Clone +'static 
{
    /// Base Transaction as a transition input
    pub tx: BaseTx,
    /// Blockchain data as a transition input
    pub bd: BlockchainParams,
    /// Transition Context which also contains world state as input
    pub ctx: TransitionContext<S>,
}

impl<S> Deref for StateInTransit<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    type Target = TransitionContext<S>;

    fn deref(&self) -> &Self::Target {
        &self.ctx    
    }
}

impl<S> DerefMut for StateInTransit<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ctx    
    }
}

/// StateInTransit implements NetworkAccountStorage with Read Write operations that:
/// - Gas is charged in every Get/Contains/Set
/// - Account State (for app data) is opened in every Set to contract storage
impl<S> NetworkAccountStorage for StateInTransit<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.app_data(pchain_types::NETWORK_ADDRESS, AppKey::new(key.to_vec())).0
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.contains_app_data(pchain_types::NETWORK_ADDRESS, AppKey::new(key.to_vec()))
    }

    fn set(&mut self, key: &[u8], value: Vec<u8>) {
        self.set_app_data(pchain_types::NETWORK_ADDRESS, AppKey::new(key.to_vec()), value);
    }

    fn delete(&mut self, key: &[u8]) {
        self.set_app_data(pchain_types::NETWORK_ADDRESS, AppKey::new(key.to_vec()), Vec::new());
    }
}

/// Tentative Charge is a Phase in State Transition. It transits state and returns gas consumption if success.
pub(crate) fn tentative_charge<S>(state: &mut StateInTransit<S>) ->  Result<u64, TransitionError>
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    let init_gas = gas::tx_base_cost();
    if state.tx.gas_limit < init_gas {
        return Err(TransitionError::PreExecutionGasExhausted)
    }

    let signer = state.tx.signer;
    let origin_nonce = state.ws.nonce(signer);
    if state.tx.nonce != origin_nonce {
        return Err(TransitionError::WrongNonce)
    }

    let origin_balance = state.ws.balance(state.tx.signer);
    let gas_limit = state.tx.gas_limit;
    let base_fee = state.bd.this_base_fee;
    let priority_fee = state.tx.priority_fee_per_gas;
    if (gas_limit * ( base_fee + priority_fee )) > origin_balance {
        return Err(TransitionError::NotEnoughBalanceForGasLimit)
    }
    // Apply change directly to World State
    state.ws.with_commit().set_balance(signer,
        origin_balance 
        - gas_limit * ( base_fee + priority_fee )
    );

    state.set_gas_consumed(init_gas);
    Ok(init_gas)
}

/// finalize gas consumption for this work step. Return Error GasExhaust if gas has already been exhausted
pub(crate) fn finalize_gas_consumption<S>(mut state: StateInTransit<S>) -> Result<StateInTransit<S>, StateChangesResult<S>>
    where S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static
{
    let gas_used = state.gas_consumed().saturating_add(state.chargeable_gas());
    if state.tx.gas_limit <  gas_used {
        return Err(abort(state, TransitionError::ExecutionProperGasExhausted))
    }
    state.set_gas_consumed(gas_used);
    Ok(state)
}

/// Abort is operation that causes all World State sets in the Work Phase to be reverted.
pub(crate) fn abort<S>(mut state: StateInTransit<S>, transition_err: TransitionError) -> StateChangesResult<S>
    where S: pchain_world_state::storage::WorldStateStorage + Send + Sync + Clone + 'static
{
    state.revert_changes();
    // read cost is mandatory in gas consumption
    let gas_used = std::cmp::min(state.tx.gas_limit, state.gas_consumed().saturating_add(state.minimum_chargeable_gas()));
    state.set_gas_consumed(gas_used);
    charge(state, Some(transition_err))
}

/// Charge is a Phase in State Transition. It finalizes balance of accounts to world state.
pub(crate) fn charge<S>(mut state: StateInTransit<S>, transition_result: Option<TransitionError>) -> StateChangesResult<S>
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    let signer = state.tx.signer;
    let base_fee = state.bd.this_base_fee;
    let priority_fee = state.tx.priority_fee_per_gas;
    let gas_used = std::cmp::min(state.gas_consumed(), state.tx.gas_limit); // Safety for avoiding overflow
    let gas_unused = state.tx.gas_limit.saturating_sub(gas_used);

    // Finalize signer's balance
    let signer_balance = state.purge_balance(signer);
    let new_signer_balance = signer_balance + gas_unused * ( base_fee + priority_fee);

    // Transfer priority fee to Proposer
    let proposer_address = state.bd.proposer_address;
    let mut proposer_balance = state.purge_balance(proposer_address);
    if signer == proposer_address {
        proposer_balance = new_signer_balance;
    }
    let new_proposer_balance = proposer_balance.saturating_add(gas_used * priority_fee);

    // Burn the gas to Treasury account
    let treasury_address = state.bd.treasury_address;
    let mut treasury_balance = state.purge_balance(treasury_address);
    if signer == treasury_address {
        treasury_balance = new_signer_balance;
    }
    if proposer_address == treasury_address {
        treasury_balance = new_proposer_balance;
    }
    let new_treasury_balance = treasury_balance.saturating_add((gas_used * base_fee * TREASURY_CUT_OF_BASE_FEE) / TOTAL_BASE_FEE);

    // Commit updated balances
    state.ws.with_commit().set_balance(signer, new_signer_balance);
    state.ws.with_commit().set_balance(proposer_address, new_proposer_balance);
    state.ws.with_commit().set_balance(treasury_address, new_treasury_balance);
    
    // Commit Signer's Nonce
    let nonce = state.ws.nonce(signer).saturating_add(1);
    state.ws.with_commit().set_nonce(signer, nonce);

    state.set_gas_consumed(gas_used);
    StateChangesResult::new(state, transition_result)
}

/// ContractModule stores the intermediate data related to Contract in Work Phase.
pub(crate) struct ContractModule {
    store: Store,
    module: contract::Module,
    /// Gas cost for getting contract code
    pub gas_cost: CostChange
}

impl ContractModule {

    pub(crate) fn new(contract_code: &Vec<u8>, memory_limit: Option<usize>) -> Result<Self, ModuleBuildError> {
        let wasmer_store = wasmer_store::new(u64::MAX, memory_limit);
        // Load the contract module from raw bytes here because it is not expected to save into sc_cache at this point of time.
        let module = contract::Module::from_wasm_bytecode(contract::CBI_VERSION, contract_code, &wasmer_store)?;

        Ok(Self {
            store: wasmer_store,
            module,
            gas_cost: CostChange::default()
        })
    }

    pub(crate) fn build_contract<S>(
        contract_address: PublicAddress,
        sc_ctx: &SmartContractContext,
        rw_set: &ReadWriteSet<S>
    ) -> Result<Self, ()> 
        where S: WorldStateStorage + Send + Sync + Clone + 'static
    {
        let (module, store, gas_cost) = {
            let (result, gas_cost) = rw_set.code_from_sc_cache(contract_address, sc_ctx);
            match result {
                Some((module, store)) => (module, store, gas_cost),
                None => return Err(())
            }
        };

        Ok(Self { store, module, gas_cost })
    }

    pub(crate) fn validate(&self) -> Result<(), ContractValidateError> {
        self.module.validate_contract(&self.store)
    }

    pub(crate) fn cache(&self, contract_address: PublicAddress, cache: &mut Cache) {
        self.module.cache_to(contract_address, cache)
    } 

    pub(crate) fn instantiate<S>(self, 
        ctx: Arc<Mutex<TransitionContext<S>>>,
        call_counter: u32,
        tx: CallTx,
        bd: BlockchainParams
    ) -> Result<ContractInstance<S>, ()> 
        where S: WorldStateStorage + Send + Sync + Clone + 'static
    {
        let gas_limit = tx.gas_limit;
        let environment = wasmer_env::Env::new(
            ctx, 
            call_counter,
            tx, 
            bd
        );

        let importable =contract::create_importable::<wasmer_env::Env<S>, ContractBinaryFunctions>(
            &self.store, 
            &environment,
        );
    
        let instance = self.module.instantiate(&importable, gas_limit).map_err(|_| ())?;

        Ok(ContractInstance { environment, instance })
    }

}

/// ContractInstance contains contract instance which is prepared to be called in Work Phase.
pub(crate) struct ContractInstance<S> 
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    environment: wasmer_env::Env<S>,
    instance: contract::Instance
}

impl<S> ContractInstance<S> 
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    pub(crate) fn call(self) -> (TransitionContext<S>, u64, Option<MethodCallError>) {
        // initialize the variable of wasmer remaining gas
        self.environment.init_wasmer_remaining_points(self.instance.remaining_points());
    
        // Invoke Wasm Execution
        let call_result = unsafe { self.instance.call_method() };
        
        let non_wasmer_gas_amount = self.environment.get_non_wasm_gas_amount();
        
        // drop the variable of wasmer remaining gas
        self.environment.drop_wasmer_remaining_points();

        let (remaining_gas, call_error) = match call_result {
            Ok(remaining_gas) => (remaining_gas, None),
            Err((remaining_gas, call_error)) => (remaining_gas, Some(call_error)),
        };

        let total_gas = self.environment.call_tx.gas_limit
            .saturating_sub(remaining_gas)
            .saturating_sub(non_wasmer_gas_amount); // add back the non_wasmer gas because it is already accounted in read write set.

        // Get the updated TransitionContext
        let ctx = self.environment.context.lock().unwrap().clone();
        (ctx, total_gas, call_error)
    }
}