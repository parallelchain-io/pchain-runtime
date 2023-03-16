/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! types defines data structures alias to types in [pchain_types::Transaction].

use std::ops::{Deref, DerefMut};

use pchain_types::{PublicAddress, Sha256Hash, Signature, Transaction};

/// BaseTx consists of common fields inside [pchain_types::Transaction].
#[derive(Clone)]
pub struct BaseTx {
    pub signer: PublicAddress,
    pub hash: Sha256Hash,
    pub signature: Signature,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_base_fee_per_gas: u64,
    pub priority_fee_per_gas: u64,   
}

impl From<&Transaction> for BaseTx {
    fn from(tx: &Transaction) -> Self {
        Self {
            signer: tx.signer, 
            hash: tx.hash, 
            signature: tx.signature, 
            nonce: tx.nonce, 
            gas_limit: tx.gas_limit, 
            max_base_fee_per_gas: tx.max_base_fee_per_gas, 
            priority_fee_per_gas: tx.priority_fee_per_gas 
        }
    }
}

/// CallTx is a struct representation of [pchain_types::Command::Call].
#[derive(Clone)]
pub struct CallTx {
    pub base_tx: BaseTx,
    pub target: PublicAddress,
    pub method: String,
    pub arguments: Option<Vec<Vec<u8>>>,
    pub amount: Option<u64>
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