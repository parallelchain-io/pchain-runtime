/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of executing [Account Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#account-commands).

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::storage::WorldStateStorage;
use std::sync::{Arc, Mutex};

use crate::{
    contract::{
        self,
        wasmer::{instance::ContractValidateError, module::ModuleBuildError},
        ContractInstance, ContractModule,
    },
    execution::abort::{abort, abort_if_gas_exhausted},
    types::{BaseTx, CallTx},
    TransitionError,
};

use crate::execution::state::ExecutionState;

/// Execution of [pchain_types::blockchain::Command::Transfer]
pub(crate) fn transfer<S>(
    state: &mut ExecutionState<S>,
    recipient: PublicAddress,
    amount: u64,
) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    let signer = state.tx.signer;
    let origin_balance = state.ctx.gas_meter.ws_get_balance(signer);

    if origin_balance < amount {
        return Err(abort(state, TransitionError::NotEnoughBalanceForTransfer));
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
pub(crate) fn call<S>(
    state: &mut ExecutionState<S>,
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
            return Err(abort(state, TransitionError::NotEnoughBalanceForTransfer));
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
            Err(transition_err) => return Err(abort(state, transition_err)),
        };

    // Call the contract
    match instance.call() {
        Some(err) => Err(abort(state, err)),
        None => abort_if_gas_exhausted(state),
    }
}

/// CallInstance defines the steps of contract instantiation and contract call.
struct CallInstance<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    state: &'a mut ExecutionState<S>,
    instance: ContractInstance<S>,
}

impl<'a, S> CallInstance<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Instantiate an instant to be called. It returns transition error for failures in
    /// contrac tinstantiation and verification.
    fn instantiate(
        state: &'a mut ExecutionState<S>,
        is_view: bool,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>,
        amount: Option<u64>,
    ) -> Result<Self, TransitionError>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static,
    {
        // check CBI version is None
        let cbi_ver = state.ctx.gas_meter.ws_get_cbi_version(target);
        match cbi_ver {
            Some(version) if !contract::is_cbi_compatible(version) => {
                return Err(TransitionError::InvalidCBI)
            }
            None => return Err(TransitionError::InvalidCBI),
            _ => {}
        }

        // ONLY load contract after checking CBI version. (To ensure the loaded contract is deployed SUCCESSFULLY,
        // otherwise, it is possible to load the cached contract in previous transaction)
        let contract_module = match state
            .ctx
            .gas_meter
            .ws_get_cached_contract(target, &state.ctx.sc_context)
        {
            Some(contract_module) => contract_module,
            None => return Err(TransitionError::NoContractcode),
        };

        // Check pay for storage gas cost at this point. Consider it as runtime cost because the world state write is an execution gas
        // Gas limit for init method call should be subtract the blockchain and worldstate storage cost
        let pre_execution_baseline_gas_limit = state.ctx.gas_meter.total_gas_used();
        if state.tx.gas_limit < pre_execution_baseline_gas_limit {
            return Err(TransitionError::ExecutionProperGasExhausted);
        }
        let gas_limit_for_execution = state
            .tx
            .gas_limit
            .saturating_sub(pre_execution_baseline_gas_limit);

        let call_tx = CallTx {
            base_tx: BaseTx {
                gas_limit: gas_limit_for_execution,
                ..state.tx
            },
            amount,
            arguments,
            method,
            target,
        };

        let instance = match contract_module.instantiate(
            Arc::new(Mutex::new(state.ctx.clone())),
            0,
            is_view,
            call_tx,
            state.bd.clone(),
        ) {
            Ok(ret) => ret,
            Err(_) => return Err(TransitionError::CannotCompile),
        };

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
pub(crate) fn deploy<S>(
    state: &mut ExecutionState<S>,
    contract: Vec<u8>,
    cbi_version: u32,
) -> Result<(), TransitionError>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    let contract_address = pchain_types::cryptography::sha256(
        [
            state.tx.signer.to_vec(),
            state.tx.nonce.to_le_bytes().to_vec(),
        ]
        .concat(),
    );

    // Instantiate instant to preform contract deployment.
    let instance = match DeployInstance::instantiate(state, contract, cbi_version, contract_address)
    {
        Ok(instance) => instance,
        Err(err) => return Err(abort(state, err.into())),
    };

    // Deploy the contract
    match instance.deploy() {
        Ok(()) => abort_if_gas_exhausted(state),
        Err(err) => Err(abort(state, err.into())),
    }
}

/// DeployInstance defines the steps of contract instantiation and contract deploy.
struct DeployInstance<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    state: &'a mut ExecutionState<S>,
    module: ContractModule,
    contract_address: PublicAddress,
    contract: Vec<u8>,
    cbi_version: u32,
}

impl<'a, S> DeployInstance<'a, S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Instantiate an instance after contract validation
    fn instantiate(
        state: &'a mut ExecutionState<S>,
        contract: Vec<u8>,
        cbi_version: u32,
        contract_address: PublicAddress,
    ) -> Result<Self, DeployError> {
        if !contract::is_cbi_compatible(cbi_version) {
            return Err(DeployError::InvalidDeployTransactionData);
        }

        let exist_cbi_version = state.ctx.gas_meter.ws_get_cbi_version(contract_address);
        if exist_cbi_version.is_some() {
            return Err(DeployError::CBIVersionAlreadySet);
        }

        let module = match ContractModule::from_contract_code(
            &contract,
            state.ctx.sc_context.memory_limit,
        ) {
            Ok(module) => module,
            Err(err) => return Err(DeployError::ModuleBuildError(err)),
        };

        if let Err(err) = module.validate() {
            return Err(DeployError::ContractValidateError(err));
        };

        Ok(Self {
            state,
            module,
            contract,
            cbi_version,
            contract_address,
        })
    }

    /// Deploy by writing contract to storage and transit to the state.
    fn deploy(self) -> Result<(), DeployError> {
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

        if self.state.tx.gas_limit < self.state.ctx.gas_meter.total_gas_used() {
            return Err(DeployError::InsufficientGasForInitialWritesError);
        }

        Ok(())
    }
}

/// DeployError is specific to the process inside [DeployInstance]
enum DeployError {
    ModuleBuildError(ModuleBuildError),
    ContractValidateError(ContractValidateError),
    InvalidDeployTransactionData,
    InsufficientGasForInitialWritesError,
    CBIVersionAlreadySet,
}

impl From<DeployError> for TransitionError {
    fn from(error: DeployError) -> Self {
        match error {
            DeployError::ModuleBuildError(err) => match err {
                ModuleBuildError::DisallowedOpcodePresent => TransitionError::DisallowedOpcode,
                ModuleBuildError::Else => TransitionError::CannotCompile,
            },
            DeployError::ContractValidateError(err) => match err {
                ContractValidateError::MethodNotFound => TransitionError::NoExportedContractMethod,
                ContractValidateError::InstantiateError => TransitionError::CannotCompile,
            },
            DeployError::InsufficientGasForInitialWritesError => {
                TransitionError::ExecutionProperGasExhausted
            }
            DeployError::InvalidDeployTransactionData => TransitionError::OtherDeployError,
            DeployError::CBIVersionAlreadySet => TransitionError::ContractAlreadyExists,
        }
    }
}
