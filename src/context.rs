/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Represents an intermediary providing access to the [World State](pchain_world_state) and components that can mutate it.
//!
//! As a singleton instance encapsulating the World State, only a single 'valid' instance of
//! [TransitionContext] can exist at any given time to maintain state integrity.
//! It forms an integral part of larger constructs, such as the [ExecutionState](crate::execution::state::ExecutionState).
//!
//! When used during [contract](crate::contract) execution, the context also maintains intermediate metadata such as
//! deferred commands generated during execution and metadata of the contract instance.
use pchain_world_state::{VersionProvider, WorldState, DB};

use crate::{
    contract::SmartContractContext,
    execution::cache::WorldStateCache,
    gas::GasMeter,
    types::{CommandOutput, DeferredCommand, TxnVersion},
};

/// TransitionContext encapsulates the World State via [GasMeter](crate::gas::GasMeter),
/// and when used during smart contract execution,
/// stores the relevant contract sub-context and holds deferred commands pending execution.
#[derive(Clone)]
pub(crate) struct TransitionContext<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Smart contract context for execution
    pub sc_context: SmartContractContext,

    /// Queue of commands that were deferred from an original Call command
    /// during the execution of a smart contract.
    pub deferred_commands: Vec<DeferredCommand>,

    /// GasMeter for the transaction, encapsulates World State access and gas tallying
    pub gas_meter: GasMeter<'a, S, V>,
}

impl<'a, S, V> TransitionContext<'a, S, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// initialize a new Transition Context, at the beginning of a new transaction
    pub fn new(version: TxnVersion, ws: WorldState<'a, S, V>, gas_limit: u64) -> Self {
        let host_gm = GasMeter::new(version, WorldStateCache::new(ws), gas_limit);

        Self {
            sc_context: Default::default(),
            deferred_commands: Vec::new(),
            gas_meter: host_gm,
        }
    }

    /// Add a deferred command to the context.
    pub fn append_deferred_command(&mut self, cmd: DeferredCommand) {
        self.deferred_commands.push(cmd);
    }

    /// Clones smart contract context for nested contract calls
    pub fn clone_smart_contract_context(&self) -> SmartContractContext {
        self.sc_context.clone()
    }

    /// Get the World State Cache which allows read-write without gas metering.
    pub fn gas_free_ws_cache(&self) -> &WorldStateCache<'a, S, V> {
        &self.gas_meter.ws_cache
    }

    /// Get the mutable World State Cache which allows read-write without gas metering.
    pub fn gas_free_ws_cache_mut(&mut self) -> &mut WorldStateCache<'a, S, V> {
        &mut self.gas_meter.ws_cache
    }

    /// Consumes self to output the World State Cache. It can be used when the transition context is
    /// no longer needed (e.g. at the end of transition).
    pub fn into_ws_cache(self) -> WorldStateCache<'a, S, V> {
        self.gas_meter.ws_cache
    }

    /// Discard the changes to world state
    pub fn revert_changes(&mut self) {
        self.gas_meter.ws_cache.revert();
    }

    /// Outputs the CommandReceipt and clears the intermediate context for next command execution.
    // IMPORTANT: This function must be called after each command execution, whether success or fail
    // as all the tallying and state changes happen here.
    pub fn complete_cmd_execution(&mut self) -> (u64, CommandOutput, Option<Vec<DeferredCommand>>) {
        // 1. Take the fields from output cache and update to gas meter at this checkpoint
        let (gas_used, command_output) = self.gas_meter.take_current_command_result();

        // 2. Clear data for next command execution
        let deferred_commands = (!self.deferred_commands.is_empty())
            .then_some(std::mem::take(&mut self.deferred_commands));

        (gas_used, command_output, deferred_commands)
    }
}
