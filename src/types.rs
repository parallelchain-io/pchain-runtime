/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines common data structures to be used inside this library, or from outside application.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use pchain_types::blockchain::{Log, CommandReceiptV2, ExitCodeV2, TransferReceipt, DeployReceipt, CallReceipt, CreatePoolReceipt, SetPoolSettingsReceipt, DeletePoolReceipt, CreateDepositReceipt, SetDepositSettingsReceipt, WithdrawDepositReceipt, StakeDepositReceipt, UnstakeDepositReceipt, NextEpochReceipt, TopUpDepositReceipt};
use pchain_types::{
    blockchain::{Command, TransactionV1, TransactionV2},
    cryptography::{PublicAddress, Sha256Hash},
    serialization::Serializable,
};

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


#[derive(Clone, Default)]
pub(crate) struct CommandOutput {
    /// Output value in [pchain_types::blockchain::CallReceipt].
    pub logs: Vec<Log>,
    /// Output value in [pchain_types::blockchain::CallReceipt].
    pub return_values: Vec<u8>,
    /// Output value in [pchain_types::blockchain::WithdrawDepositReceipt].
    pub amount_withdrawn: u64,
    /// Output value in [pchain_types::blockchain::StakeDepositReceipt].
    pub amount_staked: u64,
    /// Output value in [pchain_types::blockchain::UnstakeDepositReceipt].
    pub amount_unstaked: u64,
}


pub(crate) fn create_executed_receipt_v2(
    command: &CommandKind, 
    exit_code: ExitCodeV2, 
    gas_used: u64, 
    command_output: CommandOutput,
) -> CommandReceiptV2 {
    match command {
        CommandKind::Transfer => CommandReceiptV2::Transfer(TransferReceipt { exit_code, gas_used }),
        CommandKind::Deploy => CommandReceiptV2::Deploy(DeployReceipt { exit_code, gas_used }),
        CommandKind::Call => CommandReceiptV2::Call(CallReceipt { exit_code, gas_used, logs: command_output.logs, return_value: command_output.return_values }),
        CommandKind::CreatePool => CommandReceiptV2::CreatePool(CreatePoolReceipt { exit_code, gas_used }),
        CommandKind::SetPoolSettings => CommandReceiptV2::SetPoolSettings(SetPoolSettingsReceipt { exit_code, gas_used }),
        CommandKind::DeletePool => CommandReceiptV2::DeletePool(DeletePoolReceipt { exit_code, gas_used }),
        CommandKind::CreateDeposit => CommandReceiptV2::CreateDeposit(CreateDepositReceipt { exit_code, gas_used }),
        CommandKind::SetDepositSettings => CommandReceiptV2::SetDepositSettings(SetDepositSettingsReceipt { exit_code, gas_used }),
        CommandKind::TopUpDeposit => CommandReceiptV2::TopUpDeposit(TopUpDepositReceipt { exit_code, gas_used }),
        CommandKind::WithdrawDeposit => CommandReceiptV2::WithdrawDeposit(WithdrawDepositReceipt { exit_code, gas_used, amount_withdrawn: command_output.amount_withdrawn }),
        CommandKind::StakeDeposit => CommandReceiptV2::StakeDeposit(StakeDepositReceipt { exit_code, gas_used, amount_staked: command_output.amount_staked }),
        CommandKind::UnstakeDeposit => CommandReceiptV2::UnstakeDeposit(UnstakeDepositReceipt { exit_code, gas_used, amount_unstaked: command_output.amount_unstaked }),
        CommandKind::NextEpoch => CommandReceiptV2::NextEpoch(NextEpochReceipt { exit_code, gas_used }),
    }
}

pub(crate) fn create_not_executed_receipt_v2(command: &CommandKind) -> CommandReceiptV2 {
    match command {
        CommandKind::Transfer => CommandReceiptV2::Transfer(TransferReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::Deploy => CommandReceiptV2::Deploy(DeployReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::Call => CommandReceiptV2::Call(CallReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, logs: Vec::new(), return_value: Vec::new() }),
        CommandKind::CreatePool => CommandReceiptV2::CreatePool(CreatePoolReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::SetPoolSettings => CommandReceiptV2::SetPoolSettings(SetPoolSettingsReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::DeletePool => CommandReceiptV2::DeletePool(DeletePoolReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::CreateDeposit => CommandReceiptV2::CreateDeposit(CreateDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::SetDepositSettings => CommandReceiptV2::SetDepositSettings(SetDepositSettingsReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::TopUpDeposit => CommandReceiptV2::TopUpDeposit(TopUpDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::WithdrawDeposit => CommandReceiptV2::WithdrawDeposit(WithdrawDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, amount_withdrawn: 0 }),
        CommandKind::StakeDeposit => CommandReceiptV2::StakeDeposit(StakeDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, amount_staked: 0 }),
        CommandKind::UnstakeDeposit => CommandReceiptV2::UnstakeDeposit(UnstakeDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, amount_unstaked: 0}),
        CommandKind::NextEpoch => CommandReceiptV2::NextEpoch(NextEpochReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted })
    }
}

pub(crate) fn gas_used_and_exit_code_v2(command_receipt_v2: &CommandReceiptV2) -> (u64, ExitCodeV2) {
    macro_rules! exit_code_v2 {
        ($cmd_recp2:ident, $($var:path,)*) => {
            match $cmd_recp2 {
                $(
                    $var(receipt) => (receipt.gas_used, receipt.exit_code.clone()),
                )*
            }
        };
    }

    exit_code_v2!(
        command_receipt_v2,
        CommandReceiptV2::Transfer,
        CommandReceiptV2::Call,
        CommandReceiptV2::Deploy,
        CommandReceiptV2::CreatePool,
        CommandReceiptV2::SetPoolSettings,
        CommandReceiptV2::DeletePool,
        CommandReceiptV2::CreateDeposit,
        CommandReceiptV2::SetDepositSettings,
        CommandReceiptV2::TopUpDeposit,
        CommandReceiptV2::WithdrawDeposit,
        CommandReceiptV2::StakeDeposit,
        CommandReceiptV2::UnstakeDeposit,
        CommandReceiptV2::NextEpoch,
    )
}

pub(crate) fn set_gas_used_and_exit_code_v2(command_receipt_v2: &mut CommandReceiptV2, gas_used: u64, exit_code: ExitCodeV2) {
    macro_rules! exit_code_v2 {
        ($cmd_recp2:ident, $($var:path,)*) => {
            match $cmd_recp2 {
                $(
                    $var(receipt) => { receipt.gas_used = gas_used; receipt.exit_code = exit_code; },
                )*
            }
        };
    }

    exit_code_v2!(
        command_receipt_v2,
        CommandReceiptV2::Transfer,
        CommandReceiptV2::Call,
        CommandReceiptV2::Deploy,
        CommandReceiptV2::CreatePool,
        CommandReceiptV2::SetPoolSettings,
        CommandReceiptV2::DeletePool,
        CommandReceiptV2::CreateDeposit,
        CommandReceiptV2::SetDepositSettings,
        CommandReceiptV2::TopUpDeposit,
        CommandReceiptV2::WithdrawDeposit,
        CommandReceiptV2::StakeDeposit,
        CommandReceiptV2::UnstakeDeposit,
        CommandReceiptV2::NextEpoch,
    )
}