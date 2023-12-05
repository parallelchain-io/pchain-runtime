/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines types and functions that provide a convenient and succinct object-oriented interface for loading,
//! deploying, getting information about, and executing Wasm smart contracts.
//!
//! A contract consists of a Wasm [module] that contains one or more guest functions.
//! It should match the current [version][cbi_version] of Contract Binary Interface ([cbi](cbi_host_functions)) and is free to use functionality provided by the [host](host_functions).
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
