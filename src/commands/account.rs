/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of executing [Account Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#account-commands).

use pchain_types::cryptography::{PublicAddress, contract_address_v1, contract_address_v2};
use pchain_world_state::storage::WorldStateStorage;
use std::sync::{Arc, Mutex};

use crate::{
    contract::{
        self,
        wasmer::{instance::ContractValidateError, module::ModuleBuildError},
        ContractInstance, ContractModule, is_cbi_compatible,
    },
    execution::abort::{abort, abort_if_gas_exhausted},
    types::{BaseTx, CallTx, TxnVersion},
    TransitionError,
};

use crate::execution::state::ExecutionState;

/// Execution of [pchain_types::blockchain::Command::Transfer]
pub(crate) fn transfer<S, E>(
    state: &mut ExecutionState<S, E>,
    recipient: PublicAddress,
    amount: u64,
) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    let signer = state.tx.signer;
    let origin_balance = state.ctx.gas_meter.ws_get_balance(signer);

    if origin_balance < amount {
        abort!(state, TransitionError::NotEnoughBalanceForTransfer)
    }

    // Always deduct the amount specified in the transaction
    state
        .ctx
        .gas_meter
        .ws_set_balance(signer, origin_balance - amount);
    let recipient_balance = state.ctx.gas_meter.ws_get_balance(recipient);

    // Ceiling to MAX for safety. Overflow should not happen in real situation.
    state
        .ctx
        .gas_meter
        .ws_set_balance(recipient, recipient_balance.saturating_add(amount));

    abort_if_gas_exhausted(state)
}

/// Execution of [pchain_types::blockchain::Command::Call]
pub(crate) fn call<S, E>(
    state: &mut ExecutionState<S, E>,
    is_view: bool,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
    amount: Option<u64>,
) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    if let Some(amount) = amount {
        let signer = state.tx.signer;

        // check balance
        let origin_balance = state.ctx.gas_meter.ws_get_balance(signer);
        if origin_balance < amount {
            abort!(state, TransitionError::NotEnoughBalanceForTransfer);
        }

        // Always deduct the amount specified in the transaction
        state
            .ctx
            .gas_meter
            .ws_set_balance(signer, origin_balance - amount);
        let target_balance = state.ctx.gas_meter.ws_get_balance(target);

        // Ceiling to MAX for safety. Overflow should not happen in real situation.
        state
            .ctx
            .gas_meter
            .ws_set_balance(target, target_balance.saturating_add(amount));
    }

    // Instantiation of contract
    let instance =
        match CallInstance::instantiate(state, is_view, target, method, arguments, amount) {
            Ok(instance) => instance,
            Err(transition_err) => abort!(state, transition_err),
        };

    // Call the contract
    match instance.call() {
        Some(err) => abort!(state, err),
        None => abort_if_gas_exhausted(state),
    }
}

/// CallInstance defines the steps of contract instantiation and contract call.
struct CallInstance<'a, S, E>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    state: &'a mut ExecutionState<S, E>,
    instance: ContractInstance<S>,
}

impl<'a, S, E> CallInstance<'a, S, E>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Instantiate an instant to be called. It returns transition error for failures in
    /// contrac tinstantiation and verification.
    fn instantiate(
        state: &'a mut ExecutionState<S, E>,
        is_view: bool,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>,
        amount: Option<u64>,
    ) -> Result<Self, TransitionError>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static,
    {
        // Check CBI version
        state
            .ctx
            .gas_meter
            .ws_get_cbi_version(target)
            .filter(|version| contract::is_cbi_compatible(*version))
            .ok_or(TransitionError::InvalidCBI)?;

        // ONLY load contract after checking CBI version. (To ensure the loaded contract is deployed SUCCESSFULLY,
        // otherwise, it is possible to load the cached contract in previous transaction)
        let contract_module = state
            .ctx
            .gas_meter
            .ws_get_cached_contract(target, &state.ctx.sc_context)
            .ok_or(TransitionError::NoContractcode)?;

        // Check pay for storage gas cost at this point. Consider it as runtime cost because the world state write is an execution gas
        // Gas limit for init method call should be subtract the blockchain and worldstate storage cost
        let gas_limit_for_execution = state
            .tx
            .gas_limit
            .checked_sub(state.ctx.gas_meter.total_gas_used())
            .ok_or(TransitionError::ExecutionProperGasExhausted)?;

        let call_tx = CallTx {
            base_tx: BaseTx {
                command_kinds: state.tx.command_kinds.clone(),
                gas_limit: gas_limit_for_execution,
                ..state.tx
            },
            amount,
            arguments,
            method,
            target,
        };

        let instance = contract_module.instantiate(
            Arc::new(Mutex::new(state.ctx.clone())), // TODO avoid clone
            0,
            is_view,
            call_tx,
            state.bd.clone(),
        )
        .map_err(|_| TransitionError::CannotCompile)?;

        Ok(Self { state, instance })
    }

    /// Call the Instance and transits the state.
    fn call(self) -> Option<TransitionError> {
        let (ctx, wasm_exec_gas, call_error) = self.instance.call();
        self.state.ctx = ctx;
        self.state.ctx.gas_meter.reduce_gas(wasm_exec_gas);
        if self.state.tx.gas_limit < self.state.ctx.gas_meter.total_gas_used() {
            Some(TransitionError::ExecutionProperGasExhausted)
        } else {
            call_error.map(TransitionError::from)
        }
    }
}

