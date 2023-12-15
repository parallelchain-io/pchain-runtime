/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Constants and formulas which are primitives used in the cost calculation logic of
//! [operations that incur gas](crate::gas::operations).
//!
//! The constants in this module are based on the specification described in the gas section of
//! [Parallelchain Mainnet Protocol](https://github.com/parallelchain-io/parallelchain-protocol).
//!
//! The table below lists the protocol-defined equivalents of the constants and formulas defined here, where applicable.
//! Do note that higher-level operation-specific formulas are defined directly
//! within the [operations](crate::gas::operations) module.
//!
//! |Protocol Name          | Related Function / Constants      |
//! |:---                   |:---                               |
//! |G_wread                | [wasm_memory_read_cost]           |
//! |G_wwrite               | [wasm_memory_write_cost]          |
//! |G_txdata               | [BLOCKCHAIN_WRITE_PER_BYTE_COST]  |
//! |G_minrcpsize           | [minimum_receipt_size_v1]         |
//! |G_minrcpsize_v2        | [minimum_receipt_size_v2]         |
//! |G_mincmdrcpsize        | [MIN_CMDRECP_SIZE_V1]             |
//! |G_mincmdrcpsize_v2     | [MIN_CMDRECP_SIZE_V2_BASIC], [MIN_CMDRECP_SIZE_V2_EXTENDED]|
//! |G_acckeylen            | [ACCOUNT_TRIE_KEY_LENGTH]         |
//! |G_mpt_get1             | [get_cost_read]                   |
//! |G_mpt_get2             | [get_cost_traverse]               |
//! |G_mpt_set              | [get_cost_read], [set_cost_delete_old_value], [set_cost_write_new_value], [set_cost_rehash] |
//! |G_mpt_write            | [MPT_WRITE_PER_BYTE_COST] |
//! |G_mpt_read             | [MPT_READ_PER_BYTE_COST]  |
//! |G_mpt_traverse         | [MPT_TRAVERSE_PER_BYTE_COST]      |
//! |G_mpt_rehash           | [MPT_REHASH_PER_BYTE_COST]        |
//! |G_mpt_refund           | [MPT_WRITE_REFUND_PROPORTION]     |
//! |G_at_getcontractdisc   | [MPT_GET_CODE_DISCOUNT_PROPORTION] |
//! |G_keccek256len         | [KECCAK256_LENGTH] |
//! |G_txincl               | [tx_inclusion_cost_v1] |
//! |G_txinclv2             | [tx_inclusion_cost_v2] |
//!

/* ↓↓↓ Gas Costs for Wasm opcode execution ↓↓↓ */

use wasmer::wasmparser::Operator;

use crate::types::CommandKind;

