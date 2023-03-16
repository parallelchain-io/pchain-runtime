/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! transactions defines implementation of transition functions. The transition function basically
//! [execute]s sequence of commands across [phase]s: Tentative Charge -> Work -> Charge. The Work 
//! step(s) involves [account] Transaction and [network] Transaction. Account Transaction might 
//! involves [internal] transactions which happens inside a contract call. The transition function also
//! defines execution logic for [administration] commands.

pub(crate) mod account;

pub(crate) mod administration;

pub(crate) mod execute;

pub(crate) mod internal;

pub(crate) mod network;

pub(crate) mod phase;