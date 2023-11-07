/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines structs for contract instantiation and contract call which are used in executing Commands Phase.

use std::{
    mem::transmute,
    sync::{Arc, Mutex},
};

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{VersionProvider, DB};
// use pchain_world_state::storage::WorldStateStorage;
use wasmer::Store;

use crate::{
    contract::{
        self,
        wasmer::module::ModuleBuildError,
        wasmer::{cache::Cache, env, store},
        wasmer::{instance::ContractValidateError, module::Module},
        HostFunctions,
    },
    transition::TransitionContext,
    types::CallTx,
    BlockchainParams,
};

use super::{instance::ContractInstance, SmartContractContext};

/// ContractModule stores the intermediate data related to Contract in Commands Phase.
pub(crate) struct ContractModule {
    store: Store,
    module: Module,
}

impl ContractModule {
    pub fn from_cache(address: PublicAddress, sc_context: &SmartContractContext) -> Option<Self> {
        let store = store::instantiate_store(u64::MAX, sc_context.memory_limit);
        sc_context
            .cache
            .as_ref()
            .and_then(|cache| Module::from_cache(address, cache, &store))
            .map(|module| Self { store, module })
    }

    pub(crate) fn from_contract_code(
        contract_code: &Vec<u8>,
        memory_limit: Option<usize>,
    ) -> Result<Self, ModuleBuildError> {
        let store = store::instantiate_store(u64::MAX, memory_limit);
        // Load the contract module from raw bytes here because it is not expected to save into sc_cache at this point of time.
        let module = Module::from_wasm_bytecode(contract::CBI_VERSION, contract_code, &store)?;

        Ok(Self { store, module })
    }

    pub(crate) fn from_contract_code_unchecked(
        address: PublicAddress,
        contract_code: &Vec<u8>,
        sc_context: &SmartContractContext,
    ) -> Option<Self> {
        let store = store::instantiate_store(u64::MAX, sc_context.memory_limit);

        let module =
            Module::from_wasm_bytecode_unchecked(contract::CBI_VERSION, contract_code, &store)
                .ok()?;

        if let Some(sc_cache) = &sc_context.cache {
            module.cache_to(address, sc_cache);
        }

        Some(Self { store, module })
    }

    pub(crate) fn validate(&self) -> Result<(), ContractValidateError> {
        self.module.validate_contract(&self.store)
    }

    pub(crate) fn cache(&self, contract_address: PublicAddress, cache: &Cache) {
        self.module.cache_to(contract_address, cache)
    }

    pub(crate) fn bytes_length(&self) -> usize {
        self.module.bytes_length()
    }

    pub(crate) fn instantiate<'a, S, V>(
        self,
        ctx: Arc<Mutex<TransitionContext<'a, S, V>>>,
        call_counter: u32,
        is_view: bool,
        tx: CallTx,
        bd: BlockchainParams,
    ) -> Result<ContractInstance<'a, S, V>, ()>
    where
        S: DB + Send + Sync + Clone + 'static,
        // TODO this may be hacky, using static on V
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        let gas_limit = tx.gas_limit;
        let environment = env::Env::new(ctx, call_counter, is_view, tx, bd);

        // SAFETY: The following unsafe block assumes that the Env AWLAYS outlives the Wasm instance.
        // This invariant is maintained because a new Wasm instance is created on each call.
        // Any code change that violates this assumption could lead to undefined behavior.
        let env_static: &env::Env<'static, S, V> =
            unsafe { transmute::<&env::Env<'a, S, V>, &env::Env<'static, S, V>>(&environment) };

        // Now `env_static` can be used with `create_importable_view` or other functions
        // expecting a `'static` lifetime. It is the programmer's responsibility to ensure
        // that `env_static` does not outlive `env`.

        let importable = if is_view {
            contract::create_importable_view::<env::Env<'static, S, V>, HostFunctions>(
                &self.store,
                env_static,
            )
        } else {
            contract::create_importable::<env::Env<'static, S, V>, HostFunctions>(
                &self.store,
                env_static,
            )
        };

        let environment: env::Env<'a, S, V> = unsafe { transmute(environment) };

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
