/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines formulas in calculation of gas which is a measurement unit for transaction
//! execution. The constants and functions in module follow the content in gas section of the
//! [Parallelchain Mainnet Protocol](https://github.com/parallelchain-io/parallelchain-protocol).
//!
//! The mapping of equations or variables in the protocol to this module is as following:
//!
//! |Name       | Related Function / Constants      |
//! |:---       |:---                       |
//! |G_wread    | [wasm_memory_read_cost]   |
//! |G_wwrite   | [wasm_memory_write_cost]  |
//! |G_txdata   | [BLOCKCHAIN_WRITE_PER_BYTE_COST]  |
//! |G_mincmdrcpsize| [minimum_receipt_size]        |
//! |G_acckeylen    | [ACCOUNT_STATE_KEY_LENGTH]    |
//! |G_sget         | [get_cost], [get_code_cost]   |
//! |G_sset         | [set_cost_read_key], [set_cost_delete_old_value], [set_cost_write_new_value], [set_cost_rehash] |
//! |G_scontains    | [contains_cost]       |
//! |G_swrite   | [MPT_WRITE_PER_BYTE_COST] |
//! |G_sread    | [MPT_READ_PER_BYTE_COST]  |
//! |G_straverse| [MPT_TRAVERSE_PER_BYTE_COST]      |
//! |G_srehash  | [MPT_REHASH_PER_BYTE_COST]        |
//! |G_srefund  | [MPT_WRITE_REFUND_PROPORTION]     |
//! |G_sgetcontractdisc  | [MPT_GET_CODE_DISCOUNT_PROPORTION] |
//! |G_wsha256  | [CRYPTO_SHA256_PER_BYTE]          |
//! |G_wkeccak256   | [CRYPTO_KECCAK256_PER_BYTE]   |
//! |G_wripemd160   | [CRYPTO_RIPEMD160_PER_BYTE]   |
//! |G_wvrfy25519   | [crypto_verify_ed25519_signature_cost]  |
//! |G_txincl       | [tx_inclusion_cost] |
//!

/* ↓↓↓ Gas Costs for WASM opcode execution ↓↓↓ */

use wasmer::wasmparser::Operator;

/// wasm_opcode_gas_schedule maps between a WASM Operator to the cost of executing it. It
/// specifies the gas cost of executing every legal opcode for the smart contract method calls.
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

/* ↓↓↓ Gas Costs for Accessing WASM memory from host functions ↓↓↓ */

/// WASM_MEMORY_WRITE_PER64_BITS_COST is the cost of writing into the WASM linear memory *per 64 bits*.
pub const WASM_MEMORY_WRITE_PER64_BITS_COST: u64 = 3;
/// WASM_MEMORY_READ_PER64_BITS_COST (C_I64Load) is the cost of writing into the WASM linear memory *per 64 bits*.
pub const WASM_MEMORY_READ_PER64_BITS_COST: u64 = 3;
/// WASM_BYTE_CODE_PER_BYTE_COST (C_I64Store) is the cost of checking whether input byte code satisfy CBI.
pub const WASM_BYTE_CODE_PER_BYTE_COST: u64 = 3;

/// Cost of reading `len` bytes from the guest's memory.
pub const fn wasm_memory_read_cost(len: usize) -> u64 {
    let cost = ceil_div_8(len as u64).saturating_mul(WASM_MEMORY_READ_PER64_BITS_COST);
    if cost == 0 {
        return 1;
    } // = max(cost, 1) to make sure charging for a non-zero cost
    cost
}

///Cost of writing `len` bytes into the guest's memory.
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
/// Serialized size of a receipt containing empty command receipts.
pub const MIN_RECP_SIZE: u64 = 4;
/// Serialized size of a minimum-size command receipt.
pub const MIN_CMDRECP_SIZE: u64 = 17;

/// tx_inclusion_cost is the minimum cost for a transaction to be included in the blockchain.
///
/// It consists of:
/// 1. cost for storing transaction in a block
/// 2. cost for storing minimum-sized receipt(s) in a block
/// 3. cost for 5 read-write operations for
///     - signer's nonce
///     - signer's balance during two phases
///     - proposer's balance
///     - treasury's balance
pub fn tx_inclusion_cost(tx_size: usize, commands_len: usize) -> u64 {
    // (1) Transaction storage size
    let tx_size = tx_size as u64;
    // (2) Minimum size of receipt
    let min_receipt_size = minimum_receipt_size(commands_len);
    // (3) Cost for 5 read-write operations
    let rw_key_cost = (
        // Read cost
        get_cost(ACCOUNT_STATE_KEY_LENGTH, 8)
            // Write cost
            .saturating_add(set_cost_write_new_value(8))
            .saturating_add(set_cost_rehash(ACCOUNT_STATE_KEY_LENGTH))
    )
    .saturating_mul(5);

    // Multiply by blockchain storage write cost and add the cost of 5 read-write operations.
    tx_size
        .saturating_add(min_receipt_size)
        .saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST)
        .saturating_add(rw_key_cost)
}

