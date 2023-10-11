/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

use pchain_types::blockchain::{CommandReceiptV1, ReceiptV1, CommandReceiptV2, ReceiptV2, ExitCodeV2, TransferReceipt, DeployReceipt, CallReceipt, CreatePoolReceipt, SetPoolSettingsReceipt, DeletePoolReceipt, CreateDepositReceipt, SetDepositSettingsReceipt, TopUpDepositReceipt, WithdrawDepositReceipt, StakeDepositReceipt, UnstakeDepositReceipt, NextEpochReceipt, ExitCodeV1};

use crate::types::CommandKind;

use super::CommandOutput;

/// Store the results of execution of a Command
#[derive(Default)]
pub(crate) struct ReceiptCache {
    // TODO it stores CommandReceipts in two versions, which is not ideal
    receipts_v1: Vec<CommandReceiptV1>,
    receipts_v2: Vec<CommandReceiptV2>
}

impl ReceiptCache {
    pub fn push_command_receipt_v1(&mut self,
        _command_kind: CommandKind,
        exit_code: ExitCodeV1, 
        gas_used: u64,
        command_output: CommandOutput
    ) {
        self.receipts_v1.push(CommandReceiptV1 {
            exit_code,
            gas_used,
            logs: command_output.logs,
            return_values: command_output.return_values
        });
    }

    pub fn push_command_receipt_v2(&mut self, 
        command_kind: CommandKind,
        exit_code: ExitCodeV2,
        gas_used: u64,
        command_output: CommandOutput,
    ) {
        let command_receipt = create_executed_receipt_v2(&command_kind, exit_code, gas_used, command_output);
        self.receipts_v2.push(command_receipt);
    }

    /// Combine the information from next Command Receipt.
    /// Assumption: execution of a deferred command will not spawn non-deferred command.
    pub fn push_deferred_command_receipt_v1(&mut self,     
        _command_kind: CommandKind,
        exit_code: ExitCodeV1, 
        gas_used: u64,
        command_output: CommandOutput
    ) {
        if let Some(last_command_receipt) = self.receipts_v1.last_mut() {
            last_command_receipt.gas_used = last_command_receipt
                .gas_used
                .saturating_add(gas_used);
            last_command_receipt.exit_code = exit_code;
            last_command_receipt.return_values = command_output.return_values;
        }
    }

    /// Combine the information from next Command Receipt.
    /// Assumption: execution of a deferred command will not spawn non-deferred command.
    pub fn push_deferred_command_receipt_v2(
        &mut self,
        command_kind: CommandKind,
        exit_code: ExitCodeV2,
        gas_used: u64,
        command_output: CommandOutput,
    ) {
        if let Some(mut last_command_receipt) = self.receipts_v2.last_mut() {
            let last_command_receipt_gas_used = gas_used_v2(&last_command_receipt);
            set_gas_used_and_exit_code_v2(
                &mut last_command_receipt, 
                // Accumulate Gas Used
                last_command_receipt_gas_used.saturating_add(gas_used),
                // Overide Exit Code
                exit_code
            );
            // Overide return_value
            if let CommandReceiptV2::Call(last_call_receipt) = last_command_receipt {
                match command_kind {
                    CommandKind::Call => {
                        last_call_receipt.return_value = command_output.return_values;
                    }
                    CommandKind::WithdrawDeposit => {
                        last_call_receipt.return_value = command_output.amount_withdrawn.to_le_bytes().to_vec();
                    }
                    CommandKind::StakeDeposit => {
                        last_call_receipt.return_value = command_output.amount_staked.to_le_bytes().to_vec();
                    }
                    CommandKind::UnstakeDeposit => {
                        last_call_receipt.return_value = command_output.amount_unstaked.to_le_bytes().to_vec();
                    }
                    _=> {}
                }
            }
        }
    }

    pub fn into_receipt_v1(self) -> ReceiptV1 {
        self.receipts_v1
    }

    pub fn into_receipt_v2(mut self, gas_used: u64, commands: &[CommandKind]) -> ReceiptV2 {
        // TODO: It is to fill-up the NotExecuted command in the receipt. It can be moved into command execution cycle
        let mut i = self.receipts_v2.len();
        while i < commands.len() {
            self.receipts_v2.push(create_not_executed_receipt_v2(&commands[i]));
            i += 1;
        }
        let exit_code = exit_code_v2(self.receipts_v2.last().unwrap());
        ReceiptV2 {
            gas_used,
            exit_code,
            command_receipts: self.receipts_v2
        }
    }
}

fn create_executed_receipt_v2(
    command: &CommandKind, 
    exit_code: ExitCodeV2, 
    gas_used: u64, 
    command_output: CommandOutput,
) -> CommandReceiptV2 {
    match command {
        CommandKind::Transfer => CommandReceiptV2::Transfer(TransferReceipt { exit_code, gas_used }),
        CommandKind::Deploy => CommandReceiptV2::Deploy(DeployReceipt { exit_code, gas_used }),
        CommandKind::Call => CommandReceiptV2::Call(CallReceipt { exit_code, gas_used, logs: command_output.logs, return_value: command_output.return_values }),
        CommandKind::CreatePool => CommandReceiptV2::CreatePool(CreatePoolReceipt { exit_code, gas_used }),
        CommandKind::SetPoolSettings => CommandReceiptV2::SetPoolSettings(SetPoolSettingsReceipt { exit_code, gas_used }),
        CommandKind::DeletePool => CommandReceiptV2::DeletePool(DeletePoolReceipt { exit_code, gas_used }),
        CommandKind::CreateDeposit => CommandReceiptV2::CreateDeposit(CreateDepositReceipt { exit_code, gas_used }),
        CommandKind::SetDepositSettings => CommandReceiptV2::SetDepositSettings(SetDepositSettingsReceipt { exit_code, gas_used }),
        CommandKind::TopUpDeposit => CommandReceiptV2::TopUpDeposit(TopUpDepositReceipt { exit_code, gas_used }),
        CommandKind::WithdrawDeposit => CommandReceiptV2::WithdrawDeposit(WithdrawDepositReceipt { exit_code, gas_used, amount_withdrawn: command_output.amount_withdrawn }),
        CommandKind::StakeDeposit => CommandReceiptV2::StakeDeposit(StakeDepositReceipt { exit_code, gas_used, amount_staked: command_output.amount_staked }),
        CommandKind::UnstakeDeposit => CommandReceiptV2::UnstakeDeposit(UnstakeDepositReceipt { exit_code, gas_used, amount_unstaked: command_output.amount_unstaked }),
        CommandKind::NextEpoch => CommandReceiptV2::NextEpoch(NextEpochReceipt { exit_code, gas_used }),
    }
}

