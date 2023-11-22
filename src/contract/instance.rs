/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines struct which holds the contract Wasm instance and Env metadata
//! and exposes an entry point for calling contract methods
//!
use pchain_world_state::{VersionProvider, DB};

use crate::{
    contract::wasmer::{
        env,
        instance::{Instance, MethodCallError},
    },
    transition::TransitionContext,
};

/// ContractInstance contains contract instance which is prepared to be called in Commands Phase.
pub(crate) struct ContractInstance<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    pub(in crate::contract) environment: env::Env<'a, S, V>,
    pub(in crate::contract) instance: Instance,
}

impl<'a, S, V> ContractInstance<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    pub(crate) fn call(self) -> (TransitionContext<'a, S, V>, u64, Option<MethodCallError>) {
        // initialize the variable of wasmer remaining gas
        self.environment
            .init_wasmer_remaining_points(self.instance.remaining_points());

        // Invoke Wasm Execution
        let call_result = unsafe { self.instance.call_method() };

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
            .saturating_sub(remaining_gas);

        // Get the updated TransitionContext

        // TODO 95
        // premise, we don't want to clone this on exit
        // how, though, was it initially passed in?
        let ctx = self.environment.context.lock().unwrap().clone();
        (ctx, total_gas, call_error)
    }
}