/// wasm_opcode_gas_schedule maps between a Wasm Operator to the cost of executing it.
/// It specifies the gas cost of executing every legal opcode for the smart contract method calls.
pub fn wasm_opcode_gas_schedule(operator: &Operator) -> u64 {
    match operator {
        // Constants
        Operator::I32Const { .. } | Operator::I64Const { .. } => 0,

        // Type parameteric operators
        Operator::Drop => 2,
        Operator::Select => 3,

        // Flow control
        Operator::Nop
        | Operator::Unreachable
        | Operator::Loop { .. }
        | Operator::Else
        | Operator::If { .. } => 0,
        Operator::Br { .. }
        | Operator::BrTable { .. }
        | Operator::Return
        | Operator::Call { .. }
        | Operator::CallIndirect { .. } => 2,
        Operator::BrIf { .. } => 3,

        // Registers
        Operator::GlobalGet { .. }
        | Operator::GlobalSet { .. }
        | Operator::LocalGet { .. }
        | Operator::LocalSet { .. } => 3,

        // Reference Types
        Operator::RefIsNull
        | Operator::RefFunc { .. }
        | Operator::RefNull { .. }
        | Operator::ReturnCall { .. }
        | Operator::ReturnCallIndirect { .. } => 2,

        // Exception Handling
        Operator::CatchAll
        | Operator::Throw { .. }
        | Operator::Rethrow { .. }
        | Operator::Delegate { .. } => 2,

        // Bluk Memory Operations
        Operator::ElemDrop { .. } | Operator::DataDrop { .. } => 1,
        Operator::TableInit { .. } => 2,
        Operator::MemoryCopy { .. }
        | Operator::MemoryFill { .. }
        | Operator::TableCopy { .. }
        | Operator::TableFill { .. } => 3,

        // Memory Operations
        Operator::I32Load { .. }
        | Operator::I64Load { .. }
        | Operator::I32Store { .. }
        | Operator::I64Store { .. }
        | Operator::I32Store8 { .. }
        | Operator::I32Store16 { .. }
        | Operator::I32Load8S { .. }
        | Operator::I32Load8U { .. }
        | Operator::I32Load16S { .. }
        | Operator::I32Load16U { .. }
        | Operator::I64Load8S { .. }
        | Operator::I64Load8U { .. }
        | Operator::I64Load16S { .. }
        | Operator::I64Load16U { .. }
        | Operator::I64Load32S { .. }
        | Operator::I64Load32U { .. }
        | Operator::I64Store8 { .. }
        | Operator::I64Store16 { .. }
        | Operator::I64Store32 { .. } => 3,

        // 32 and 64-bit Integer Arithmetic Operations
        Operator::I32Add
        | Operator::I32Sub
        | Operator::I64Add
        | Operator::I64Sub
        | Operator::I64LtS
        | Operator::I64LtU
        | Operator::I64GtS
        | Operator::I64GtU
        | Operator::I64LeS
        | Operator::I64LeU
        | Operator::I64GeS
        | Operator::I64GeU
        | Operator::I32Eqz
        | Operator::I32Eq
        | Operator::I32Ne
        | Operator::I32LtS
        | Operator::I32LtU
        | Operator::I32GtS
        | Operator::I32GtU
        | Operator::I32LeS
        | Operator::I32LeU
        | Operator::I32GeS
        | Operator::I32GeU
        | Operator::I64Eqz
        | Operator::I64Eq
        | Operator::I64Ne
        | Operator::I32And
        | Operator::I32Or
        | Operator::I32Xor
        | Operator::I64And
        | Operator::I64Or
        | Operator::I64Xor => 1,
        Operator::I32Shl
        | Operator::I32ShrU
        | Operator::I32ShrS
        | Operator::I32Rotl
        | Operator::I32Rotr
        | Operator::I64Shl
        | Operator::I64ShrU
        | Operator::I64ShrS
        | Operator::I64Rotl
        | Operator::I64Rotr => 2,
        Operator::I32Mul | Operator::I64Mul => 3,
        Operator::I32DivS
        | Operator::I32DivU
        | Operator::I32RemS
        | Operator::I32RemU
        | Operator::I64DivS
        | Operator::I64DivU
        | Operator::I64RemS
        | Operator::I64RemU => 80,
        Operator::I32Clz | Operator::I64Clz => 105,

        // Type Casting & Truncation Operations
        Operator::I32WrapI64
        | Operator::I64ExtendI32S
        | Operator::I64ExtendI32U
        | Operator::I32Extend8S
        | Operator::I32Extend16S
        | Operator::I64Extend8S
        | Operator::I64Extend16S
        | Operator::I64Extend32S => 3,

        // Everything Else is 1
        _ => 1,
    }
}

/* ↓↓↓ Gas Costs for Accessing Wasm memory from host functions ↓↓↓ */

