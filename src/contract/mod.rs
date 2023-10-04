/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines types and functions that provide a convenient and succinct object-oriented interface for loading,
//! deploying, getting information about, and executing (WASM) smart contracts.
//!
//! A contract consists of a set of host functions that can be compiled into WASM [module]. The state transition
//! function prepares execution [context] and builds an [instance] from a contract with well-defined [functions].
//! The contract should match the current [version] of Contract Binary Interface ([cbi]). 

pub mod cbi;
pub(crate) use cbi::*;

pub mod context;
pub(crate) use context::*;

pub mod functions;
pub(crate) use functions::*;

pub mod instance;
pub(crate) use instance::*;

pub mod module;
pub(crate) use module::*;

pub mod version;
pub(crate) use version::*;
