/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct that serves as temporary store for Command Receipts.
use pchain_types::blockchain::{
    CommandReceiptV1, CommandReceiptV2, ExitCodeV2, ReceiptV1, ReceiptV2,
};

use crate::types::{self, CommandKind};

/// Store the results of execution of a Command
#[derive(Default)]
pub(crate) struct CommandReceiptCache<E> {
    cmd_rcps: Vec<E>,
}

impl<E> CommandReceiptCache<E> {
    pub fn new() -> Self {
        Self {
            cmd_rcps: Vec::new(),
        }
    }
}

pub(crate) trait ReceiptCacher<E, R> {
    fn push_command_receipt(&mut self, command_receipt: E);

    fn push_deferred_command_receipt(&mut self, command_receipt: E);

    fn into_receipt(self, gas_used: u64, commands: &[CommandKind]) -> R;
}

impl ReceiptCacher<CommandReceiptV1, ReceiptV1> for CommandReceiptCache<CommandReceiptV1> {
    fn push_command_receipt(&mut self, command_receipt: CommandReceiptV1) {
        self.cmd_rcps.push(command_receipt)
    }

    /// Combine the information from next Command Receipt.
    /// Assumption: execution of a deferred command will not spawn non-deferred command.
    fn push_deferred_command_receipt(&mut self, command_receipt: CommandReceiptV1) {
        if let Some(last_command_receipt) = self.cmd_rcps.last_mut() {
            last_command_receipt.gas_used = last_command_receipt
                .gas_used
                .saturating_add(command_receipt.gas_used);
            last_command_receipt.exit_code = command_receipt.exit_code;
            last_command_receipt.return_values = command_receipt.return_values;
        }
    }

    fn into_receipt(self, _gas_used: u64, _commands: &[CommandKind]) -> ReceiptV1 {
        self.cmd_rcps
    }
}

impl ReceiptCacher<CommandReceiptV2, ReceiptV2> for CommandReceiptCache<CommandReceiptV2> {
    fn push_command_receipt(&mut self, command_receipt: CommandReceiptV2) {
        self.cmd_rcps.push(command_receipt);
    }

    /// Combine the information from next Command Receipt.
    /// Assumption: execution of a deferred command will not spawn non-deferred command.
    fn push_deferred_command_receipt(&mut self, command_receipt: CommandReceiptV2) {
        if let Some(last_command_receipt) = self.cmd_rcps.last_mut() {
            let (last_command_receipt_gas_used, _) =
                types::gas_used_and_exit_code_v2(last_command_receipt);
            let (gas_used, exit_code) = types::gas_used_and_exit_code_v2(&command_receipt);
            types::set_gas_used_and_exit_code_v2(
                last_command_receipt,
                // Accumulate Gas Used
                last_command_receipt_gas_used.saturating_add(gas_used),
                // Overide Exit Code
                exit_code,
            );
            // Overide return_value
            if let CommandReceiptV2::Call(last_call_receipt) = last_command_receipt {
                match command_receipt {
                    CommandReceiptV2::Call(receipt) => {
                        last_call_receipt.return_value = receipt.return_value;
                    }
                    CommandReceiptV2::WithdrawDeposit(receipt) => {
                        last_call_receipt.return_value =
                            receipt.amount_withdrawn.to_le_bytes().to_vec();
                    }
                    CommandReceiptV2::StakeDeposit(receipt) => {
                        last_call_receipt.return_value =
                            receipt.amount_staked.to_le_bytes().to_vec();
                    }
                    CommandReceiptV2::UnstakeDeposit(receipt) => {
                        last_call_receipt.return_value =
                            receipt.amount_unstaked.to_le_bytes().to_vec();
                    }
                    _ => {}
                }
            }
        }
    }

    fn into_receipt(mut self, gas_used: u64, command_kinds: &[CommandKind]) -> ReceiptV2 {
        let num_executed = self.cmd_rcps.len();
        let num_cmds = command_kinds.len();

        if num_cmds == 0 && num_executed == 0 {
            return ReceiptV2 {
                gas_used,
                exit_code: ExitCodeV2::Ok,
                command_receipts: self.cmd_rcps,
            };
        }

        // receipt's exit code is the exit code of last executed command receipt
        let (_, exit_code) = types::gas_used_and_exit_code_v2(self.cmd_rcps.last().unwrap());

        if num_executed < num_cmds {
            // fill in missing command receipts with a NotExecuted exit code
            self.cmd_rcps.extend(
                command_kinds[num_executed..]
                    .iter()
                    .map(|command_kind| types::create_not_executed_cmd_rcp_v2(command_kind)),
            );
        }

        ReceiptV2 {
            gas_used,
            exit_code,
            command_receipts: self.cmd_rcps,
        }
    }
}