/// WASM_MEMORY_WRITE_PER64_BITS_COST is the cost of writing into the WASM linear memory *per 64 bits*.
pub const WASM_MEMORY_WRITE_PER64_BITS_COST: u64 = 3;
/// WASM_MEMORY_READ_PER64_BITS_COST (C_I64Load) is the cost of writing into the WASM linear memory *per 64 bits*.
pub const WASM_MEMORY_READ_PER64_BITS_COST: u64 = 3;
/// WASM_BYTE_CODE_PER_BYTE_COST (C_I64Store) is the cost of checking whether input byte code satisfy CBI.
pub const WASM_BYTE_CODE_PER_BYTE_COST: u64 = 3;

/// Cost of reading `len` bytes from Wasm linear memory.
pub const fn wasm_memory_read_cost(len: usize) -> u64 {
    let cost = ceil_div_8(len as u64).saturating_mul(WASM_MEMORY_READ_PER64_BITS_COST);
    if cost == 0 {
        return 1;
    } // = max(cost, 1) to make sure charging for a non-zero cost
    cost
}

/// Cost of writing `len` bytes into Wasm linear memory.
pub const fn wasm_memory_write_cost(len: usize) -> u64 {
    let cost = ceil_div_8(len as u64).saturating_mul(WASM_MEMORY_WRITE_PER64_BITS_COST);
    if cost == 0 {
        return 1;
    } // = max(cost, 1) to make sure charging for a non-zero cost
    cost
}

/// Ceiling of the value after dividing by 8.
pub const fn ceil_div_8(l: u64) -> u64 {
    l.saturating_add(7).saturating_div(8)
}

/* ↓↓↓ Gas Costs for Transaction-related data storage ↓↓↓ */

/// Cost of including 1 byte of data in a Block as part of a transaction or a receipt.
pub const BLOCKCHAIN_WRITE_PER_BYTE_COST: u64 = 30;
/// Serialized size of a ReceiptV1 containing empty command receipts
pub const MIN_RECP_SIZE_V1: u64 = 4;
/// Serialized size of a ReceiptV2 containing empty command receipts
pub const MIN_RECP_SIZE_V2: u64 = 13;
/// Serialized size of a minimum-size CommandReceiptV1.
pub const MIN_CMDRECP_SIZE_V1: u64 = 17;
/// Serialized size of a CommandReceiptV2 with common fields only.
pub const MIN_CMDRECP_SIZE_V2_BASIC: u64 = 9;
/// Serialized size of a CommandReceiptV2 with custom fields for certain commands.
pub const MIN_CMDRECP_SIZE_V2_EXTENDED: u64 = 17;

///  minimum cost for a V1 transaction to be included in the blockchain.
///
/// It consists of:
/// 1. cost for storing transaction in a block
/// 2. cost for storing minimum-sized receipt(s) in a block
/// 3. cost for 5 read-write operations for
///     - signer's nonce
///     - signer's balance during two phases
///     - proposer's balance
///     - treasury's balance
pub fn tx_inclusion_cost_v1(tx_size: usize, commands: &Vec<CommandKind>) -> u64 {
    // (1) Transaction storage size
    let tx_size = tx_size as u64;
    // (2) Minimum size of receipt
    let min_receipt_size = minimum_receipt_size_v1(commands);
    // (3) Cost for 5 read-write operations
    let rw_key_cost = (
        // Read cost
        get_cost_traverse(ACCOUNT_TRIE_KEY_LENGTH)
            .saturating_add(get_cost_read(8))
            // Write cost
            .saturating_add(set_cost_write_new_value(8))
            .saturating_add(set_cost_rehash(ACCOUNT_TRIE_KEY_LENGTH))
    )
    .saturating_mul(5);

    // Multiply by blockchain storage write cost and add the cost of 5 read-write operations.
    tx_size
        .saturating_add(min_receipt_size)
        .saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST)
        .saturating_add(rw_key_cost)
}

