/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::blockchain::Log;

#[derive(Clone, Default)]
pub(crate) struct CommandOutputCache {
    /// stores the list of events from exeuting a command, ordered by the sequence of emission
    pub(in crate::execution) logs: Vec<Log>,

    /// value returned by a call transaction using the `return_value` SDK function.
    /// It is None if the execution has not/did not return anything.
    pub(in crate::execution) return_values: Option<Vec<u8>>,
    
    // TODO - Support V2 output
}

impl CommandOutputCache {
    pub fn take(&mut self) -> (Vec<Log>, Vec<u8>) {
        let logs = self.take_logs();
        let return_values = self
            .take_return_values()
            .map_or(Vec::new(), std::convert::identity);
        (logs, return_values)
    }

    pub fn take_logs(&mut self) -> Vec<Log> {
        std::mem::take(&mut self.logs)
    }

    pub fn take_return_values(&mut self) -> Option<Vec<u8>> {
        self.return_values.take()
    }
}
