/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct to use [wasmer::Module] as underlying WASM module to work with compiled contract bytecode in Parallelchain Mainnet.

use pchain_types::cryptography::PublicAddress;

use crate::contract::wasmer::cache::{Cache as SmartContractCache, ModuleMetadata};
use crate::contract::{blank, Importable};

use super::instance::{ContractValidateError, Instance, CONTRACT_METHOD};

/// Module is a structure representing a WebAssembly executable that has been compiled down to architecture-specific
/// machine code in preparation for execution.
pub(in crate::contract) struct Module(pub wasmer::Module, pub ModuleMetadata);

impl Module {
    /// from_cache returns the contract Module cached in smart contract cache.
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

    /// cache_to caches contract module to smart contract cache.
    pub fn cache_to(&self, address: PublicAddress, cache: &SmartContractCache) {
        let _ = cache.store(address, &self.0, self.1.bytes_length);
    }

    /// from_wasm_bytecode returns the contract Module produced by compiling the wasm bytecode provided as an argument.
    /// The bytecode will be validated and the process is slow. Err `ModuleBuildError::DisallowedOpcodePresent` could be returned.
    pub fn from_wasm_bytecode(
        cbi_version: u32,
        bytecode: &Vec<u8>,
        wasmer_store: &wasmer::Store,
    ) -> Result<Module, ModuleBuildError> {
        let wasmer_module = match wasmer::Module::from_binary(wasmer_store, bytecode) {
            Ok(m) => m,
            Err(e) => {
                if e.to_string().contains("OpcodeError") {
                    return Err(ModuleBuildError::DisallowedOpcodePresent);
                }
                return Err(ModuleBuildError::Else);
            }
        };

        Ok(Module(
            wasmer_module,
            ModuleMetadata {
                cbi_version,
                bytes_length: bytecode.len(),
            },
        ))
    }

    /// from_wasm_bytecode_unchecked returns the contract Module produced by compiling the wasm bytecode provided as an argument.
    /// The bytecode will NOT be validated. Use method `from_wasm_bytecode` if the bytecode should be validated. Err
    /// `ModuleBuildError::DisallowedOpcodePresent` could be returned.
    pub fn from_wasm_bytecode_unchecked(
        cbi_version: u32,
        bytecode: &Vec<u8>,
        wasmer_store: &wasmer::Store,
    ) -> Result<Module, ModuleBuildError> {
        let wasmer_module =
            match unsafe { wasmer::Module::from_binary_unchecked(wasmer_store, bytecode) } {
                Ok(m) => m,
                Err(e) => {
                    if e.to_string().contains("OpcodeError") {
                        return Err(ModuleBuildError::DisallowedOpcodePresent);
                    }
                    return Err(ModuleBuildError::Else);
                }
            };

        Ok(Module(
            wasmer_module,
            ModuleMetadata {
                cbi_version,
                bytes_length: bytecode.len(),
            },
        ))
    }

    /// size of the wasm bytecode
    pub fn bytes_length(&self) -> usize {
        self.1.bytes_length
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

    /// validate_contract returns whether this contract Module exports a function with the name METHOD_ACTIONS
    /// and can be instantiated with calls() function.
    pub fn validate_contract(
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

/// ModuleBuildError enumerates the possible reasons why arbitrary bytecode might fail to be interpreted as WASM and compiled
/// down to machine code in preparation for execution.
#[derive(Debug)]
pub(crate) enum ModuleBuildError {
    /// Contract contains opcodes what are not allowed.
    DisallowedOpcodePresent,
    /// Errors other than `DisallowedOpcodePresent`
    Else,
}
