/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Construct to facilitate state management and gas value handling for smart contract calls.
//!
//! The [ContractInstance] struct, which is initialized during the [smart contract call process](crate::commands::account::call),
//! orchestrates [TransitionContext] to ensure accurate state preservation during and after contract execution.
//! While it does not actively manage gas usage, it reads and passes on the value of any remaining gas after execution.

use pchain_world_state::{VersionProvider, DB};

use crate::{
    context::TransitionContext,
    contract::wasmer::{
        env,
        instance::{Instance, MethodCallError},
    },
};

/// ContractInstance holds the active Wasm instance and its associated execution environment (`Env`).
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
        // initialize the variable with the Wasmer global var exposed after contract instantiation
        self.environment
            .init_wasmer_gas_global(self.instance.remaining_points());

        // Invoke Wasm Execution
        let call_result = unsafe { self.instance.call_method() };

        // drop the variable of wasmer remaining gas
        self.environment.drop_wasmer_gas_global();

        let (remaining_gas, call_error) = match call_result {
            Ok(remaining_gas) => (remaining_gas, None),
            Err((remaining_gas, call_error)) => (remaining_gas, Some(call_error)),
        };

        let total_gas = self
            .environment
            .call_tx
            .gas_limit
            .saturating_sub(remaining_gas);

        // After contract execution, retrieve a clone of the updated TransitionContext
        // Note that we cannot take ownership of the Mutex<TransitionContext> within the Arc
        // because this function is invoked from both External and Internal contract calls.
        // In the second scenario, prior calls still hold counted refs to the Mutex.
        // Also, the Wasmer importable also holds refs to the Mutex.
        // This might be refactored in future with a change to Wasmer's API

        let ctx = self.environment.context.lock().unwrap().clone();
        (ctx, total_gas, call_error)
    }
}
