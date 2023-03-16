/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! cost defines the cost functions for executing wasm byte code.

use wasmer::wasmparser::Operator;

/// wasm_cost_function maps between a WASM Operator to the cost of executing it.
/// Only EToC transaction contains this gas fee for the smart contract function call.
/// This function is called for each `Operator` encountered during the Wasm module execution. 
/// Return: The latency of the x86-64 Instructions each opcode is translated into by the Wasmer compiler 
pub(crate) fn wasm_cost_function(operator: &Operator) -> u64 {
    match operator {
        // Bulk memory operations, reference types and exception handling
        Operator::Nop | Operator::Unreachable | Operator::Loop{..} | Operator::Else | Operator::If {..} | Operator::I32Const {..} | Operator::I64Const {..} => 0,
        Operator::Br{..} | Operator::BrTable{..} | Operator::Return | Operator::Call{..} | Operator::CallIndirect{..} |
        Operator::ReturnCall{..} | Operator::ReturnCallIndirect{..} | Operator::Drop | Operator::Throw{..} | Operator::Rethrow{..} | Operator::Delegate{..} |
        Operator::CatchAll | Operator::TableInit{..} | Operator::RefNull{..} | Operator::RefIsNull | Operator::RefFunc{..} => 2,         
        Operator::Select | Operator::BrIf {..} | Operator::GlobalGet {..} | Operator::GlobalSet {..} |
        Operator::MemoryCopy{..} | Operator::MemoryFill{..} | Operator::TableCopy{..} | Operator::TableFill{..}  => 3, 

        // Integer Memory Operations 
        Operator::I32Load8S{..} | Operator::I32Load8U{..} | Operator::I32Load16S{..} | Operator::I32Load16U{..} | Operator::I32Load{..} |
        Operator::I64Load8S{..} | Operator::I64Load8U{..} | Operator::I64Load16S{..} | Operator::I64Load16U{..} | 
        Operator::I64Load32S{..} | Operator::I64Load32U{..} | Operator::I64Load{..} | Operator::I32Store{..} | Operator::I64Store{..} |  
        Operator::I32Store8{..} | Operator::I32Store16{..} | Operator::I64Store8{..} | Operator::I64Store16{..} | Operator::I64Store32{..}  
        => 3, 

        // Integer Arithmetic Operations 
        Operator::I32Shl | Operator::I32ShrU | Operator::I32ShrS | Operator::I32Rotl | Operator::I32Rotr | Operator::I64Shl | Operator::I64ShrU | Operator::I64ShrS | Operator::I64Rotl | 
        Operator::I64Rotr => 2,
        Operator::I32Mul | Operator::I64Mul  => 3,

        Operator::I32DivS | Operator::I32DivU | Operator::I32RemS | Operator::I32RemU | Operator::I64DivS | Operator::I64DivU | Operator::I64RemS | Operator::I64RemU => 80,
        Operator::I32Clz | Operator::I64Clz => 105,

        // Integer Type Casting & Truncation Operations 
        Operator::I32WrapI64 | Operator::I64ExtendI32S | Operator::I64ExtendI32U | 
        Operator::I32Extend8S | Operator::I32Extend16S | Operator::I64Extend8S | Operator::I64Extend16S | Operator::I64Extend32S  
        => 3,

        // Everything Else is 1 
        _ => 1,
    }
}

/// Gas Costs for crypto functions
// CRYPTO_SHA256_PER_BYTE is the cost of executing the hash function SHA256 on data per byte.
pub const CRYPTO_SHA256_PER_BYTE: u64 = 16;
// CRYPTO_KECCAK256_PER_BYTE is the cost of executing the hash function KECCAK256 on data per byte.
pub const CRYPTO_KECCAK256_PER_BYTE: u64 = 16;
// CRYPTO_RIPEMD160_PER_BYTE is the cost of executing the hash function RIPEMD160 on data per byte.
pub const CRYPTO_RIPEMD160_PER_BYTE: u64 = 16;
// crypto_verify_ed25519_signature_cost is the cost of verifying signature signed by ed22519 key.
pub const fn crypto_verify_ed25519_signature_cost(msg_len: usize) -> u64 {
    // Base Cost (1400000) + 16 * Message Length
    1_400_000_u64.saturating_add((msg_len as u64).saturating_mul(16_u64))
}

/// All transactions pay (at least) a TX_BASE_GAS corresponding to two sets of storage operations:
///
/// 1. Reading and then writing of 4 world state keys (this happens in the course of every transaction):
/// - nonce
/// - from account balance
/// - validator balance
///
/// 2. Writing of transaction data in a block:
pub mod gas {
    use std::ops::{Add, AddAssign, Sub, SubAssign};

    pub const TX_BASE_SIZE: u64 = 201;
    pub const RECEIPT_BASE_SIZE: u64 = 9;
    pub const ACCOUNT_STATE_KEY_LENGTH: u64 = 33;
    /// LEAF_NODE_BASE_LENGTH (X) is the cost added to the key length in Write Gas Calculation
    pub const LEAF_NODE_BASE_LENGTH: u64 = 150;

