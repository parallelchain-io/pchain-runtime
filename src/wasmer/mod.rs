/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! wasmer defines wasmer environment, memory and store. The state transition funcion executes
//! contract in [Wasmer Environment](wasmer_env) after creation of a [Wasmer Store](wasmer_store). 
//! The environment defines read-write access to [Wasmer Memory](wasmer_memory). The store is
//! created with a [Wasmer OpCode Filter](non_determinism_filter) so that invalid contract will
//! not be executed. The state transition function might specify a [cache] for storing compiled 
//! smart contract to execute.

pub mod wasmer_env;

pub mod wasmer_memory;

pub mod wasmer_store;

mod non_determinism_filter;

pub mod cache;

pub mod custom_tunables;