/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of executing [Account Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#account-commands).

use std::{
    ops::{Deref, DerefMut}, 
    sync::{Arc, Mutex}
};
use pchain_world_state::{storage::WorldStateStorage};
use pchain_types::{cryptography::PublicAddress, blockchain::Command, runtime::{TransferInput, DeployInput, CallInput}};

use crate::{
    transition::{StateChangesResult}, 
    contract::{self, ModuleBuildError, ContractValidateError}, 
    TransitionError, 
    types::{CallTx, BaseTx}
};

use super::{
    phase::{self},
    execute::TryExecuteResult, contract::{ContractInstance, ContractModule},
    state::ExecutionState
};

/// Execution Logic for Account Commands. Err If the Command is not Account Command.
pub(crate) fn try_execute<S>(
    state: ExecutionState<S>, 
    command: &Command
) -> TryExecuteResult<S>
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    
    let ret = match command {
        Command::Transfer(TransferInput { recipient, amount })=>
            transfer(state, *recipient, *amount),
        Command::Deploy(DeployInput { contract, cbi_version }) =>
            deploy(state, contract.to_vec(), *cbi_version),
        Command::Call(CallInput{ target, method, arguments, amount }) =>
            call(state, false, *target, method.clone(),arguments.clone(), *amount),
        _=> return TryExecuteResult::Err(state)
    };

    TryExecuteResult::Ok(ret)
}

/// Execution of [pchain_types::blockchain::Command::Transfer]
pub(crate) fn transfer<S>(
    mut state: ExecutionState<S>, 
    recipient: PublicAddress,
    amount: u64,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone + 'static
{
    let signer = state.tx.signer;

    // Check Balance
    let (origin_balance, _) = state.balance(signer);
    if origin_balance < amount {
        return Err(phase::abort(state, TransitionError::NotEnoughBalanceForTransfer));
    }

    // Transfer Balance
    state.set_balance(signer, origin_balance - amount);
    let (recipient_balance, _) = state.balance(recipient);
    state.set_balance(recipient, recipient_balance + amount);

    phase::finalize_gas_consumption(state)
}

/// Execution of [pchain_types::blockchain::Command::Call]
pub(crate) fn call <S>(
    mut state: ExecutionState<S>,
    is_view: bool,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
    amount: Option<u64>,
) -> Result<ExecutionState<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{

    if let Some(amount) = amount {
        let signer = state.tx.signer;

        // Check Balance
        let (origin_balance, _) = state.balance(signer);
        if origin_balance < amount {
            return Err(phase::abort(state, TransitionError::NotEnoughBalanceForTransfer))
        }
        
        // Transfer Balance
        state.set_balance(signer, origin_balance - amount);
        let (target_balance, _) = state.balance(target);
        state.set_balance(target, target_balance + amount);
    }

    // Instantiation of contract
    let instance = CallInstance::instantiate(state, is_view, target, method, arguments, amount)
        .map_err(|(state, transition_err)| phase::abort(state, transition_err) )?;
    
    // Call the contract
    let (state,transition_err) = instance.call();

    match transition_err {
        Some(err) => Err(phase::abort(state, err)),
        None => phase::finalize_gas_consumption(state)
    }
}

/// CallInstance defines the steps of contract instantiation and contract call.
struct CallInstance<S>
where S: WorldStateStorage + Send + Sync + Clone + 'static { 
    state: ExecutionState<S>,
    instance: ContractInstance<S>,
}