/// Execution of [pchain_types::blockchain::Command::Deploy]
pub(crate) fn deploy<S, E>(
    state: &mut ExecutionState<S, E>,
    cmd_index: u32,
    contract: Vec<u8>,
    cbi_version: u32,
) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let contract_address = match state.tx.version {
        TxnVersion::V1 => contract_address_v1(&state.tx.signer, state.tx.nonce),
        TxnVersion::V2 => contract_address_v2(&state.tx.signer, state.tx.nonce, cmd_index)
    };

    // Instantiate instant to preform contract deployment.
    let instance = match DeployInstance::instantiate(state, contract, cbi_version, contract_address) {
        Ok(instance) => instance,
        Err(err) => abort!(state, err),
    };

    // Deploy the contract
    match instance.deploy() {
        Some(err) => abort!(state, err),
        None => abort_if_gas_exhausted(state),
    }
}

/// DeployInstance defines the steps of contract instantiation and contract deploy.
struct DeployInstance<'a, S, E>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    state: &'a mut ExecutionState<S, E>,
    module: ContractModule,
    contract_address: PublicAddress,
    contract: Vec<u8>,
    cbi_version: u32,
}

impl<'a, S, E> DeployInstance<'a, S, E>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Instantiate an instance after contract validation
    fn instantiate(
        state: &'a mut ExecutionState<S, E>,
        contract: Vec<u8>,
        cbi_version: u32,
        contract_address: PublicAddress,
    ) -> Result<Self, TransitionError> {
        if !is_cbi_compatible(cbi_version) {
            return Err(TransitionError::OtherDeployError);
        }

        let exist_cbi_version = state.ctx.gas_meter.ws_get_cbi_version(contract_address);
        if exist_cbi_version.is_some() {
            return Err(TransitionError::ContractAlreadyExists);
        }

        let module = ContractModule::from_contract_code(
            &contract,
            state.ctx.sc_context.memory_limit,
        ).map_err(|build_err|
            match build_err {
                ModuleBuildError::DisallowedOpcodePresent => TransitionError::DisallowedOpcode,
                ModuleBuildError::Else => TransitionError::CannotCompile,
            }
        )?;

        module.validate().map_err(|validate_err|
            match validate_err {
                ContractValidateError::MethodNotFound => TransitionError::NoExportedContractMethod,
                ContractValidateError::InstantiateError => TransitionError::CannotCompile
            }
        )?;

        Ok(Self {
            state,
            module,
            contract,
            cbi_version,
            contract_address,
        })
    }

    /// Deploy by writing contract to storage and transit to the state.
    fn deploy(self) -> Option<TransitionError> {
        let contract_address = self.contract_address;

        // cache the module
        if let Some(sc_cache) = &self.state.ctx.sc_context.cache {
            self.module.cache(contract_address, sc_cache);
        }

        // Write contract code with CBI version.
        let contract_code = self.contract.clone();
        let cbi_version = self.cbi_version;

        let ctx = &mut self.state.ctx;
        ctx.gas_meter.ws_set_code(contract_address, contract_code);
        ctx.gas_meter
            .ws_set_cbi_version(contract_address, cbi_version);

        (self.state.tx.gas_limit < self.state.ctx.gas_meter.total_gas_used())
            .then_some(TransitionError::ExecutionProperGasExhausted)
    }
}