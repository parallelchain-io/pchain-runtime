/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::blockchain::Log;

use crate::types::CommandOutput;

#[derive(Clone, Default)]
pub(crate) struct CommandOutputCache {

    /// stores the list of events from exeuting a command, ordered by the sequence of emission
    pub(in crate::execution) logs: MaybeUnused<Vec<Log>>,

    /// value returned by a call transaction using the `return_value` SDK function.
    /// It is None if the execution has not/did not return anything.
    pub(in crate::execution) return_values: MaybeUnused<Vec<u8>>,
    
    /// value returned from result of WithdrawDeposit command.
    pub(in crate::execution) amount_withdrawn: MaybeUnused<u64>,

    /// value returned from result of StakeDeposit command.
    pub(in crate::execution) amount_staked: MaybeUnused<u64>,

    /// value returned from result of UnstakeDeposit command.
    pub(in crate::execution) amount_unstaked: MaybeUnused<u64>,
}

impl CommandOutputCache {
    pub fn take(&mut self) -> CommandOutput {
        CommandOutput {
           logs: self.logs.take_or_default(),
           return_values: self.return_values.take_or_default(),
           amount_withdrawn: self.amount_withdrawn.take_or_default(),
           amount_staked: self.amount_staked.take_or_default(),
           amount_unstaked: self.amount_unstaked.take_or_default()
        }
    }

    // Used in cross-contract call
    pub fn take_return_values(&mut self) -> Option<Vec<u8>> {
        self.return_values.take()
    }
}

/// This struct is considered as "Used" when method `as_mut` is called, which initializes default value for
/// the data `T`.
#[derive(Clone, Default)]
pub(crate) struct MaybeUnused<T>(Option<T>) where T: Default;

impl<T> MaybeUnused<T> where T: Default {
    pub fn as_mut(&mut self) -> &mut T {
        if self.0.is_none() {
            self.0 = Some(T::default());
        }
        self.0.as_mut().unwrap()
    }

    pub fn take(&mut self) -> Option<T> {
        self.0.take()
    }

    pub fn take_or_default(&mut self) -> T {
        self.0.take().map_or(T::default(), std::convert::identity)
    }
}