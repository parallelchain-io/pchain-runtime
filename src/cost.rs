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
        // Constants
        Operator::I32Const {..} | Operator::I64Const {..} => 0,

        // Type parameteric operators
        Operator::Drop => 2,
        Operator::Select => 3,

        // Flow control
        Operator::Nop | Operator::Unreachable | Operator::Loop{..} | Operator::Else | Operator::If {..} 
        => 0,
        Operator::Br{..} | Operator::BrTable{..} | Operator::Return | Operator::Call{..} | Operator::CallIndirect{..}
        => 2,
        Operator::BrIf {..}
        => 3,

        // Registers 
        Operator::GlobalGet {..} | Operator::GlobalSet {..} | Operator::LocalGet {..} | Operator::LocalSet {..}
        => 3,
        
        // Reference Types
        Operator::RefIsNull | Operator::RefFunc {..} | Operator::RefNull{..} | Operator::ReturnCall{..} | Operator::ReturnCallIndirect{..}  
        => 2, 

        // Exception Handling
        Operator::CatchAll | Operator::Throw{..} | Operator::Rethrow{..} | Operator::Delegate{..}
        => 2, 
        
        // Bluk Memory Operations
        Operator::ElemDrop {..} | Operator::DataDrop {..} 
        => 1, 
        Operator::TableInit{..} 
        => 2,
        Operator::MemoryCopy{..} | Operator::MemoryFill{..} | Operator::TableCopy{..} | Operator::TableFill{..} 
        => 3, 

        // Memory Operations 
        Operator::I32Load{..} | Operator::I64Load{..} | Operator::I32Store {..} | Operator::I64Store{..} | 
        Operator::I32Store8{..} | Operator::I32Store16{..} |  Operator::I32Load8S{..} | Operator::I32Load8U{..} | Operator::I32Load16S{..} | Operator::I32Load16U{..} |
        Operator::I64Load8S{..} | Operator::I64Load8U{..} | Operator::I64Load16S{..} | Operator::I64Load16U{..} | Operator::I64Load32S{..} | Operator::I64Load32U{..} |
        Operator::I64Store8{..} | Operator::I64Store16{..} | Operator::I64Store32{..}  
        => 3, 

        // 32 and 64-bit Integer Arithmetic Operations
        Operator::I32Add | Operator::I32Sub | Operator::I64Add | Operator::I64Sub | Operator::I64LtS | Operator::I64LtU |
        Operator::I64GtS| Operator::I64GtU | Operator::I64LeS | Operator::I64LeU | Operator::I64GeS | Operator::I64GeU |
        Operator::I32Eqz | Operator::I32Eq | Operator::I32Ne | Operator::I32LtS | Operator::I32LtU | Operator::I32GtS |
        Operator::I32GtU | Operator::I32LeS | Operator::I32LeU | Operator::I32GeS | Operator::I32GeU | Operator::I64Eqz |
        Operator::I64Eq | Operator::I64Ne | Operator::I32And | Operator::I32Or | Operator::I32Xor | Operator::I64And | 
        Operator::I64Or | Operator::I64Xor
        => 1,
        Operator::I32Shl | Operator::I32ShrU | Operator::I32ShrS | Operator::I32Rotl | Operator::I32Rotr | Operator::I64Shl | 
        Operator::I64ShrU | Operator::I64ShrS | Operator::I64Rotl | Operator::I64Rotr
        => 2,
        Operator::I32Mul | Operator::I64Mul  
        => 3,
        Operator::I32DivS | Operator::I32DivU | Operator::I32RemS | Operator::I32RemU | Operator::I64DivS | Operator::I64DivU | 
        Operator::I64RemS | Operator::I64RemU
        => 80,
        Operator::I32Clz | Operator::I64Clz 
        => 105,

        // Type Casting & Truncation Operations
        Operator::I32WrapI64 | Operator::I64ExtendI32S | Operator::I64ExtendI32U | Operator::I32Extend8S | Operator::I32Extend16S | Operator::I64Extend8S | 
        Operator::I64Extend16S | Operator::I64Extend32S  
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

/// gas defines constants, formulas and functions for gas calculation.
pub mod gas {
    use std::ops::{Add, AddAssign, Sub, SubAssign};

    use pchain_types::{Transaction, Serializable};

    pub const MIN_RECP_SIZE: u64 = 4;
    pub const MIN_CMDRECP_SIZE: u64 = 17;
    pub const ACCOUNT_STATE_KEY_LENGTH: u64 = 33;

