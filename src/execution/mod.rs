/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Implementation of state transition functions. 
//! 
//! The transition function basically [executes](execute) sequence of commands across [phases](phase): 
//! Pre-Charge -> Command(s) -> Charge. The Commands to execute includes [Account](account) Command,
//! [Staking](staking) Command and [Protocol](protocol) Command. 
//! During execution of Account Command, [internal] transactions can happens inside a [contract] call.

pub mod account;

pub mod contract;

pub mod execute;

pub mod internal;

pub mod staking;

pub mod phase;

pub mod protocol;

pub mod state;