/// minimum cost for a V2 transaction to be included in the blockchain.
///
/// It consists of:
/// 1. cost for storing transaction in a block
/// 2. cost for storing minimum-sized receipt(s) in a block
/// 3. cost for 5 read-write operations for
///     - signer's nonce
///     - signer's balance during two phases
///     - proposer's balance
///     - treasury's balance
///
/// supersedes [V1](tx_inclusion_cost_v1)
pub fn tx_inclusion_cost_v2(tx_size: usize, commands: &Vec<CommandKind>) -> u64 {
    // (1) Transaction storage size
    let tx_size = tx_size as u64;
    // (2) Minimum size of receipt
    let min_receipt_size = minimum_receipt_size_v2(commands);
    // (3) Cost for 5 read-write operations
    let rw_key_cost = (
        // Read cost
        get_cost_traverse(ACCOUNT_TRIE_KEY_LENGTH)
            .saturating_add(get_cost_read(8))
            // Write cost
            .saturating_add(set_cost_write_new_value(8))
            .saturating_add(set_cost_rehash(ACCOUNT_TRIE_KEY_LENGTH))
    )
    .saturating_mul(5);
    // Multiply by blockchain storage write cost and add the cost of 5 read-write operations.
    tx_size
        .saturating_add(min_receipt_size)
        .saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST)
        .saturating_add(rw_key_cost)
}

/// Serialized size of a ReceiptV1 for `Vec<CommandKind>` containing minimum-sized command receipts.
pub fn minimum_receipt_size_v1(commands: &Vec<CommandKind>) -> u64 {
    MIN_RECP_SIZE_V1.saturating_add(MIN_CMDRECP_SIZE_V1.saturating_mul(commands.len() as u64))
}

/// Serialized size of a ReceiptV2 for `Vec<CommandKind>` containing minimum-sized command receipts.
pub fn minimum_receipt_size_v2(commands: &Vec<CommandKind>) -> u64 {
    MIN_RECP_SIZE_V2.saturating_add(
        commands
            .iter()
            .fold(0, |acc, cmd| acc.saturating_add(cmd_recp_min_size_v2(cmd))),
    )
}

/// blockchain_return_values_cost calculates the cost of writing byte data into the receipt.
pub const fn blockchain_storage_cost(data_len: usize) -> u64 {
    // data_len * C_txdata
    (data_len as u64).saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST)
}

/// blockchain_log_cost calculates the cost of writing a log into the receipt.
pub const fn blockchain_log_cost(topic_len: usize, val_len: usize) -> u64 {
    let topic_len = topic_len as u64;
    let val_len = val_len as u64;
    let log_len = topic_len.saturating_add(val_len);

    // Ceil(l/8) * C_wasmread
    (ceil_div_8(log_len).saturating_mul(WASM_MEMORY_READ_PER64_BITS_COST))
        // t * C_sha256
        .saturating_add(topic_len.saturating_mul(CRYPTO_SHA256_PER_BYTE))
        // l X Z
        .saturating_add(log_len.saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST))
}

/* ↓↓↓ World state storage and access ↓↓↓ */

/// The length of keys in the root world state MPT.
pub const ACCOUNT_TRIE_KEY_LENGTH: usize = 33;
/// Cost of writing a single byte into the MPT's backing storage.
pub const MPT_WRITE_PER_BYTE_COST: u64 = 2500;
/// Cost of reading a single byte from the MPT's backing storage.
pub const MPT_READ_PER_BYTE_COST: u64 = 50;
/// Cost of traversing 1 byte (2 nibbles) down an MPT.
pub const MPT_TRAVERSE_PER_BYTE_COST: u64 = 20;
/// Cost of traversing 1 byte up (2 nibbles) and recomputing the SHA256 hashes of 2 nodes
/// in an MPT after it or one of its descendants is changed.
pub const MPT_REHASH_PER_BYTE_COST: u64 = 130;
/// Proportion of the cost of writing a tuple into an MPT that is refunded when that tuple is re-set or deleted.
pub const MPT_WRITE_REFUND_PROPORTION: u64 = 50;
/// Proportion of get cost which is discounted if the tuple contains a contract.
pub const MPT_GET_CODE_DISCOUNT_PROPORTION: u64 = 50;
/// Length of a Keccak256 hash.
pub const KECCAK256_LENGTH: u64 = 32;