    /// tx_inclusion_cost is the minimum cost for a transaction to be included in the blockchain.
    /// It basically calls [tx_inclusion_cost_from] to calculate the cost by passing size of serialized
    /// transaction and the lenght of its commands.
    pub fn tx_inclusion_cost(tx: &Transaction) -> u64 {
        tx_inclusion_cost_from(tx.serialize().len(), tx.commands.len())
    }
    
    /// tx_inclusion_cost is the minimum cost for a transaction to be included in the blockchain.
    /// It consists of:
    /// 1. cost for storing transaction in a block
    /// 2. cost for storing minimum-sized receipt(s) in a block
    /// 3. cost for 5 read-write operations for 
    ///     - signer's nonce
    ///     - signer's balance during two phases
    ///     - proposer's balance
    ///     - treasury's balance
    pub fn tx_inclusion_cost_from(tx_size: usize, commands_len: usize) -> u64 {
        let tx_size = tx_size as u64 ;
        let min_receipt_size = MIN_RECP_SIZE.saturating_add(
            MIN_CMDRECP_SIZE.saturating_mul(commands_len as u64)
        );
        let rw_key_cost = (
            read_cost(ACCOUNT_STATE_KEY_LENGTH as usize, 8) 
            + write_cost(ACCOUNT_STATE_KEY_LENGTH as usize, 8, 8)
        )
        .deduct.saturating_mul(5);

        tx_size.saturating_add(min_receipt_size).saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST).saturating_add(rw_key_cost)
    }

    /// BLOCKCHAIN_WRITE_PER_BYTE_COST (C_txdata) is the cost of writes to the blockchain transaction data per byte.
    pub const BLOCKCHAIN_WRITE_PER_BYTE_COST: u64 = 30;

    /// MPT_READ_PER_BYTE_COST (C_read) is the cost of reading a single byte from the world state.
    pub const MPT_READ_PER_BYTE_COST: u64 = 50;

    /// MPT_WRITE_PER_BYTE_COST (C_write) is the cost of writing a single byte into the world state.
    pub const MPT_WRITE_PER_BYTE_COST: u64 = 2500;

    /// MPT_TRAVERSE_PER_BYTE (C_traverse) is the cost of traversing 1 byte (2 nibbles) down a MPT.
    pub const MPT_TRAVERSE_PER_BYTE_COST: u64 = 20;

    /// MPT_REHASH_PER_BYTE (C_rehash) is the cost of traversing 1 byte up (2 nibbles) and recomputing 
    /// the SHA256 hashes of 2 nodes in an MPT after it or one of its descendants is changed.
    pub const MPT_REHASH_PER_BYTE_COST: u64 = 130;

    /// MPT_WRITE_REFUND_PROPORTION (C_refund in percentage) is the proportion of the cost of writing 
    /// a tuple into an MPT that is refunded when that tuple is re-set or deleted.
    pub const MPT_WRITE_REFUND_PROPORTION: u64 = 50;

    /// MPT_GET_CODE_DISCOUNT_PROPORTION (C_contractdisc in percentage) is the proportion of the cost of reading 
    /// a tuple from the world state which is discounted if the tuple contains a contract.
    pub const MPT_GET_CODE_DISCOUNT_PROPORTION: u64 = 50;

    /// WASM_MEMORY_WRITE_PER64_BITS_COST is the cost of writing into the WASM linear memory *per 64 bits*.
    pub const WASM_MEMORY_WRITE_PER64_BITS_COST: u64 = 3;

    /// WASM_MEMORY_READ_PER64_BITS_COST (C_I64Load) is the cost of writing into the WASM linear memory *per 64 bits*.
    pub const WASM_MEMORY_READ_PER64_BITS_COST: u64 = 3;

    /// WASM_BYTE_CODE_PER_BYTE_COST (C_I64Store) is the cost of checking whether input byte code satisfy CBI.
    pub const WASM_BYTE_CODE_PER_BYTE_COST: u64 = 3;

    /// contains_cost calculates the cost of checking key existence in the World State
    pub const fn contains_cost(key_len: usize) -> CostChange {
        CostChange::deduct((key_len as u64).saturating_mul(MPT_TRAVERSE_PER_BYTE_COST))
    }

    /// read_code_cost calculates the cost of reading contract code from the World State
    pub const fn read_code_cost(code_len: usize) -> CostChange {
        let key_len = ACCOUNT_STATE_KEY_LENGTH;
        let code_len = code_len as u64;
        
        CostChange::deduct(
            // Read Cost
            code_len.saturating_mul(MPT_READ_PER_BYTE_COST).saturating_add((key_len).saturating_mul(MPT_TRAVERSE_PER_BYTE_COST))
            // Code Discount
            .saturating_mul(MPT_GET_CODE_DISCOUNT_PROPORTION).saturating_div(100)
        )
    }

    /// read_cost calculates the cost of reading data from the World State 
    pub const fn read_cost(key_len : usize, value_len: usize) -> CostChange {
        let key_len = key_len as u64;
        let value_len = value_len as u64;
        CostChange::deduct(value_len.saturating_mul(MPT_READ_PER_BYTE_COST).saturating_add(key_len.saturating_mul(MPT_TRAVERSE_PER_BYTE_COST)))
    }

    /// write_cost calculates the cost of writing data into the World State
    #[allow(clippy::double_comparisons)]
    pub fn write_cost(key_len: usize, old_val_len: usize, new_val_len: usize) -> CostChange {
        let key_len = key_len as u64;
        let old_val_len = old_val_len as u64;
        let new_val_len = new_val_len as u64;

        // (1) Get cost should be already charged.
        // (2):
        let cost =
        if (old_val_len > 0 || old_val_len == 0) && new_val_len > 0 {
            // a * C_write * C_refund 
            CostChange::reward(old_val_len.saturating_mul(MPT_WRITE_PER_BYTE_COST * MPT_WRITE_REFUND_PROPORTION).saturating_div(100))
        } else if old_val_len > 0 && new_val_len == 0 {
            // (k + a) * C_write * C_refund 
            CostChange::reward((key_len.saturating_add(old_val_len)).saturating_mul(MPT_WRITE_PER_BYTE_COST * MPT_WRITE_REFUND_PROPORTION).saturating_div(100))    
        } else { // old_val_len == 0 && new_val_len == 0
            CostChange::reward(0)
        };
        cost +
        // (3) b * C_write
        CostChange::deduct(new_val_len.saturating_mul(MPT_WRITE_PER_BYTE_COST)) +
        // (4) k * C_rehash
        CostChange::deduct(key_len.saturating_mul(MPT_REHASH_PER_BYTE_COST))
    }

    /// blockchain_return_value_cost calculates the cost of writing return data into the receipt
    pub const fn blockchain_return_value_cost(data_len: usize) -> CostChange {
        // data_len * C_txdata
        CostChange::deduct((data_len as u64).saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST))
    }

    /// blockchain_log_cost calculates the cost of writing log into the receipt
    pub const fn blockchain_log_cost(topic_len: usize, val_len: usize) -> CostChange {
        let topic_len = topic_len as u64;
        let val_len = val_len as u64;
        let log_len = topic_len.saturating_add(val_len);
        CostChange::deduct(
            // Ceil(l/8) * C_wasmread
            (ceil_div_8(log_len).saturating_mul(WASM_MEMORY_READ_PER64_BITS_COST))
            // t * C_sha256
            .saturating_add(topic_len.saturating_mul(crate::cost::CRYPTO_SHA256_PER_BYTE))
            // l X Z
            .saturating_add(log_len.saturating_mul(BLOCKCHAIN_WRITE_PER_BYTE_COST))
        )
    }

    pub const fn wasm_memory_read_cost(val_len: usize) -> CostChange {
        let mut cost = ceil_div_8(val_len as u64).saturating_mul(WASM_MEMORY_READ_PER64_BITS_COST);
        if cost == 0 { cost = 1; } // = max(cost, 1) to make sure charging for a non-zero cost 
        CostChange::deduct(cost) 
    }

    pub const fn wasm_memory_write_cost(val_len: usize) -> CostChange {
        let mut cost = ceil_div_8(val_len as u64).saturating_mul(WASM_MEMORY_WRITE_PER64_BITS_COST);
        if cost == 0 { cost = 1; } // = max(cost, 1) to make sure charging for a non-zero cost 
        CostChange::deduct(cost) 
    }

    /// Ceiling of the value after dividing by 8 
    pub const fn ceil_div_8(l: u64) -> u64 {
        l.saturating_add(7).saturating_div(8)
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
