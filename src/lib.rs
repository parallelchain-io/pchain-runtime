/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! ParallelChain Mainnet Runtime is a **State Transition Function** to transit from an input state of the blockchain to next state.
//! It is also the sole system component to handle Smart Contract that is primarily built from Rust code by using
//! ParallelChain Smart Contract Development Kit (SDK).
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
//! let result = pchain_runtime::Runtime::new().transition(ws, tx, bd);
//! ```
//!
//! In summary, a state [transition] function intakes Transaction, Blockchain and World State to [execute](execution),
//! and output transition result which could be a success result or an [error]. The transition follows
//! the data [types] definitions of ParallelChain Mainnet and the [formulas] in this library.
//! When transiting the state by executing smart [contract], it uses [wasmer] as underlying WebAssembly runtime,
//! which is gas-metered, and the [gas] [cost] incurred will be set to transaction receipt.

pub mod commands;

pub mod contract;
pub use contract::wasmer::cache::Cache;

pub mod error;
pub use error::TransitionError;

pub mod execution;

pub mod formulas;

pub mod gas;

pub mod transition;
pub use transition::{cbi_version, Runtime, TransitionResultV1, ValidatorChanges};

pub mod types;
pub use types::{BlockProposalStats, BlockchainParams, ValidatorPerformance};
