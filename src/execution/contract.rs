/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines structs for contract instantiation and contract call which are used in executing Commands Phase.

use std::sync::{Arc, Mutex};

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::storage::WorldStateStorage;
use wasmer::Store;

use crate::{
    contract::{
        self, ContractBinaryFunctions, ContractValidateError, MethodCallError, ModuleBuildError,
    },
    cost::CostChange,
    transition::TransitionContext,
    types::CallTx,
    wasmer::{wasmer_env, wasmer_store},
    BlockchainParams, Cache,
};

/// ContractModule stores the intermediate data related to Contract in Commands Phase.
pub(crate) struct ContractModule {
    store: Store,
    module: contract::Module,
    /// Gas cost for getting contract code
    pub gas_cost: CostChange,
}

impl ContractModule {
    pub(crate) fn new(
        contract_code: &Vec<u8>,
        memory_limit: Option<usize>,
    ) -> Result<Self, ModuleBuildError> {
        let wasmer_store = wasmer_store::instantiate_store(u64::MAX, memory_limit);
        // Load the contract module from raw bytes here because it is not expected to save into sc_cache at this point of time.
        let module = contract::Module::from_wasm_bytecode(
            contract::CBI_VERSION,
            contract_code,
            &wasmer_store,
        )?;

        Ok(Self {
            store: wasmer_store,
            module,
            gas_cost: CostChange::default(),
        })
    }

    pub(crate) fn build_contract<S>(
        contract_address: PublicAddress,
        transition_ctx: &TransitionContext<S>,
    ) -> Result<Self, ()>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static,
    {
        let (module, store) = {
            match transition_ctx
                .gas_meter
                .ws_get_cached_contract(contract_address, &transition_ctx.sc_context)
            {
                Some((module, store)) => (module, store),
                None => return Err(()),
            }
        };

        // TODO PENDING remove this cost change no longer used
        Ok(Self {
            store,
            module,
            gas_cost: CostChange::default(),
        })
    }

    pub(crate) fn validate(&self) -> Result<(), ContractValidateError> {
        self.module.validate_contract(&self.store)
    }

    pub(crate) fn cache(&self, contract_address: PublicAddress, cache: &mut Cache) {
        self.module.cache_to(contract_address, cache)
    }

    pub(crate) fn instantiate<S>(
        self,
        ctx: Arc<Mutex<TransitionContext<S>>>,
        call_counter: u32,
        is_view: bool,
        tx: CallTx,
        bd: BlockchainParams,
    ) -> Result<ContractInstance<S>, ()>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static,
    {
        let gas_limit = tx.gas_limit;
        let environment = wasmer_env::Env::new(ctx, call_counter, is_view, tx, bd);

        let importable = if is_view {
            contract::create_importable_view::<wasmer_env::Env<S>, ContractBinaryFunctions>(
                &self.store,
                &environment,
            )
        } else {
            contract::create_importable::<wasmer_env::Env<S>, ContractBinaryFunctions>(
                &self.store,
                &environment,
            )
        };

        let instance = self
            .module
            .instantiate(&importable, gas_limit)
            .map_err(|_| ())?;

        Ok(ContractInstance {
            environment,
            instance,
        })
    }
}

/// ContractInstance contains contract instance which is prepared to be called in Commands Phase.
pub(crate) struct ContractInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    environment: wasmer_env::Env<S>,
    instance: contract::Instance,
}

impl<S> ContractInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub(crate) fn call(self) -> (TransitionContext<S>, u64, Option<MethodCallError>) {
        // initialize the variable of wasmer remaining gas
        self.environment
            .init_wasmer_remaining_points(self.instance.remaining_points());

        // Invoke Wasm Execution
        let call_result = unsafe { self.instance.call_method() };

        // TODO 7 - `non_wasmer_gas_amount` is no longer needed, can remove every where
        //
        // can run tests and double check this value will be 0
        let non_wasmer_gas_amount = self.environment.get_non_wasm_gas_amount();

        // drop the variable of wasmer remaining gas
        self.environment.drop_wasmer_remaining_points();

        let (remaining_gas, call_error) = match call_result {
            Ok(remaining_gas) => (remaining_gas, None),
            Err((remaining_gas, call_error)) => (remaining_gas, Some(call_error)),
        };

        let total_gas = self
            .environment
            .call_tx
            .gas_limit
            .saturating_sub(remaining_gas)
            .saturating_sub(non_wasmer_gas_amount); // add back the non_wasmer gas because it is already accounted in read write set.

        // Get the updated TransitionContext
        let ctx = self.environment.context.lock().unwrap().clone();
        (ctx, total_gas, call_error)
    }
}
