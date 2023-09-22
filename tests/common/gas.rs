use super::SimulateWorldStateStorage;
use pchain_runtime::TransitionResult;

pub(crate) fn extract_gas_used(ret: &TransitionResult<SimulateWorldStateStorage>) -> u64 {
    ret.receipt
        .as_ref()
        .unwrap()
        .iter()
        .map(|g| g.gas_used)
        .sum::<u64>()
}