/// calculates the cost of traversing between nodes in the MPT data structure,
/// based on the length of the key. The cost is proportional to the number of nodes traversed.
pub const fn get_cost_traverse(key_len: usize) -> u64 {
    (key_len as u64).saturating_mul(MPT_TRAVERSE_PER_BYTE_COST)
}

/// calculates the cost of reading the value stored at a particular MPT node
pub const fn get_cost_read(value_len: usize) -> u64 {
    (value_len as u64).saturating_mul(MPT_READ_PER_BYTE_COST)
}

/// discount_code_read applies a discount to the read cost if the value read is contract code
pub fn discount_code_read(code_read_cost: u64) -> u64 {
    code_read_cost
        .saturating_mul(MPT_GET_CODE_DISCOUNT_PROPORTION)
        .saturating_div(100)
}

/// cost of hashing Storage Trie costs
pub const fn storage_trie_key_hashing_cost(key_len: usize) -> u64 {
    if key_len < 32 {
        0
    } else {
        CRYPTO_KECCAK256_PER_BYTE * key_len as u64
    }
}

/// Set Cost (2): Cost for deleting the old value for a refund
/// Note, Set Cost (1) is calculated under Get costs
#[allow(clippy::double_comparisons)]
pub const fn set_cost_delete_old_value(
    key_len: usize,
    old_val_len: usize,
    new_val_len: usize,
) -> u64 {
    let old_val_len = old_val_len as u64; // (a)
    let new_val_len = new_val_len as u64; // (b)
    if (old_val_len > 0 || old_val_len == 0) && new_val_len > 0 {
        // a * C_write * C_refund
        old_val_len
            .saturating_mul(MPT_WRITE_PER_BYTE_COST * MPT_WRITE_REFUND_PROPORTION)
            .saturating_div(100)
    } else if old_val_len > 0 && new_val_len == 0 {
        // (k + a) * C_write * C_refund
        ((key_len as u64).saturating_add(old_val_len))
            .saturating_mul(MPT_WRITE_PER_BYTE_COST * MPT_WRITE_REFUND_PROPORTION)
            .saturating_div(100)
    } else {
        // old_val_len == 0 && new_val_len == 0
        0
    }
}

/// Set Cost (3): Cost for writing a new value
pub const fn set_cost_write_new_value(new_val_len: usize) -> u64 {
    // b * C_write
    (new_val_len as u64).saturating_mul(MPT_WRITE_PER_BYTE_COST)
}

/// Set Cost (4): Cost for recomputing node hashes until the root
pub const fn set_cost_rehash(key_len: usize) -> u64 {
    // k * C_rehash
    (key_len as u64).saturating_mul(MPT_REHASH_PER_BYTE_COST)
}

/* ↓↓↓ Gas Costs for crypto functions ↓↓↓ */

/// Multiplier of computing the SHA256 hash over the length of a message.
pub const CRYPTO_SHA256_PER_BYTE: u64 = 16;
/// Multiplier of computing the Keccak256 hash over the length of a message.
pub const CRYPTO_KECCAK256_PER_BYTE: u64 = 16;
/// Multiplier of computing the RIPEMD160  hash over the length of a message.
pub const CRYPTO_RIPEMD160_PER_BYTE: u64 = 16;
/// Multiplier of verifying the Ed25519 signature over the length of a message.
pub const CRYPTO_ED25519_PER_BYTE: u64 = 16;

fn cmd_recp_min_size_v2(command: &CommandKind) -> u64 {
    match command {
        CommandKind::Call
        | CommandKind::WithdrawDeposit
        | CommandKind::StakeDeposit
        | CommandKind::UnstakeDeposit => MIN_CMDRECP_SIZE_V2_EXTENDED,
        _ => MIN_CMDRECP_SIZE_V2_BASIC,
    }
}
