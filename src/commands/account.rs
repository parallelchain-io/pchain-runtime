/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Business logic used by [Execute](crate::execution::execute) trait implementations for
//! [Account Commands](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Runtime.md#protocol-commands).
//!
//! These commands modify state of individual accounts in the World State,
//! affecting elements such as user account balances, or contract account code bytes.
//!
//! They can do so directly, or indirectly by triggering the execution of WebAsembly smart contracts,
//! which in turn hook into the state modification methods of the Wasm host API.

use pchain_types::cryptography::{contract_address_v1, contract_address_v2, PublicAddress};
use pchain_world_state::{VersionProvider, DB};
use std::sync::{Arc, Mutex};

use crate::{
    contract::{
        self, is_cbi_compatible,
        wasmer::{instance::ContractValidateError, module::ModuleBuildError},
        ContractInstance, ContractModule,
    },
    execution::abort::{abort, abort_if_gas_exhausted},
    types::{CallTx, TxnMetadata, TxnVersion},
    TransitionError,
};

use crate::execution::state::ExecutionState;

/* ↓↓↓ Transfer Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::Transfer]
/// Transfers the specified amount of tokens from the signer's account to the recipient's account.
pub(crate) fn transfer<S, E, V>(
    state: &mut ExecutionState<'_, S, E, V>,
    recipient: PublicAddress,
    amount: u64,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    let signer = state.txn_meta.signer;
    let origin_balance = state.ctx.gas_meter.ws_balance(signer);

    if origin_balance < amount {
        abort!(state, TransitionError::NotEnoughBalanceForTransfer)
    }

    // Always deduct the amount specified in the transaction
    state
        .ctx
        .gas_meter
        .ws_set_balance(signer, origin_balance - amount);
    let recipient_balance = state.ctx.gas_meter.ws_balance(recipient);

    // Ceiling to MAX for safety. Overflow should not happen in real situation.
    state
        .ctx
        .gas_meter
        .ws_set_balance(recipient, recipient_balance.saturating_add(amount));

    abort_if_gas_exhausted(state)
}

/* ↓↓↓ Call Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::Call]
/// which invokes specified method of in the target contract
/// with arguments, if any.
/// Optionally, users can transfer a specified amount of tokens to the target contract.
pub(crate) fn call<S, E, V>(
    state: &mut ExecutionState<S, E, V>,
    is_view: bool,
    target: PublicAddress,
    method: String,
    arguments: Option<Vec<Vec<u8>>>,
    amount: Option<u64>,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    if let Some(amount) = amount {
        let signer = state.txn_meta.signer;

        // check balance
        let origin_balance = state.ctx.gas_meter.ws_balance(signer);
        if origin_balance < amount {
            abort!(state, TransitionError::NotEnoughBalanceForTransfer);
        }

        // Always deduct the amount specified in the transaction
        state
            .ctx
            .gas_meter
            .ws_set_balance(signer, origin_balance - amount);
        let target_balance = state.ctx.gas_meter.ws_balance(target);

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

/// CallInstance abstracts the details of contract instantiation and the actual method calling.
struct CallInstance<'a, 'b, S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// a reference to the global ExecutionState
    /// which is needed to bring TransitionContext into the contract execution environment.
    state: &'b mut ExecutionState<'a, S, E, V>,

    /// the specific Wasm instance
    instance: ContractInstance<'a, S, V>,
}

impl<'a, 'b, S, E, V> CallInstance<'a, 'b, S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// Instantiate an instance to be called. It returns transition error for failures in
    /// contract instantiation and verification.
    fn instantiate(
        state: &'b mut ExecutionState<'a, S, E, V>,
        is_view: bool,
        target: PublicAddress,
        method: String,
        arguments: Option<Vec<Vec<u8>>>,
        amount: Option<u64>,
    ) -> Result<Self, TransitionError>
    where
        S: DB + Send + Sync + Clone + 'static,
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        // Check CBI version
        state
            .ctx
            .gas_meter
            .ws_cbi_version(target)
            .filter(|version| contract::is_cbi_compatible(*version))
            .ok_or(TransitionError::InvalidCBI)?;

        // ONLY load contract after checking CBI version. (To ensure the loaded contract is deployed SUCCESSFULLY,
        // otherwise, it is possible to load a previous version of contract code)
        let contract_module = state
            .ctx
            .gas_meter
            .ws_cached_contract(target, &state.ctx.sc_context)
            .ok_or(TransitionError::NoContractcode)?;

        // Check that storage related operations for execution setup have not exceeded gas limit at this point
        let gas_limit_for_execution = state
            .txn_meta
            .gas_limit
            .checked_sub(state.ctx.gas_meter.total_gas_used())
            .ok_or(TransitionError::ExecutionProperGasExhausted)?;

        let call_tx = CallTx {
            base_tx: TxnMetadata {
                command_kinds: state.txn_meta.command_kinds.clone(),
                gas_limit: gas_limit_for_execution,
                ..state.txn_meta
            },
            amount,
            arguments,
            method,
            target,
        };

        let instance = contract_module
            .instantiate(
                // Before contract execution, we need to clone to the TransitionContext and embed it an Arc<Mutex>
                // to fulfil the trait requirements for WasmerEnv (Send + Sync + Clone).
                // Because this function does not own ExecutionState, it cannot pass an owned instance of TransitionContext.
                // This might be refactored in future with a change to Wasmer's API
                Arc::new(Mutex::new(state.ctx.clone())),
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
        self.state.ctx.gas_meter.manually_charge_gas(wasm_exec_gas);
        if self.state.txn_meta.gas_limit < self.state.ctx.gas_meter.total_gas_used() {
            Some(TransitionError::ExecutionProperGasExhausted)
        } else {
            call_error.map(TransitionError::from)
        }
    }
}

/* ↓↓↓ Deploy Command ↓↓↓ */

