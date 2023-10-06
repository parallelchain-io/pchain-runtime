use super::SimulateWorldStateStorage;
use pchain_runtime::TransitionResultV1;

pub(crate) fn extract_gas_used(ret: &TransitionResultV1<SimulateWorldStateStorage>) -> u64 {
    ret.receipt
        .as_ref()
        .unwrap()
        .iter()
        .map(|g| g.gas_used)
        .sum::<u64>()
}
