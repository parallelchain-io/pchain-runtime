/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/
 
//! contract defines types and functions that provide a convenient and succinct object-oriented interface for loading,
//! deploying, getting information about, and executing (WASM) smart contracts inside Executor and else where. 
//! 
//! A contract consists of a set of host functions that can be compiled into WASM [module]. The state transition 
//! function prepares execution [context] and builds an [instance] from a contract with with well-defined [functions].
//! The contract should matche with the current [version] of Contract Binary Interface ([cbi]),

pub(crate) mod cbi;
pub(crate) use cbi::*;

pub(crate) mod context;
pub(crate) use context::*;

pub(crate) mod functions;
pub(crate) use functions::*;

pub(crate) mod instance;
pub(crate) use instance::*;

pub(crate) mod module;
pub(crate) use module::*;

pub(crate) mod version;
pub(crate) use version::*;