/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct containing [wasmer::Instance] to execute a method call according to ParallelChain Smart Contract Defintions.

use anyhow::Result;

/// Instance represents a stateful instance of a WebAssembly module (quasi-process) and ready for contract method execution
/// through its callable function.
pub(in crate::contract) struct Instance(pub(crate) wasmer::Instance);

impl Instance {
    /// call_method executes the named method of the Instance
    ///
    /// If the call completes successfully, it returns the remaining gas after the execution. If the call terminated early,
    /// it returns a two-tuple comprising the remaining gas after the execution, and a MethodCallError describing the
    /// cause of the early termination.
    ///
    /// # Panics
    /// call_method assumes that the Instance does export the name method, and panics otherwise.
    pub(crate) unsafe fn call_method(&self) -> Result<u64, (u64, MethodCallError)> {
        let remaining_gas = match wasmer_middlewares::metering::get_remaining_points(&self.0) {
            wasmer_middlewares::metering::MeteringPoints::Exhausted => 0,
            wasmer_middlewares::metering::MeteringPoints::Remaining(gas_left_after_execution) => {
                gas_left_after_execution
            }
        };

        let method = match self
            .0
            .exports
            .get_native_function::<(), ()>(CONTRACT_METHOD)
        {
            Ok(m) => m,
            Err(e) => return Err((remaining_gas, MethodCallError::NoExportedMethod(e))), // Invariant violated: A contract that does not export method_name was deployed.
        };

        // method call
        let execution_result = method.call();

        // use the Wasmer provided method to access the gas global variable
        let remaining_gas = match wasmer_middlewares::metering::get_remaining_points(&self.0) {
            wasmer_middlewares::metering::MeteringPoints::Exhausted => 0,
            wasmer_middlewares::metering::MeteringPoints::Remaining(gas_left_after_execution) => {
                gas_left_after_execution
            }
        };

        match execution_result{
            Ok(_) => Ok(remaining_gas),
            Err(_) if remaining_gas == 0 => Err((remaining_gas, MethodCallError::GasExhaustion)),
            Err(e) /* remaining_gas > 0 */ => Err((remaining_gas, MethodCallError::Runtime(e)))
        }
    }

    /// return a global variable which can read and modify the metering remaining points of wasm execution of this Instance
    pub(crate) fn remaining_points(&self) -> wasmer::Global {
        self.0
            .exports
            .get_global("wasmer_metering_remaining_points")
            .unwrap()
            .clone()
    }
}

/// MethodCallError enumerates through the possible reasons why a call into a contract Instance's exported methods might
/// terminate early.
#[derive(Debug)]
pub enum MethodCallError {
    Runtime(wasmer::RuntimeError),
    GasExhaustion,
    NoExportedMethod(wasmer::ExportError),
}

/// ContractValidateError enumerates through the possible reasons why the contract is not runnable
#[derive(Debug)]
pub enum ContractValidateError {
    MethodNotFound,
    InstantiateError,
}

/// CONTRACT_METHOD is reserved by the ParallelChain Mainnet protocol to name callable function
/// exports from smart contract Modules.  
pub const CONTRACT_METHOD: &str = "entrypoint";
