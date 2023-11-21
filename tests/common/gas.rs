use super::SimulateWorldStateStorage;
use pchain_runtime::TransitionResultV1;
use pchain_world_state::V1;

// TODO 90, shouldnt this be tied to V1?
pub(crate) fn extract_gas_used(ret: &TransitionResultV1<SimulateWorldStateStorage, V1>) -> u64 {
    ret.receipt
        .as_ref()
        .unwrap()
        .iter()
        .map(|g| g.gas_used)
        .sum::<u64>()
}
