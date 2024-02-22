/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! An abstraction of the smart contract module compiled from Wasm bytecode that exposes high-level management APIs.
//!
//! The [ContractModule] struct is typically compiled during [command execution](crate::commands::account) and cached for subsequent calls.

use std::{
    mem::transmute,
    sync::{Arc, Mutex},
};

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::{VersionProvider, DB};
use wasmer::Store;

use crate::{
    context::TransitionContext,
    contract::{
        self,
        wasmer::module::ModuleBuildError,
        wasmer::{cache::Cache, env, store},
        wasmer::{instance::ContractValidateError, module::Module},
        HostFunctions,
    },
    types::CallTx,
    BlockchainParams,
};

use super::{instance::ContractInstance, SmartContractContext};

/// ContractModule contains the necessary components needed to build Wasm modules and instantiate them.
pub(crate) struct ContractModule {
    store: Store,
    module: Module,
}

impl ContractModule {
    /// called during contract invocation for faster loading of the Wasm module
    pub fn from_cache(address: PublicAddress, sc_context: &SmartContractContext) -> Option<Self> {
        let store = store::instantiate_store(u64::MAX, sc_context.memory_limit);
        sc_context
            .cache
            .as_ref()
            .and_then(|cache| Module::from_cache(address, cache, &store))
            .map(|module| Self { store, module })
    }

    /// called during initial contract deployment
    /// compiles bytecode for the very first time with validation
    pub(crate) fn from_bytecode_checked(
        contract_code: &Vec<u8>,
        memory_limit: Option<usize>,
    ) -> Result<Self, ModuleBuildError> {
        let store = store::instantiate_store(u64::MAX, memory_limit);
        let module =
            Module::from_wasm_bytecode_checked(contract::CBI_VERSION, contract_code, &store)?;
        Ok(Self { store, module })
    }

    /// called during subsequent contract invocation
    /// compiles bytecode without validation for faster performance
    pub(crate) fn from_bytecode_unchecked(
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

    /// check if the Wasm module is a proper contract according to the Parallelchain CBI
    pub(crate) fn validate_proper_contract(&self) -> Result<(), ContractValidateError> {
        self.module.validate_entry_point(&self.store)
    }

    pub(crate) fn cache(&self, contract_address: PublicAddress, cache: &Cache) {
        self.module.cache_to(contract_address, cache)
    }

    pub(crate) fn bytecode_length(&self) -> usize {
        self.module.bytecode_length()
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
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        let gas_limit = tx.gas_limit;
        let environment = env::Env::new(ctx, call_counter, is_view, tx, bd);

        // SAFETY: The following unsafe block assumes that the Env AWLAYS outlives the Wasm instance.
        // This invariant is guaranteed because a new Wasm instance is created on each call,
        // hence `env` is essentially "static" for the lifetime of the Wasm instance.
        // It is required because Wasmer expects Env to respect a static lifetime annotation.
        // IMPORTANT: Any code change that violates the assumption could lead to undefined behavior, take care!
        let env_static: &env::Env<'static, S, V> =
            unsafe { transmute::<&env::Env<'a, S, V>, &env::Env<'static, S, V>>(&environment) };

        // Now `env_static` can be used with `create_importable_view` or other functions
        // expecting a `'static` lifetime.
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

        // cast Env back to the original lifetime after use
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
