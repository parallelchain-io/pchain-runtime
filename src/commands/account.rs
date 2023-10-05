/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of executing [Account Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#account-commands).

use pchain_types::cryptography::PublicAddress;
use pchain_world_state::storage::WorldStateStorage;
use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
};

use crate::{
    contract::{
        self,
        wasmer::{instance::ContractValidateError, module::ModuleBuildError},
        ContractInstance, ContractModule,
    },
    execution::abort::{abort, abort_if_gas_exhausted, AbortResult},
    types::{BaseTx, CallTx},
    TransitionError,
};

use crate::execution::state::ExecutionState;

/// Execution of [pchain_types::blockchain::Command::Transfer]
pub(crate) fn transfer<S>(
    mut state: ExecutionState<S>,
    recipient: PublicAddress,
    amount: u64,
) -> Result<ExecutionState<S>, AbortResult<S>>
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
    mut state: ExecutionState<S>,
    is_view: bool,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
    amount: Option<u64>,
) -> Result<ExecutionState<S>, AbortResult<S>>
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
    let instance = CallInstance::instantiate(state, is_view, target, method, arguments, amount)
        .map_err(|(state, transition_err)| abort(state, transition_err))?;

    // Call the contract
    let (state, transition_err) = instance.call();

    match transition_err {
        Some(err) => Err(abort(state, err)),
        None => abort_if_gas_exhausted(state),
    }
}

/// CallInstance defines the steps of contract instantiation and contract call.
struct CallInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    state: ExecutionState<S>,
    instance: ContractInstance<S>,
}

impl<S> CallInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Instantiate an instant to be called. It returns transition error for failures in
    /// contrac tinstantiation and verification.
    fn instantiate(
        state: ExecutionState<S>,
        is_view: bool,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>,
        amount: Option<u64>,
    ) -> Result<Self, (ExecutionState<S>, TransitionError)>
    where
        S: WorldStateStorage + Send + Sync + Clone + 'static,
    {
        // check CBI version is None
        let cbi_ver = state.ctx.gas_meter.ws_get_cbi_version(target);
        match cbi_ver {
            Some(version) if !contract::is_cbi_compatible(version) => {
                return Err((state, TransitionError::InvalidCBI))
            }
            None => return Err((state, TransitionError::InvalidCBI)),
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
            None => return Err((state, TransitionError::NoContractcode)),
        };

        // Check pay for storage gas cost at this point. Consider it as runtime cost because the world state write is an execution gas
        // Gas limit for init method call should be subtract the blockchain and worldstate storage cost
        let pre_execution_baseline_gas_limit = state.ctx.gas_meter.total_gas_used();
        if state.tx.gas_limit < pre_execution_baseline_gas_limit {
            return Err((state, TransitionError::ExecutionProperGasExhausted));
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
            Err(_) => return Err((state, TransitionError::CannotCompile)),
        };

        Ok(Self { state, instance })
    }

    /// Call the Instance and transits the state.
    fn call(self) -> (ExecutionState<S>, Option<TransitionError>) {
        let (ctx, wasm_exec_gas, call_error) = self.instance.call();
        let mut state = ExecutionState { ctx, ..self.state };
        state.ctx.gas_meter.reduce_gas(wasm_exec_gas);

        let transition_err = if state.tx.gas_limit < state.ctx.gas_meter.total_gas_used() {
            Some(TransitionError::ExecutionProperGasExhausted)
        } else {
            call_error.map(TransitionError::from)
        };

        (state, transition_err)
    }
}

/// Execution of [pchain_types::blockchain::Command::Deploy]
pub(crate) fn deploy<S>(
    state: ExecutionState<S>,
    contract: Vec<u8>,
    cbi_version: u32,
) -> Result<ExecutionState<S>, AbortResult<S>>
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
    let instance = DeployInstance::instantiate(state, contract, cbi_version, contract_address)
        .map_err(|(state, err)| abort(state, err.into()))?;

    // Deploy the contract
    let state = instance
        .deploy()
        .map_err(|(state, err)| abort(state, err.into()))?;

    abort_if_gas_exhausted(state)
}

/// DeployInstance defines the steps of contract instantiation and contract deploy.
struct DeployInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    state: ExecutionState<S>,
    module: ContractModule,
    contract_address: PublicAddress,
    contract: Vec<u8>,
    cbi_version: u32,
}

impl<S> DeployInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone + 'static,
{
    /// Instantiate an instance after contract validation
    fn instantiate(
        state: ExecutionState<S>,
        contract: Vec<u8>,
        cbi_version: u32,
        contract_address: PublicAddress,
    ) -> Result<Self, (ExecutionState<S>, DeployError)> {
        if !contract::is_cbi_compatible(cbi_version) {
            return Err((state, DeployError::InvalidDeployTransactionData));
        }

        let exist_cbi_version = state.ctx.gas_meter.ws_get_cbi_version(contract_address);
        if exist_cbi_version.is_some() {
            return Err((state, DeployError::CBIVersionAlreadySet));
        }

        let module = match ContractModule::from_contract_code(
            &contract,
            state.ctx.sc_context.memory_limit,
        ) {
            Ok(module) => module,
            Err(err) => return Err((state, DeployError::ModuleBuildError(err))),
        };

        if let Err(err) = module.validate() {
            return Err((state, DeployError::ContractValidateError(err)));
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
    fn deploy(mut self) -> Result<ExecutionState<S>, (ExecutionState<S>, DeployError)> {
        let contract_address = self.contract_address;

        // cache the module
        if let Some(sc_cache) = &self.ctx.sc_context.cache {
            self.module.cache(contract_address, sc_cache);
        }

        // Write contract code with CBI version.
        let contract_code = self.contract.clone();
        let cbi_version = self.cbi_version;

        let ctx = &mut self.state.ctx;
        ctx.gas_meter.ws_set_code(contract_address, contract_code);
        ctx.gas_meter
            .ws_set_cbi_version(contract_address, cbi_version);

        if self.tx.gas_limit < self.ctx.gas_meter.total_gas_used() {
            return Err((
                self.state,
                DeployError::InsufficientGasForInitialWritesError,
            ));
        }

        Ok(self.state)
    }
}

impl<S> Deref for DeployInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    type Target = ExecutionState<S>;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<S> DerefMut for DeployInstance<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
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