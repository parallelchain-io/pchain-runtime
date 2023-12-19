/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Utilizes [Wasmer](https://wasmer.io/) for smart contract execution in a Wasm environment.
//!
//! This module orchestrates the execution of smart contract code within a Wasm [instance].
//!
//! The instance is created using resources from both a [Wasm environment](mod@env) and a [Wasm Store](store),
//! where the environment provides read-write access to [Wasm linear memory](memory),
//! and the store, equipped with a [Wasmer OpCode Filter](non_determinism_filter),
//! maintains the runtime state and ensures that only valid contracts are executed.
//!
//! To reduce compilation times, a [cache] is also available for storing compiled smart contracts.

pub mod env;

pub mod memory;

pub mod module;

pub mod store;

pub mod non_determinism_filter;

pub mod cache;

pub mod custom_tunables;

pub mod instance;
