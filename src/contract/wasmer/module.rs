/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! A wrapper over [wasmer::Module] to represent a compiled smart contract instance Parallelchain Mainnet.

use pchain_types::cryptography::PublicAddress;

use crate::contract::wasmer::cache::{Cache as SmartContractCache, ModuleMetadata};
use crate::contract::{blank, Importable};

use super::instance::{ContractValidateError, Instance, CONTRACT_METHOD};

/// Module is a struct representing a WebAssembly executable that has been compiled down to architecture-specific
/// machine code in preparation for execution, tagged with metadata.
pub(in crate::contract) struct Module(pub wasmer::Module, pub ModuleMetadata);

impl Module {
    /// returns the contract module cached in smart contract cache
    pub fn from_cache(
        address: PublicAddress,
        cache: &SmartContractCache,
        wasmer_store: &wasmer::Store,
    ) -> Option<Module> {
        cache
            .load(address, wasmer_store)
            .ok()
            .map(|(m, d)| Module(m, d))
    }

    /// caches the contract module
    pub fn cache_to(&self, address: PublicAddress, cache: &SmartContractCache) {
        let _ = cache.store(address, &self.0, self.1.bytecode_length);
    }

    /// compiles bytecode with validation, potentially slow
    pub fn from_wasm_bytecode_checked(
        cbi_version: u32,
        bytecode: &Vec<u8>,
        wasmer_store: &wasmer::Store,
    ) -> Result<Module, ModuleBuildError> {
        let wasmer_module = wasmer::Module::from_binary(wasmer_store, bytecode).map_err(|e| {
            if e.to_string().contains("OpcodeError") {
                ModuleBuildError::DisallowedOpcodePresent
            } else {
                ModuleBuildError::Else
            }
        })?;

        Ok(Module(
            wasmer_module,
            ModuleMetadata {
                cbi_version,
                bytecode_length: bytecode.len(),
            },
        ))
    }

    /// compiles bytecode without validation
    /// use this function only when you are sure that the bytecode has been previously validated
    pub fn from_wasm_bytecode_unchecked(
        cbi_version: u32,
        bytecode: &Vec<u8>,
        wasmer_store: &wasmer::Store,
    ) -> Result<Module, ModuleBuildError> {
        let wasmer_module =
            unsafe { wasmer::Module::from_binary_unchecked(wasmer_store, bytecode) }.map_err(
                |e| {
                    if e.to_string().contains("OpcodeError") {
                        ModuleBuildError::DisallowedOpcodePresent
                    } else {
                        ModuleBuildError::Else
                    }
                },
            )?;

        Ok(Module(
            wasmer_module,
            ModuleMetadata {
                cbi_version,
                bytecode_length: bytecode.len(),
            },
        ))
    }

    /// returns size of the wasm bytecode as recorded in the Module's metadata
    pub fn bytecode_length(&self) -> usize {
        self.1.bytecode_length
    }

    /// instantiate creates a new instance of this contract Module.
    #[allow(clippy::result_large_err)]
    pub fn instantiate(
        &self,
        importable: &Importable,
        gas_limit: u64,
    ) -> Result<Instance, wasmer::InstantiationError> {
        // instantiate wasmer::Instance
        let wasmer_instance = wasmer::Instance::new(&self.0, &importable.0)?;
        // Set the remaining points from metering middleware to wasmer environment
        wasmer_middlewares::metering::set_remaining_points(&wasmer_instance, gas_limit);
        Ok(Instance(wasmer_instance))
    }

    /// returns whether this contract Module
    /// exports a correctly named entry point method which can be invoked by the call() function.
    pub fn validate_entry_point(
        &self,
        wasmer_store: &wasmer::Store,
    ) -> Result<(), ContractValidateError> {
        if !self
            .0
            .exports()
            .functions()
            .any(|f| f.name() == CONTRACT_METHOD)
        {
            return Err(ContractValidateError::MethodNotFound);
        }
        let imports_object = blank::imports(wasmer_store);
        if let Ok(instance) = wasmer::Instance::new(&self.0, &imports_object) {
            if instance
                .exports
                .get_native_function::<(), ()>(CONTRACT_METHOD)
                .is_ok()
            {
                return Ok(());
            }
        }
        Err(ContractValidateError::InstantiateError)
    }
}

/// ModuleBuildError enumerates the possible reasons why arbitrary bytecode might fail to be interpreted as Wasm and compiled
/// down to machine code in preparation for execution.
#[derive(Debug)]
pub(crate) enum ModuleBuildError {
    /// Contract contains opcodes what are not allowed.
    DisallowedOpcodePresent,
    /// Errors other than `DisallowedOpcodePresent`
    Else,
}
