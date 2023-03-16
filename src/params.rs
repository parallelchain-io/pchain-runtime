/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! The Input Structures that are used as input to state transition function.

use std::collections::HashMap;

use pchain_types::{Sha256Hash, PublicAddress, ViewNumber};


/// BlockchainParams defines information that are supplied to state transition function.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub cur_view: ViewNumber,
    /// Validator performance is measured by the number of proposed blocks for each validators. 
    /// It is optional because it is not needed in every transaction.
    pub validator_performance: Option<ValidatorPerformance>
}

/// ValidatorPerformance is the an input for epoch transaction, which is a factor in Pool reward calculation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidatorPerformance {
    /// Number of blocks per epoch
    pub blocks_per_epoch: u32,
    /// A map from a pool address to block proposal statistics
    pub stats: HashMap<PublicAddress, BlockProposalStats>
}

/// Block Proposal Statistics
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockProposalStats {
    /// Number of proposed blocks within an epoch
    pub num_of_proposed_blocks: u32,
}

impl BlockProposalStats {
    pub fn new(num_of_proposed_blocks: u32) -> Self {
        Self { num_of_proposed_blocks }
    }
}