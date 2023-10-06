/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::blockchain::{CommandReceiptV1, ReceiptV1};

/// Store the results of execution of a Command, which can combine
/// the Command Receipt from result of deferred commands.
/// - Gas used is added up by the later command receipt
/// - Exit status is overwritten by the later command receipt (i.e. if the last command fails, the exit status should also be failed.)
/// - Return value is overwritten by the later command receipt
#[derive(Default)]
pub(crate) struct ReceiptCache(Vec<CommandReceiptV1>);

impl ReceiptCache {
    pub fn push_command_receipt(&mut self, command_receipt: CommandReceiptV1) {
        self.0.push(command_receipt);
    }

    /// Combine the information from next Command Receipt.
    /// Assumption: execution of a deferred command will not spawn non-deferred command.
    pub fn push_deferred_command_receipt(&mut self, command_receipt: CommandReceiptV1) {
        if let Some(last_command_receipt) = self.0.last_mut() {
            last_command_receipt.gas_used = last_command_receipt
                .gas_used
                .saturating_add(command_receipt.gas_used);
            last_command_receipt.exit_code = command_receipt.exit_code;
            last_command_receipt.return_values = command_receipt.return_values;
        }
    }
}

impl From<ReceiptCache> for ReceiptV1 {
    fn from(value: ReceiptCache) -> Self {
        value.0
    }
}
