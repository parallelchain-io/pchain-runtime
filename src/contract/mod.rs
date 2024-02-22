/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Provides functions and an object-oriented interface for loading,
//! deploying, and executing Wasm smart contracts.
//!
//! A contract consists of a Wasm [module] that contains one or more guest functions.
//! It should match the current [version](mod@cbi_version) of the [Contract Binary Interface](cbi_host_functions)
//! and can use exported functionality provided by the [host](host_functions).
//! The state transition function prepares the execution [context] and builds an [instance] of the contract.

pub mod cbi_host_functions;
pub(crate) use cbi_host_functions::*;

pub mod context;
pub(crate) use context::*;

pub mod host_functions;
pub(crate) use host_functions::*;

pub mod wasmer;

pub mod cbi_version;
pub(crate) use cbi_version::*;

pub mod instance;
pub(crate) use instance::*;

pub mod module;
pub(crate) use module::*;