fn create_not_executed_receipt_v2(command: &CommandKind) -> CommandReceiptV2 {
    match command {
        CommandKind::Transfer => CommandReceiptV2::Transfer(TransferReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::Deploy => CommandReceiptV2::Deploy(DeployReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::Call => CommandReceiptV2::Call(CallReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, logs: Vec::new(), return_value: Vec::new() }),
        CommandKind::CreatePool => CommandReceiptV2::CreatePool(CreatePoolReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::SetPoolSettings => CommandReceiptV2::SetPoolSettings(SetPoolSettingsReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::DeletePool => CommandReceiptV2::DeletePool(DeletePoolReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::CreateDeposit => CommandReceiptV2::CreateDeposit(CreateDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::SetDepositSettings => CommandReceiptV2::SetDepositSettings(SetDepositSettingsReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::TopUpDeposit => CommandReceiptV2::TopUpDeposit(TopUpDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted}),
        CommandKind::WithdrawDeposit => CommandReceiptV2::WithdrawDeposit(WithdrawDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, amount_withdrawn: 0 }),
        CommandKind::StakeDeposit => CommandReceiptV2::StakeDeposit(StakeDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, amount_staked: 0 }),
        CommandKind::UnstakeDeposit => CommandReceiptV2::UnstakeDeposit(UnstakeDepositReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted, amount_unstaked: 0}),
        CommandKind::NextEpoch => CommandReceiptV2::NextEpoch(NextEpochReceipt { gas_used: 0, exit_code: ExitCodeV2::NotExecuted })
    }
}

fn gas_used_v2(command_receipt_v2: &CommandReceiptV2) -> u64 {
    macro_rules! exit_code_v2 {
        ($cmd_recp2:ident, $($var:path,)*) => {
            match $cmd_recp2 {
                $(
                    $var(receipt) => receipt.gas_used,
                )*
            }
        };
    }

    exit_code_v2!(
        command_receipt_v2,
        CommandReceiptV2::Transfer,
        CommandReceiptV2::Call,
        CommandReceiptV2::Deploy,
        CommandReceiptV2::CreatePool,
        CommandReceiptV2::SetPoolSettings,
        CommandReceiptV2::DeletePool,
        CommandReceiptV2::CreateDeposit,
        CommandReceiptV2::SetDepositSettings,
        CommandReceiptV2::TopUpDeposit,
        CommandReceiptV2::WithdrawDeposit,
        CommandReceiptV2::StakeDeposit,
        CommandReceiptV2::UnstakeDeposit,
        CommandReceiptV2::NextEpoch,
    )
}

fn set_gas_used_and_exit_code_v2(command_receipt_v2: &mut CommandReceiptV2, gas_used: u64, exit_code: ExitCodeV2) {
    macro_rules! exit_code_v2 {
        ($cmd_recp2:ident, $($var:path,)*) => {
            match $cmd_recp2 {
                $(
                    $var(receipt) => { receipt.gas_used = gas_used; receipt.exit_code = exit_code; },
                )*
            }
        };
    }

    exit_code_v2!(
        command_receipt_v2,
        CommandReceiptV2::Transfer,
        CommandReceiptV2::Call,
        CommandReceiptV2::Deploy,
        CommandReceiptV2::CreatePool,
        CommandReceiptV2::SetPoolSettings,
        CommandReceiptV2::DeletePool,
        CommandReceiptV2::CreateDeposit,
        CommandReceiptV2::SetDepositSettings,
        CommandReceiptV2::TopUpDeposit,
        CommandReceiptV2::WithdrawDeposit,
        CommandReceiptV2::StakeDeposit,
        CommandReceiptV2::UnstakeDeposit,
        CommandReceiptV2::NextEpoch,
    )
}


fn exit_code_v2(command_receipt_v2: &CommandReceiptV2) -> ExitCodeV2 {
    macro_rules! exit_code_v2 {
        ($cmd_recp2:ident, $($var:path,)*) => {
            match $cmd_recp2 {
                $(
                    $var(receipt) => receipt.exit_code.clone(),
                )*
            }
        };
    }

    exit_code_v2!(
        command_receipt_v2,
        CommandReceiptV2::Transfer,
        CommandReceiptV2::Call,
        CommandReceiptV2::Deploy,
        CommandReceiptV2::CreatePool,
        CommandReceiptV2::SetPoolSettings,
        CommandReceiptV2::DeletePool,
        CommandReceiptV2::CreateDeposit,
        CommandReceiptV2::SetDepositSettings,
        CommandReceiptV2::TopUpDeposit,
        CommandReceiptV2::WithdrawDeposit,
        CommandReceiptV2::StakeDeposit,
        CommandReceiptV2::UnstakeDeposit,
        CommandReceiptV2::NextEpoch,
    )
}