    /// TX_BASE_COST is thr base cost of executing the ’simplest’ possible Transaction that can be included in a block.
    pub fn tx_base_cost() -> u64 {
        ((TX_BASE_SIZE + RECEIPT_BASE_SIZE) * BLOCKCHAIN_WRITE_PER_BYTE_COST )
            .saturating_add( 
            (
                read_cost(ACCOUNT_STATE_KEY_LENGTH as usize, 8) 
                + write_cost(ACCOUNT_STATE_KEY_LENGTH as usize, 8, 8)
            )
            .deduct.saturating_mul(4)
        )
    }

    /// BLOCKCHAIN_DATA_BYTE is the cost of writes to the blockchain transaction data per byte.
    pub const BLOCKCHAIN_WRITE_PER_BYTE_COST: u64 = 30;

    /// MPT_TRAVERSE_PER_NIBBLE_COST (T) is the cost to compute the hash of the encoding of a nibble in the modified MPT. 
    pub const MPT_TRAVERSE_PER_NIBBLE_COST: u64 = 10;

    /// MPT_HASH_COMPUTE_PER_NIBBLE_COST (H) is the cost to traverse to next/previous nibble in the modified MPT 
    pub const MPT_HASH_COMPUTE_PER_NIBBLE_COST: u64 = 55;

    /// MPT_READ_PER_BYTE_COST (R) is the cost of reading the World State *per byte*.
    pub const MPT_READ_PER_BYTE_COST: u64 = 100;

    /// MPT_WRITE_PER_BYTE_COST (W) is the cost of writing into the World State *per byte*.
    pub const MPT_WRITE_PER_BYTE_COST: u64 = 1250;

    /// MPT_WRITE_REFUND_PROPORTION (Z) is the refund proportion frees the storage.
    pub const MPT_WRITE_REFUND_PROPORTION: u64 = 50;

    // MPT_GET_CODE_DISCOUNT (D) is the discount for the reading cost of contract byte code from world state.
    pub const MPT_GET_CODE_DISCOUNT: u64 = 50;

    /// WASM_MEMORY_WRITE_PER64_BITS_COST is the cost of writing into the WASM linear memory *per 64 bits*.
    pub const WASM_MEMORY_WRITE_PER64_BITS_COST: u64 = 3;

    /// WASM_MEMORY_READ_PER64_BITS_COST is the cost of writing into the WASM linear memory *per 64 bits*.
    pub const WASM_MEMORY_READ_PER64_BITS_COST: u64 = 3;

    /// WASM_BYTE_CODE_PER_BYTE_COST is the cost of checking whether input byte code satisfy CBI.
    pub const WASM_BYTE_CODE_PER_BYTE_COST: u64 = 100;

    // LOGICAL_OR_PER64_BITS_COST is the cost of calculating logical or between two variable *per 64 bits*.
    pub const LOGICAL_OR_PER64_BITS_COST: u64 = 1;

    /// contains_cost calculates the cost of checking key existence in the World State
    pub const fn contains_cost(key_len: usize) -> CostChange {
        let key_len = key_len as u64;
        CostChange::deduct(key_len.saturating_mul(2 * MPT_TRAVERSE_PER_NIBBLE_COST))
    }

    /// read_code_cost calculates the cost of reading contract code from the World State
    pub const fn read_code_cost(code_len: usize) -> CostChange {
        let code_len = code_len as u64;
        CostChange::deduct(
            // Read Cost + Cost_contains
            (code_len.saturating_mul(MPT_READ_PER_BYTE_COST).saturating_add(2 * ACCOUNT_STATE_KEY_LENGTH * MPT_TRAVERSE_PER_NIBBLE_COST)) 
            // Code Discount
            .saturating_mul(MPT_GET_CODE_DISCOUNT).saturating_div(100)
        )
    }

    /// read_cost calculates the cost of reading data from the World State 
    pub const fn read_cost(key_len : usize, value_len: usize) -> CostChange {
        let key_len = key_len as u64;
        let value_len = value_len as u64;
        CostChange::deduct(value_len.saturating_mul(MPT_READ_PER_BYTE_COST).saturating_add(key_len.saturating_mul( 2 * MPT_TRAVERSE_PER_NIBBLE_COST )))
    }

