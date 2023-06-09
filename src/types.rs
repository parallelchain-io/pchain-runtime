/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines common data structures to be used inside this library, or from outside application.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use pchain_types::{
    blockchain::{Command, Transaction},
    cryptography::{PublicAddress, Sha256Hash},
};

/// BaseTx consists of common fields inside [Transaction].
#[derive(Clone, Default)]
pub(crate) struct BaseTx {
    pub signer: PublicAddress,
    pub hash: Sha256Hash,
    pub nonce: u64,
    pub gas_limit: u64,
    pub priority_fee_per_gas: u64,
}

impl From<&Transaction> for BaseTx {
    fn from(tx: &Transaction) -> Self {
        Self {
            signer: tx.signer,
            hash: tx.hash,
            nonce: tx.nonce,
            gas_limit: tx.gas_limit,
            priority_fee_per_gas: tx.priority_fee_per_gas,
        }
    }
}

/// CallTx is a struct representation of [pchain_types::Command::Call].
#[derive(Clone)]
pub(crate) struct CallTx {
    pub base_tx: BaseTx,
    pub target: PublicAddress,
    pub method: String,
    pub arguments: Option<Vec<Vec<u8>>>,
    pub amount: Option<u64>,
}

impl Deref for CallTx {
    type Target = BaseTx;
    fn deref(&self) -> &Self::Target {
        &self.base_tx
    }
}

impl DerefMut for CallTx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base_tx
    }
}

/// DeferredCommand is the command created from contract call.
#[derive(Clone, Debug)]
pub(crate) struct DeferredCommand {
    pub contract_address: PublicAddress,
    pub command: Command,
}

/// Defines information that are supplied to state transition function.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BlockchainParams {
    /// Height of the Block
    pub this_block_number: u64,
    /// Previous Block Hash
    pub prev_block_hash: Sha256Hash,
    /// Base fee in the Block
    pub this_base_fee: u64,
    /// Unix timestamp
    pub timestamp: u32,
    /// Random Bytes (Reserved.)
    pub random_bytes: Sha256Hash,
    /// Address of block proposer
    pub proposer_address: PublicAddress,
    /// Address of the treasury
    pub treasury_address: PublicAddress,
    /// The current view for this block, given from hotstuff_rs
    pub cur_view: u64,
    /// Validator performance is measured by the number of proposed blocks for each validators.
    /// It is optional because it is not needed in every transaction.
    pub validator_performance: Option<ValidatorPerformance>,
}

/// Input for epoch transaction, which is a factor in Pool reward calculation
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidatorPerformance {
    /// Number of blocks per epoch
    pub blocks_per_epoch: u32,
    /// A map from a pool address to block proposal statistics
    pub stats: HashMap<PublicAddress, BlockProposalStats>,
}

/// Statistics on Block Proposal
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockProposalStats {
    /// Number of proposed blocks within an epoch
    pub num_of_proposed_blocks: u32,
}

impl BlockProposalStats {
    pub fn new(num_of_proposed_blocks: u32) -> Self {
        Self {
            num_of_proposed_blocks,
        }
    }
}