impl<S> CallInstance<S>
where S: WorldStateStorage + Send + Sync + Clone + 'static  {

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
        where S: WorldStateStorage + Send + Sync + Clone + 'static
    {
        // check CBI version is None
        match state.cbi_version(target) {
            (Some(version), _) if !contract::is_cbi_compatible(version) => return Err((state, TransitionError::InvalidCBI)),
            (None, _) => return Err((state, TransitionError::InvalidCBI)),
            _=>{}
        }
        
        // ONLY load contract after checking CBI version. (To ensure the loaded contract is deployed SUCCESSFULLY, 
        // otherwise, it is possible to load the cached contract in previous transaction)
        let module = match ContractModule::build_contract(
            target,
            &state.sc_context,
            &state.rw_set,
        ) {
            Ok(module) => module,
            Err(_) => {
                return Err((state, TransitionError::NoContractcode))
            }
        };

        // Check pay for storage gas cost at this point. Consider it as runtime cost because the world state write is an execution gas
        // Gas limit for init method call should be subtract the blockchain and worldstate storage cost
        let pre_execution_baseline_gas_limit = state.total_gas_to_be_consumed();
        if state.tx.gas_limit < pre_execution_baseline_gas_limit {
            return Err((state, TransitionError::ExecutionProperGasExhausted))
        }
        let gas_limit_for_execution = state.tx.gas_limit.saturating_sub(pre_execution_baseline_gas_limit);

        let call_tx = CallTx {
            base_tx: BaseTx {
                gas_limit: gas_limit_for_execution,
                ..state.tx
            },
            amount,
            arguments,
            method,
            target
        };

        let instance = match module.instantiate(
            Arc::new(Mutex::new(state.ctx.clone())),
            0,
            is_view,
            call_tx,
            state.bd.clone()
        ) {
            Ok(ret) => ret,
            Err(_) => return Err((state, TransitionError::CannotCompile))
        };

        Ok(Self { state, instance })
    }

    /// Call the Instance and transits the state.
    fn call(self) -> (ExecutionState<S>, Option<TransitionError>) {
        let (ctx, wasm_exec_gas, call_error) = self.instance.call();
        let mut state = ExecutionState { 
            ctx,
            ..self.state
        };
        // gas for execution is already consumed
        let gas_consumed = state.gas_consumed();
        state.set_gas_consumed(gas_consumed.saturating_add(wasm_exec_gas));

        let transition_err =
        if state.tx.gas_limit < state.total_gas_to_be_consumed() {
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
) -> Result<ExecutionState<S>, StateChangesResult<S>>
    where S: WorldStateStorage + Send + Sync + Clone
{
    let contract_address = pchain_types::cryptography::sha256([state.tx.signer.to_vec(), state.tx.nonce.to_le_bytes().to_vec()].concat());
    
    // Instantiate instant to preform contract deployment.
    let instance = DeployInstance::instantiate(state, contract, cbi_version, contract_address)
        .map_err(|(state, err)| phase::abort(state, err.into()))?;

    // Deploy the contract
    let state = instance.deploy()
        .map_err(|(state, err)| phase::abort(state, err.into()))?;

    phase::finalize_gas_consumption(state)
}

/// DeployInstance defines the steps of contract instantiation and contract deploy.
struct DeployInstance<S>
where S: WorldStateStorage + Send + Sync + Clone + 'static { 
    state: ExecutionState<S>,
    module: ContractModule,
    contract_address: PublicAddress,
    contract: Vec<u8>,
    cbi_version: u32,
}

impl<S> DeployInstance<S>
where S: WorldStateStorage + Send + Sync + Clone + 'static  {

    /// Instantiate an instance after contract validation
    fn instantiate(
        state: ExecutionState<S>,
        contract: Vec<u8>,
        cbi_version: u32,
        contract_address: PublicAddress
    ) -> Result<Self, (ExecutionState<S>, DeployError)> {

        if !contract::is_cbi_compatible(cbi_version) {
            return Err((state, DeployError::InvalidDeployTransactionData))
        }

        // Check if CBIVersion is already set for this address
        let (exist_cbi_version, _) = state.cbi_version(contract_address);
        if exist_cbi_version.is_some() {
            return Err((state, DeployError::CBIVersionAlreadySet))
        }
        
        let module = match ContractModule::new(&contract, state.sc_context.memory_limit) {
            Ok(module) => module,
            Err(err) => return Err((state, DeployError::ModuleBuildError(err)))
        };

        if let Err(err) = module.validate() {
            return Err((state, DeployError::ContractValidateError(err)))
        };

        Ok(Self {
            state,
            module,
            contract,
            cbi_version,
            contract_address
        })
    }

    /// Deploy by writing contract to storage and transit to the state.
    fn deploy(mut self) -> Result<ExecutionState<S>, (ExecutionState<S>, DeployError)> {
        let contract_address = self.contract_address;
        
        // cache the module
        if let Some(sc_cache) = &self.sc_context.cache {
            self.module.cache(contract_address, &mut sc_cache.clone());
        }

        // Write contract code with CBI version.
        let contract_code = self.contract.clone();
        let cbi_version = self.cbi_version;
        self.set_code(contract_address, contract_code);
        self.set_cbi_version(contract_address, cbi_version);

        if self.tx.gas_limit < self.total_gas_to_be_consumed() {
            return Err((self.state, DeployError::InsufficientGasForInitialWritesError))
        }

        Ok(self.state)
    }

}

impl<S> Deref for DeployInstance<S>
where S: WorldStateStorage + Send + Sync + Clone {
    type Target = ExecutionState<S>;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<S> DerefMut for DeployInstance<S>
where S: WorldStateStorage + Send + Sync + Clone {
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
            DeployError::ModuleBuildError(err) => {
                match err {
                    ModuleBuildError::DisallowedOpcodePresent => TransitionError::DisallowedOpcode,
                    ModuleBuildError::Else => TransitionError::CannotCompile,
                }
            },
            DeployError::ContractValidateError(err) => {
                match err {
                    ContractValidateError::MethodNotFound => TransitionError::NoExportedContractMethod,
                    ContractValidateError::InstantiateError => TransitionError::CannotCompile,
                }
            },
            DeployError::InsufficientGasForInitialWritesError => TransitionError::ExecutionProperGasExhausted,
            DeployError::InvalidDeployTransactionData => TransitionError::OtherDeployError,
            DeployError::CBIVersionAlreadySet => TransitionError::ContractAlreadyExists
        }
    }
}