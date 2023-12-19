/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Describes the business logic for executing individual [Commands](pchain_types::blockchain::Command).
//!
//! These Commands are included in a [TransactionV1](pchain_types::blockchain::TransactionV1)
//! or [TransactionV2](pchain_types::blockchain::TransactionV2).
//!
//! There are three categories of Commands:
//! - [Account](account) Commands that modify the state inside user or contract accounts.
//! - [Staking](staking) Commands that modify the state relating to users' deposits and stakes.
//! - [Protocol](protocol) Commands that modify the protocol state.

pub(crate) mod account;

pub(crate) mod protocol;

pub(crate) mod staking;
