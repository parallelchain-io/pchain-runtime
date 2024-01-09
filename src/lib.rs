/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/


//! `pchain-runtime` is the reference implementation of a ParallelChain Runtime, the component of the 
//! ParallelChain protocol that executes transactions.
//! 
//! ## Transition function
//! 
//! The interface that the Runtime offers to the other components of the ParallelChain protocol is called the
//! *Transition Function*. The transition function is a pure function with three parameters and two return 
//! values. Its signature can be represented symbolically as follows:
//! 
//! ```transition(ws, txn, bp) -> (ws', rcp)```.
//! 
//! In the above signature:
//! - `txn` denotes a [Transaction](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Blockchain.md#transaction)
//!    to be executed.
//! - `ws` denotes the current world state [World State](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/World%20State.md).
//! - `bp` denotes a set of information about the current contents of the Blockchain, including information 
//!    about the current [Block](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Blockchain.md#block-header)
//!    (e.g., the current block height).
//! - `ws'` denotes the world state after the execution of `txn`.
//! - `rcp` denotes the [Receipt](https://github.com/parallelchain-io/parallelchain-protocol/blob/master/Blockchain.md#receipt),
//!    a compact summary of "what happened" in the transaction's execution.
//!
//! ## Versioning
//! 
//! This version (v0.5.0) of `pchain-runtime` implements the Runtime up to **v0.5** of the ParallelChain Protocol.
//! Therefore, this library contains implementations of [V1](transition::Runtime::transition_v1) and 
//! [V2](transition::Runtime::transition_v2) of the transition function, as well as the special 
//! ["V1 to V2"](transition::Runtime::transition_v1_to_v2) transition function that is used to transition
//! a blockchain from a V1 World State and V1 Blocks to a V2 World State and V2 Blocks.
//!
//! ## Usage
//! 
//! To use `pchain-runtime`, create an instance of [Runtime](transition::Runtime), then call one of its "transition_v*"
//! methods (e.g., [transition_v2](transition::Runtime::transition_v2)). For example:
//! 
//! ```rust
//! // prepare world state (ws), transaction (tx), and blockchain params (bp),
//! // call the respective transition function.
//!
//! // using WorldState::<S,V1> and TransactionV1
//! let result = pchain_runtime::Runtime::new().transition_v1(ws, tx, bp);
//!
//! // or using WorldState::<S,V2> and TransactionV2
//! let result = pchain_runtime::Runtime::new().transition_v2(ws, tx, bp);
//! ```

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
