/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines structs for contract instantiation and contract call which are used in executing Commands Phase.

use pchain_world_state::storage::WorldStateStorage;

use crate::{
    contract::wasmer::{
        env,
        instance::{Instance, MethodCallError},
    },
    transition::TransitionContext,
};

/// ContractInstance contains contract instance which is prepared to be called in Commands Phase.
pub(crate) struct ContractInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    pub(in crate::contract) environment: env::Env<S>,
    pub(in crate::contract) instance: Instance,
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
        let ctx = self.environment.context.lock().unwrap().clone(); // TODO better to take out from Arc Mutext rather than clone
        (ctx, total_gas, call_error)
    }
}