    /// write_cost calculates the cost of writing data into the World State
    pub fn write_cost(key_len: usize, old_val_len: usize, new_val_len: usize) -> CostChange {
        let key_len = key_len as u64;
        let old_val_len = old_val_len as u64;
        let new_val_len = new_val_len as u64;

        if old_val_len == 0 && new_val_len > 0 {
            CostChange::deduct((key_len.saturating_add(new_val_len).saturating_add(LEAF_NODE_BASE_LENGTH)).saturating_mul(MPT_WRITE_PER_BYTE_COST)) + 
            CostChange::deduct(key_len.saturating_mul( 2 * (MPT_TRAVERSE_PER_NIBBLE_COST + MPT_HASH_COMPUTE_PER_NIBBLE_COST)))
        } else if old_val_len > 0 && new_val_len > 0 {
            CostChange::deduct(new_val_len.saturating_mul(MPT_WRITE_PER_BYTE_COST)) +
            CostChange::reward(old_val_len.saturating_mul(MPT_WRITE_REFUND_PROPORTION * MPT_WRITE_PER_BYTE_COST).saturating_div(100)) +
            CostChange::deduct(key_len.saturating_mul( 2 * (MPT_TRAVERSE_PER_NIBBLE_COST + MPT_HASH_COMPUTE_PER_NIBBLE_COST)))
        } else if old_val_len > 0 && new_val_len == 0 {
            CostChange::reward((key_len.saturating_add(old_val_len).saturating_add(LEAF_NODE_BASE_LENGTH)).saturating_mul(MPT_WRITE_PER_BYTE_COST * MPT_WRITE_REFUND_PROPORTION).saturating_div(100)) + 
            CostChange::deduct(key_len.saturating_mul( 2 * (MPT_TRAVERSE_PER_NIBBLE_COST + MPT_HASH_COMPUTE_PER_NIBBLE_COST)))
        } else {
            CostChange::deduct(0)
        }
    }

    /// blockchain_txreceipt_cost calculates the cost of writing blockchain data into the storage
    pub const fn blockchain_txreceipt_cost(data_len: usize) -> CostChange {
        let data_len = data_len as u64;
        CostChange::deduct(data_len.saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST))
    }

    /// blockchain_txlog_cost calculates the cost of writing log into the storage
    pub const fn blockchain_txlog_cost(topic_len: usize, val_len: usize) -> CostChange {
        let topic_len = topic_len as u64;
        let val_len = val_len as u64;
        let log_len = topic_len.saturating_add(val_len);
        CostChange::deduct(
            // Ceil(l/8) * W
            (log_len.saturating_add(7).saturating_div(8).saturating_mul(WASM_MEMORY_READ_PER64_BITS_COST))
            // t * X
            .saturating_add(topic_len.saturating_mul(crate::cost::CRYPTO_SHA256_PER_BYTE))
            // 256 * Y / 64
            .saturating_add(256 * LOGICAL_OR_PER64_BITS_COST / 64)
            // l X Z
            .saturating_add(log_len.saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST))
        )
    }

    pub const fn wasm_memory_read_cost(val_len: usize) -> CostChange {
        let val_len = val_len as u64;
        CostChange::deduct( val_len.saturating_add(7).saturating_div(8).saturating_mul(WASM_MEMORY_READ_PER64_BITS_COST) ) 
    }

    pub const fn wasm_memory_write_cost(val_len: usize) -> CostChange {
        let val_len = val_len as u64;
        CostChange::deduct( val_len.saturating_add(7).saturating_div(8).saturating_mul(WASM_MEMORY_WRITE_PER64_BITS_COST) ) 
    }

    /// CostChange defines gas cost change by adding or substrating value to the total gas.
    /// 
    /// ### Example:
    /// ```no_run
    /// let mut change = CostChange::default(); // = 0
    /// change += CostChange::reward(1); // = 1
    /// change += CostChange::deduct(2); // = -1
    /// assert_eq!(change.values().0, 1);
    /// ```
    #[derive(Clone, Copy, Debug, Default)]
    pub struct CostChange {
        deduct: u64,
        reward: u64
    }

    impl CostChange {
        pub const fn deduct(value: u64) -> Self { Self { deduct: value, reward: 0 }}
        pub const fn reward(value: u64) -> Self { Self { deduct: 0, reward: value }}
        pub fn value(&self) -> i128 {
            if self.deduct < self.reward {
                self.reward as i128 - self.deduct as i128
            } else {
                self.deduct as i128 - self.reward as i128
            }
        }
        pub fn values(&self) -> (u64, u64) {
            (
                self.deduct.saturating_sub(self.reward),
                self.reward.saturating_sub(self.deduct),
            )
        }
    }

    impl AddAssign for CostChange {
        fn add_assign(&mut self, rhs: Self) {
            self.deduct = self.deduct.saturating_add(rhs.deduct);
            self.reward = self.reward.saturating_add(rhs.reward);
        }
    }


    impl Add for CostChange {
        type Output = Self;
        fn add(self, other:Self) -> Self {
            Self {
                deduct: self.deduct.saturating_add(other.deduct),
                reward: self.reward.saturating_add(other.reward)
            }
        }
    }

    impl SubAssign for CostChange {
        fn sub_assign(&mut self, rhs: Self) {
            let v = self.sub(rhs);
            *self = v;
        }
    }

    impl Sub for CostChange {
        type Output = Self;
        fn sub(self, other: Self) -> Self {
            let net_deduct = other.deduct.saturating_sub(self.deduct);
            let net_reward = other.reward.saturating_sub(self.reward);
            Self {
                deduct: self.deduct.saturating_sub(other.deduct) + net_reward,
                reward: self.reward.saturating_sub(other.reward) + net_deduct
            }
        }
    }
}
