use super::SimulateWorldStateStorage;
use pchain_runtime::TransitionV1Result;
use pchain_types::blockchain::{CommandReceiptV2, ExitCodeV2, ReceiptV2};
use pchain_world_state::V1;

pub(crate) fn extract_gas_used(ret: &TransitionV1Result<SimulateWorldStateStorage, V1>) -> u64 {
    ret.receipt
        .as_ref()
        .unwrap()
        .iter()
        .map(|g| g.gas_used)
        .sum::<u64>()
}

pub(crate) fn gas_used_and_exit_code_v2(
    command_receipt_v2: &CommandReceiptV2,
) -> (u64, ExitCodeV2) {
    macro_rules! exit_code_v2 {
        ($cmd_recp2:ident, $($var:path,)*) => {
            match $cmd_recp2 {
                $(
                    $var(receipt) => (receipt.gas_used, receipt.exit_code.clone()),
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

pub(crate) fn verify_receipt_content_v2(
    receipt: &ReceiptV2,
    total_gas_used: u64,
    commands_gas_used: u64,
    receipt_exit_code: ExitCodeV2,
    non_executed_count: usize,
) -> bool {
    let gas_used_in_header = receipt.gas_used;

    let gas_used_in_commands = receipt
        .command_receipts
        .iter()
        .map(|g| gas_used_and_exit_code_v2(g).0)
        .sum::<u64>();

    let count = receipt
        .command_receipts
        .iter()
        .rev()
        .map(gas_used_and_exit_code_v2)
        .take_while(|(_, e)| e == &ExitCodeV2::NotExecuted)
        .count();

    gas_used_in_header == total_gas_used
        && gas_used_in_commands == commands_gas_used
        && receipt.exit_code == receipt_exit_code
        && count == non_executed_count
}
