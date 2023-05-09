/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! error defines sets of error definitions in entire life time of state transitions. 

use crate::contract::{MethodCallError, FuncError};

/// Descriptive error definitions of a Transition
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransitionError {

    /// Nonce is not current nonce.
    WrongNonce,

    /// Not enough balance to pay for gas limit.
    NotEnoughBalanceForGasLimit,

    /// Not enough balance to pay for transfer.
    NotEnoughBalanceForTransfer,

    /// Gas limit was insufficient to cover pre-execution costs.
    PreExecutionGasExhausted,

    /// The contract bytecode contains disallowed opcodes.
    DisallowedOpcode,

    /// Contract cannot be compiled into machine code (it is probably invalid WASM).
    CannotCompile,

    /// Contract does not export the METHOD_CONTRACT method.
    NoExportedContractMethod,
    
    /// Deployment failed for some other reason.
    OtherDeployError,

    /// Deployment failed because the Contract already exists (CBI version was set for the account)
    ContractAlreadyExists,

    /// Contract cannot be found in state
    NoContractcode,

    /// Fail to load Contract from the CBI
    InvalidCBI,

    /// Gas limit was insufficient to cover execution proper costs.
    ExecutionProperGasExhausted,

    /// Runtime error during execution proper of the entree smart contract.
    RuntimeError,

    /// Gas limit was insufficient to cover execution proper costs of an internal transaction.
    InternalExecutionProperGasExhaustion,

    /// Runtime error during execution proper of an internal transaction.
    InternalRuntimeError,

    /// Network Command - Create Pool fails because the pool already exists
    PoolAlreadyExists,

    /// Network Command fails for non-existing pool
    PoolNotExists,

    /// Network Command - Unstake Deposit fails because the Pool has no stakes.
    PoolHasNoStakes,

    /// Network Command fails because pool policy is invalid.
    /// Scenarios such as
    /// 1. commission fee is greater than 100
    /// 2. commission fee is as same as the origin onw
    InvalidPoolPolicy, 

    /// Network Command - Create Deposits fails because the deposits already exists
    DepositsAlreadyExists,

    /// Network Command fails because the deposits does not exist.
    DepositsNotExists,
    
    /// Network Command - Set Deposit Settings fails because the deposit amount 
    InvalidDepositPolicy, 
    
    /// Network Command fails because the specified amount does not match with the requirement of the operation. 
    /// Scenarios such as
    /// 1. Stake power has already reached upper limit (deposit amount) for Command - Stake Deposit
    /// 2. Stake power is not enough to stay in the delegated stakes for Command - Stake Deposit
    /// 3. Stake power has already reached lower limit for Command - Withdrawal Deposit
    InvalidStakeAmount,

    /// Transaction commands are empty
    InvalidCommands,

    /// There is more than 1 NextEpoch Command in a transaction.
    InvalidNextEpochCommand,
}

impl From<MethodCallError> for TransitionError {
    fn from(call_error: MethodCallError) -> Self {
        match call_error {
            MethodCallError::GasExhaustion => TransitionError::ExecutionProperGasExhausted,
            MethodCallError::NoExportedMethod(_) => TransitionError::RuntimeError,
            MethodCallError::Runtime(e) => {
                // check for internal errors
                match e.downcast::<FuncError>() {
                    Err(_) => TransitionError::RuntimeError,
                    Ok(FuncError::GasExhaustionError) => TransitionError::ExecutionProperGasExhausted,
                    Ok(_) => TransitionError::InternalRuntimeError,
                }
            }
        }
    }
}

impl<'a> From<&'a TransitionError> for pchain_types::ExitStatus {
    fn from(value: &'a TransitionError) -> Self {
        match value {
            TransitionError::ExecutionProperGasExhausted |
            TransitionError::InternalExecutionProperGasExhaustion => pchain_types::ExitStatus::GasExhausted,
            _ => pchain_types::ExitStatus::Failed
        }
    }
}