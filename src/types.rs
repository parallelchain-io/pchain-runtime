/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines common data structures to be used inside this library, or from outside application.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use pchain_types::{
    blockchain::{Command, TransactionV1, TransactionV2},
    cryptography::{PublicAddress, Sha256Hash},
    serialization::Serializable,
};

/// BaseTx consists of common fields inside [Transaction].
#[derive(Clone, Default)]
pub(crate) struct BaseTx {
    pub version: TxnVersion,
    pub command_kinds: Vec<CommandKind>,

    pub signer: PublicAddress,
    pub hash: Sha256Hash,
    pub nonce: u64,
    pub gas_limit: u64,
    pub priority_fee_per_gas: u64,

    // serialized size of the original transaction
    pub size: usize,
    /// length of commands in the transaction
    pub commands_len: usize,
}

impl From<&TransactionV1> for BaseTx {
    fn from(tx: &TransactionV1) -> Self {
        Self {
            version: TxnVersion::V1,
            command_kinds: tx.commands.iter().map(CommandKind::from).collect(),
            signer: tx.signer,
            hash: tx.hash,
            nonce: tx.nonce,
            gas_limit: tx.gas_limit,
            priority_fee_per_gas: tx.priority_fee_per_gas,
            size: tx.serialize().len(),
            commands_len: tx.commands.len(),
        }
    }
}

impl From<&TransactionV2> for BaseTx {
    fn from(tx: &TransactionV2) -> Self {
        Self {
            version: TxnVersion::V2,
            command_kinds: tx.commands.iter().map(CommandKind::from).collect(),
            signer: tx.signer,
            hash: tx.hash,
            nonce: tx.nonce,
            gas_limit: tx.gas_limit,
            priority_fee_per_gas: tx.priority_fee_per_gas,
            size: tx.serialize().len(),
            commands_len: tx.commands.len(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TxnVersion {
    V1,
    V2
}

impl Default for TxnVersion {
    fn default() -> Self {
        Self::V1
    }
}

#[derive(Clone, Copy)]
pub(crate) enum CommandKind {
    Transfer,
    Deploy,
    Call, 
    CreatePool,
    SetPoolSettings,
    DeletePool,
    CreateDeposit,
    SetDepositSettings,
    TopUpDeposit,
    WithdrawDeposit,
    StakeDeposit,
    UnstakeDeposit,
    NextEpoch
}

impl From<&Command> for CommandKind {
    fn from(command: &Command) -> Self {
        match command {
            Command::Transfer(_) => CommandKind::Transfer,
            Command::Deploy(_) => CommandKind::Deploy,
            Command::Call(_) => CommandKind::Call,
            Command::CreatePool(_) => CommandKind::CreatePool,
            Command::SetPoolSettings(_) => CommandKind::SetPoolSettings,
            Command::DeletePool => CommandKind::DeletePool,
            Command::CreateDeposit(_) => CommandKind::CreateDeposit,
            Command::SetDepositSettings(_) => CommandKind::SetDepositSettings,
            Command::TopUpDeposit(_) => CommandKind::TopUpDeposit,
            Command::WithdrawDeposit(_) => CommandKind::WithdrawDeposit,
            Command::StakeDeposit(_) => CommandKind::StakeDeposit,
            Command::UnstakeDeposit(_) => CommandKind::UnstakeDeposit,
            Command::NextEpoch => CommandKind::NextEpoch,
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