/// Serialized size of a receipt containing `commands_len` minimum-sized command receipts.
pub const fn minimum_receipt_size(commands_len: usize) -> u64 {
    MIN_RECP_SIZE.saturating_add(MIN_CMDRECP_SIZE.saturating_mul(commands_len as u64))
}

/// blockchain_return_values_cost calculates the cost of writing return data into the receipt.
pub const fn blockchain_return_values_cost(data_len: usize) -> u64 {
    // data_len * C_txdata
    (data_len as u64).saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST)
}

/// blockchain_log_cost calculates the cost of writing log into the receipt.
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
pub const ACCOUNT_STATE_KEY_LENGTH: usize = 33;
/// Cost of writing a single byte into the world state.
pub const MPT_WRITE_PER_BYTE_COST: u64 = 2500;
/// Cost of reading a single byte from the world state.
pub const MPT_READ_PER_BYTE_COST: u64 = 50;
/// Cost of traversing 1 byte (2 nibbles) down an MPT.
pub const MPT_TRAVERSE_PER_BYTE_COST: u64 = 20;
/// Cost of traversing 1 byte up (2 nibbles) and recomputing the SHA256 hashes of 2 nodes
/// in an MPT after it or one of its descendants is changed.
pub const MPT_REHASH_PER_BYTE_COST: u64 = 130;
/// Proportion of the cost of writing a tuple into an MPT that is refunded when that tuple is re-set or deleted.
pub const MPT_WRITE_REFUND_PROPORTION: u64 = 50;
/// Proportion of which is discounted if the tuple contains a contract.
pub const MPT_GET_CODE_DISCOUNT_PROPORTION: u64 = 50;

/// get_cost calculates the cost of reading data from the World State.
pub const fn get_cost(key_len: usize, value_len: usize) -> u64 {
    let get_cost_1 = (value_len as u64).saturating_mul(MPT_READ_PER_BYTE_COST);
    let get_cost_2 = (key_len as u64).saturating_mul(MPT_TRAVERSE_PER_BYTE_COST);
    get_cost_1.saturating_add(get_cost_2)
}

/// get_code_cost calculates the cost of reading contract code from the World State.
pub const fn get_code_cost(code_len: usize) -> u64 {
    // Get Cost
    get_cost(ACCOUNT_STATE_KEY_LENGTH, code_len)
        // Code Discount
        .saturating_mul(MPT_GET_CODE_DISCOUNT_PROPORTION)
        .saturating_div(100)
}

/// Set Cost (1): Cost for getting the key.
pub const fn set_cost_read_key(key_len: usize, value_len: usize) -> u64 {
    get_cost(key_len, value_len)
}

/// Set Cost (2): Cost for deleting the old value for a refund.
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

/// Set Cost (3): Cost for writing a new value.
pub const fn set_cost_write_new_value(new_val_len: usize) -> u64 {
    // b * C_write
    (new_val_len as u64).saturating_mul(MPT_WRITE_PER_BYTE_COST)
}

/// Set Cost (4): Cost for recomputing node hashes until the root.
pub const fn set_cost_rehash(key_len: usize) -> u64 {
    // k * C_rehash
    (key_len as u64).saturating_mul(MPT_REHASH_PER_BYTE_COST)
}

/// contains_cost calculates the cost of checking key existence in the World State.
pub const fn contains_cost(key_len: usize) -> u64 {
    (key_len as u64).saturating_mul(MPT_TRAVERSE_PER_BYTE_COST)
}

/* ↓↓↓ Gas Costs for crypto functions ↓↓↓ */

/// Multiplier of computing the SHA256 hash over the length of a message.
pub const CRYPTO_SHA256_PER_BYTE: u64 = 16;
/// Multiplier of computing the Keccak256 hash over the length of a message.
pub const CRYPTO_KECCAK256_PER_BYTE: u64 = 16;
/// Multiplier of computing the RIPEMD160  hash over the length of a message.
pub const CRYPTO_RIPEMD160_PER_BYTE: u64 = 16;
/// Cost of verifying whether an Ed25519 signature over a message of length.
pub const fn crypto_verify_ed25519_signature_cost(msg_len: usize) -> u64 {
    // Base Cost (1400000) + 16 * Message Length
    1_400_000_u64.saturating_add((msg_len as u64).saturating_mul(16_u64))
}
