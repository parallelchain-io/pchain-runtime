/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! ParallelChain F Runtime is a **State Transition Function** to transit from an input state of the blockchain to next state. 
//! It is also the sole system component to handle Smart Contract that is primarily built from Rust code by using 
//! ParallelChain F Smart Contract Development Kit (SDK).
//! 
//! ```text
//! f(WS, BD, TX) -> (WS', R)
//! 
//! WS = World state represented by set of key-value pairs
//! BD = Blockchain Data
//! TX = Transaction, which is essentially a sequence of Commands
//! R = Receipt, which is a sequence of Command Receipts correspondingly.
//! ```
//! 
//! ### Example
//! 
//! ```rust
//! // prepare world state (ws), transaction (tx), and blockchain data (bd),
//! // and call transition.
//! let result = pchain_runtime::new().transition(ws, tx, bd);
//! ```
//! 
//! In summary, A state [transition] function intakes Transaction, Blockchain [params] and world state to execute 
//! [transactions], and output transition result which could be a success result or an [error]. The transition follows 
//! the data [types] definitions of ParallelChain F. In Call Transaction, it uses [wasmer] as underlying WebAssembly 
//! runtime to invoke a [contract], which is gas-metered, and the [cost] incurred will be set to transaction receipt.

mod contract;

mod cost;
pub use cost::gas;

mod error;
pub use error::TransitionError;

pub mod params;
pub use params::{
    BlockchainParams,
    BlockProposalStats,
    ValidatorPerformance
};

mod transactions;

mod transition;
pub use transition::{
    cbi_version,
    Runtime,
    TransitionResult,
    ValidatorChanges
};

mod types;

mod wasmer;
pub use crate::wasmer::cache::Cache;