/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Temporary store for outputs from processing a single command.
//!
//! Used in the [GasMeter](crate::gas::GasMeter) and [HostFuncGasMeter](crate::gas::HostFuncGasMeter).
use crate::types::CommandOutput;
use pchain_types::blockchain::Log;

/// CommandOutputCache is compatible with the return fields of both CommandReceiptV1 and CommandReceiptV2.
#[derive(Clone, Default)]
pub(crate) struct CommandOutputCache {
    /// stores the list of event logs from exeuting a command, ordered by the sequence of emission
    pub logs: MaybeUnused<Vec<Log>>,

    /// value returned by a call transaction using the `return_value` SDK function.
    /// It is None if the execution did not return anything.
    pub return_value: MaybeUnused<Vec<u8>>,

    /// value returned from result of WithdrawDeposit command.
    pub amount_withdrawn: MaybeUnused<u64>,

    /// value returned from result of StakeDeposit command.
    pub amount_staked: MaybeUnused<u64>,

    /// value returned from result of UnstakeDeposit command.
    pub amount_unstaked: MaybeUnused<u64>,
}

impl CommandOutputCache {
    /// retrieves all the values from the cache, emptying the cache.
    pub fn take(&mut self) -> CommandOutput {
        CommandOutput {
            logs: self.logs.take_or_default(),
            return_value: self.return_value.take_or_default(),
            amount_withdrawn: self.amount_withdrawn.take_or_default(),
            amount_staked: self.amount_staked.take_or_default(),
            amount_unstaked: self.amount_unstaked.take_or_default(),
        }
    }

    // used to retrieve the return value from child contracts during a cross-contract call
    pub fn take_return_value(&mut self) -> Option<Vec<u8>> {
        self.return_value.take()
    }
}

/// This struct is defaulted with `T:default`
/// when `as_mut` is called for the first time.
#[derive(Clone, Default)]
pub(crate) struct MaybeUnused<T>(Option<T>)
where
    T: Default;

impl<T> MaybeUnused<T>
where
    T: Default,
{
    pub fn as_mut(&mut self) -> &mut T {
        self.0.get_or_insert_with(T::default)
    }

    pub fn take(&mut self) -> Option<T> {
        self.0.take()
    }

    pub fn take_or_default(&mut self) -> T {
        self.0
            .take()
            .map_or_else(T::default, std::convert::identity)
    }
}