/// Execution of [pchain_types::blockchain::Command::Deploy]
/// which deploys the specified Wasm byte code to a deterministic contract address.
pub(crate) fn deploy<'a, 'b, S, E, V>(
    state: &'b mut ExecutionState<'a, S, E, V>,
    cmd_index: u32,
    contract: Vec<u8>,
    cbi_version: u32,
) -> Result<(), TransitionError>
where
    S: DB + Send + Sync + Clone,
    V: VersionProvider + Send + Sync + Clone,
{
    // compute the deploy destination, which differs between V1 and V2 transactions
    let contract_address = match state.txn_meta.version {
        TxnVersion::V1 => contract_address_v1(&state.txn_meta.signer, state.txn_meta.nonce),
        TxnVersion::V2 => {
            contract_address_v2(&state.txn_meta.signer, state.txn_meta.nonce, cmd_index)
        }
    };

    let instance = match DeployInstance::instantiate(state, contract, cbi_version, contract_address)
    {
        Ok(instance) => instance,
        Err(err) => abort!(state, err),
    };

    match instance.deploy() {
        Some(err) => abort!(state, err),
        None => abort_if_gas_exhausted(state),
    }
}

/// DeployInstance abstracts the details of building the WebAssembly module
/// and storing the raw contract bytecode.
struct DeployInstance<'a, 'b, S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// a reference to the global ExecutionState
    /// which provides a handle to TransitionContext
    state: &'b mut ExecutionState<'a, S, E, V>,

    /// compiled WebAssembly module
    module: ContractModule,

    /// calculated contract address
    contract_address: PublicAddress,

    /// user-provided Wasm bytecode
    contract: Vec<u8>,

    /// user-provided CBI version of the the smart contract
    cbi_version: u32,
}

impl<'a, 'b, S, E, V> DeployInstance<'a, 'b, S, E, V>
where
    S: DB + Send + Sync + Clone + 'static,
    V: VersionProvider + Send + Sync + Clone,
{
    /// builds a WebAssembly module from bytecode for validation
    fn instantiate(
        state: &'b mut ExecutionState<'a, S, E, V>,
        bytecode: Vec<u8>,
        cbi_version: u32,
        contract_address: PublicAddress,
    ) -> Result<Self, TransitionError> {
        if !is_cbi_compatible(cbi_version) {
            return Err(TransitionError::OtherDeployError);
        }

        // do not allow previously deployed contracts to be overwritten
        let exist_cbi_version = state.ctx.gas_meter.ws_cbi_version(contract_address);
        if exist_cbi_version.is_some() {
            return Err(TransitionError::ContractAlreadyExists);
        }

        // check if the bytecode can be compiled into a valid Wasm module
        let module =
            ContractModule::from_bytecode_checked(&bytecode, state.ctx.sc_context.memory_limit)
                .map_err(|build_err| match build_err {
                    ModuleBuildError::DisallowedOpcodePresent => TransitionError::DisallowedOpcode,
                    ModuleBuildError::Else => TransitionError::CannotCompile,
                })?;

        // check if the Wasm module is a valid contract according to the ParallelChain Protocol CBI
        module
            .validate_proper_contract()
            .map_err(|validate_err| match validate_err {
                ContractValidateError::MethodNotFound => TransitionError::NoExportedContractMethod,
                ContractValidateError::InstantiateError => TransitionError::CannotCompile,
            })?;

        Ok(Self {
            state,
            module,
            contract: bytecode,
            cbi_version,
            contract_address,
        })
    }

    /// Write contract bytecode to the relevant account in the World State.
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

        (self.state.txn_meta.gas_limit < self.state.ctx.gas_meter.total_gas_used())
            .then_some(TransitionError::ExecutionProperGasExhausted)
    }
}
