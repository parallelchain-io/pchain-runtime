/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! ParallelChain Mainnet Runtime is a **State Transition Function**
//! that transits an input state of the blockchain to the next state.
//!
//! It is also the sole system component that executes WebAssembly (Wasm) smart contracts
//! written in Rust by using the ParallelChain Smart Contract Development Kit (SDK).
//!
//! ```text
//! f(WS, BD, TX) -> (WS', R)
//!
//! WS = World state represented by set of key-value pairs
//! BD = Blockchain Data
//! TX = Transaction, which is essentially a sequence of Commands
//! R = Receipt, comprising mainly a corresponding sequence of Command Receipts
//! ```
//!
//! ### Example
//!
//! ```rust
//! // prepare world state (ws), transaction (tx), and blockchain data (bd),
//! // call the respective transition function.
//!
//! // using WorldState::<S,V1> and TransactionV1
//! let result = pchain_runtime::Runtime::new().transition_v1(ws, tx, bd);
//!
//! // or using WorldState::<S,V2> and TransactionV2
//! let result = pchain_runtime::Runtime::new().transition_v2(ws, tx, bd);
//! ```
//!
//! In summary, a state [transition] function takes in Transaction, Blockchain and World State to [execute](execution),
//! and outputs a transition result which could be success or [error].
//!
//! The transition follows the data [type](types) definitions of ParallelChain Mainnet
//! and the [reward formulas](rewards_formulas) in this library.
//!
//! The execution of [commands](commands) incurs gas, and this will be recorded in the respective receipts.
//! Smart [contracts](contract) can also effect state transitions, through the underlying [wasmer](wasmer) WebAssembly runtime.

pub mod commands;

pub mod context;

pub mod contract;
pub use contract::cbi_version::cbi_version;
pub use contract::wasmer::cache::Cache;

pub mod error;
pub use error::TransitionError;

pub mod execution;

pub mod gas;
pub mod rewards_formulas;

pub mod transition;
pub use transition::{
    Runtime, TransitionV1Result, TransitionV1ToV2Result, TransitionV2Result, ValidatorChanges,
};

pub mod types;
pub use types::{BlockProposalStats, BlockchainParams, CommandKind, ValidatorPerformance};
