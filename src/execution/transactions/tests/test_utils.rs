use std::collections::HashMap;

use crate::{
    context::TransitionContext,
    execution::{
        execute_next_epoch::{execute_next_epoch_v1, execute_next_epoch_v2},
        state::ExecutionState,
    },
    gas,
    types::{self, BaseTx, TxnVersion},
    BlockProposalStats, BlockchainParams, TransitionV1Result, ValidatorPerformance,
};
use pchain_types::{
    blockchain::{
        Command, CommandReceiptV1, CommandReceiptV2, ExitCodeV1, ExitCodeV2, ReceiptV2,
        TransactionV1, TransactionV2,
    },
    cryptography::PublicAddress,
};
use pchain_world_state::{
    constants, NetworkAccountSized, PoolKey, Stake, StakeValue, VersionProvider, WorldState, DB,
    V1, V2,
};

pub(crate) const TEST_MAX_VALIDATOR_SET_SIZE: u16 = constants::MAX_VALIDATOR_SET_SIZE;
pub(crate) const TEST_MAX_STAKES_PER_POOL: u16 = constants::MAX_STAKES_PER_POOL;
pub(crate) const MIN_BASE_FEE: u64 = 8;
type NetworkAccount<'a, S> =
    NetworkAccountSized<'a, S, { TEST_MAX_VALIDATOR_SET_SIZE }, { TEST_MAX_STAKES_PER_POOL }>;

type ExecutionStateV1<'a, S> = ExecutionState<'a, S, CommandReceiptV1, V1>;
type ExecutionStateV2<'a, S> = ExecutionState<'a, S, CommandReceiptV2, V2>;

type Key = Vec<u8>;
type Value = Vec<u8>;

#[derive(Clone, Default)]
pub(crate) struct SimpleStore {
    inner: HashMap<Key, Value>,
}
impl DB for SimpleStore {
    fn get(&self, key: &[u8]) -> Option<Value> {
        match self.inner.get(key) {
            Some(v) => Some(v.clone()),
            None => None,
        }
    }
}

pub(crate) const ACCOUNT_A: [u8; 32] = [1u8; 32];
pub(crate) const ACCOUNT_B: [u8; 32] = [2u8; 32];
pub(crate) const ACCOUNT_C: [u8; 32] = [3u8; 32];
pub(crate) const ACCOUNT_D: [u8; 32] = [4u8; 32];
pub(crate) const DEFAULT_AMOUNT: u64 = 500_000_000;

#[derive(Default)]
pub(crate) struct TestFixture {
    store: SimpleStore,
}

impl TestFixture {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn ws<V>(&self) -> WorldState<'_, SimpleStore, V>
    where
        V: VersionProvider + Send + Sync + Clone + 'static,
    {
        let mut ws = WorldState::<SimpleStore, V>::new(&self.store);
        for account in [ACCOUNT_A, ACCOUNT_B, ACCOUNT_C, ACCOUNT_D].iter() {
            ws.account_trie_mut()
                .set_balance(account, DEFAULT_AMOUNT)
                .unwrap();
        }
        ws
    }
}

pub(crate) fn create_state_v1(
    init_ws: Option<WorldState<SimpleStore, V1>>,
) -> ExecutionStateV1<SimpleStore> {
    let tx = create_tx(ACCOUNT_A);
    let ctx = TransitionContext::new(TxnVersion::V1, init_ws.unwrap(), tx.gas_limit);
    let base_tx = BaseTx::from(&tx);

    ExecutionState::new(base_tx, create_bd(), ctx)
}

pub(crate) fn create_state_v2(
    init_ws: Option<WorldState<SimpleStore, V2>>,
) -> ExecutionStateV2<SimpleStore> {
    let tx = create_tx_v2(ACCOUNT_A);
    let ctx = TransitionContext::new(TxnVersion::V2, init_ws.unwrap(), tx.gas_limit);
    let base_tx = BaseTx::from(&tx);
    ExecutionState::new(base_tx, create_bd(), ctx)
}

pub(crate) fn set_tx(
    state: &mut ExecutionStateV1<SimpleStore>,
    signer: PublicAddress,
    nonce: u64,
    commands: &Vec<Command>,
) -> u64 {
    let mut tx = create_tx(signer);
    tx.nonce = nonce;
    tx.commands = commands.clone();
    state.tx = BaseTx::from(&tx);
    gas::tx_inclusion_cost_v1(state.tx.size, &state.tx.command_kinds)
}

pub(crate) fn create_tx(signer: PublicAddress) -> TransactionV1 {
    TransactionV1 {
        signer,
        gas_limit: 10_000_000,
        priority_fee_per_gas: 0,
        max_base_fee_per_gas: MIN_BASE_FEE,
        nonce: 0,
        hash: [0u8; 32],
        signature: [0u8; 64],
        commands: Vec::new(),
    }
}

pub(crate) fn set_tx_v2(
    state: &mut ExecutionStateV2<SimpleStore>,
    signer: PublicAddress,
    nonce: u64,
    commands: &Vec<Command>,
) -> u64 {
    let mut tx = create_tx_v2(signer);
    tx.nonce = nonce;
    tx.commands = commands.clone();
    state.tx = BaseTx::from(&tx);
    gas::tx_inclusion_cost_v2(state.tx.size, &state.tx.command_kinds)
}

pub(crate) fn create_tx_v2(signer: PublicAddress) -> TransactionV2 {
    TransactionV2 {
        signer,
        gas_limit: 10_000_000,
        priority_fee_per_gas: 0,
        max_base_fee_per_gas: MIN_BASE_FEE,
        nonce: 0,
        hash: [0u8; 32],
        signature: [0u8; 64],
        commands: Vec::new(),
    }
}

pub(crate) fn create_bd() -> BlockchainParams {
    let mut validator_performance = ValidatorPerformance::default();
    validator_performance.blocks_per_epoch = TEST_MAX_VALIDATOR_SET_SIZE as u32;
    for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
        let mut address = [1u8; 32];
        address[0] = i as u8;
        validator_performance
            .stats
            .insert(address, BlockProposalStats::new(1));
    }
    BlockchainParams {
        this_block_number: 1,
        prev_block_hash: [3u8; 32],
        this_base_fee: 1,
        timestamp: 1665370157,
        random_bytes: [255u8; 32],
        proposer_address: [99u8; 32],
        treasury_address: [100u8; 32],
        cur_view: 1234,
        validator_performance: Some(validator_performance),
    }
}

pub(crate) fn single_node_performance(
    address: PublicAddress,
    num_of_blocks: u32,
) -> ValidatorPerformance {
    let mut validator_performance = ValidatorPerformance::default();
    validator_performance.blocks_per_epoch = num_of_blocks;
    validator_performance
        .stats
        .insert(address, BlockProposalStats::new(num_of_blocks));
    validator_performance
}

pub(crate) fn all_nodes_performance() -> ValidatorPerformance {
    let mut validator_performance = ValidatorPerformance::default();
    validator_performance.blocks_per_epoch = TEST_MAX_STAKES_PER_POOL as u32;

    for i in 0..TEST_MAX_STAKES_PER_POOL {
        let mut address = [1u8; 32];
        address[0] = i as u8;
        validator_performance
            .stats
            .insert(address, BlockProposalStats::new(1));
    }
    validator_performance
}

/// Account address range from \[X, X, X, X, ... , 2\] where X starts with u32(\[2,2,2,2\]). Number of Accounts = MAX_STAKES_PER_POOL
pub(crate) fn prepare_accounts_balance<V>(ws: &mut WorldState<SimpleStore, V>)
where
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    let start = u32::from_le_bytes([2u8, 2, 2, 2]);
    for i in 0..TEST_MAX_STAKES_PER_POOL {
        let mut address = [2u8; 32];
        address[0..4].copy_from_slice(&(start + i as u32).to_le_bytes().to_vec());
        ws.account_trie_mut()
            .set_balance(&address, DEFAULT_AMOUNT)
            .unwrap();
    }
}

/// Pools address range from \[X, 1, 1, 1, ... , 1\] where X \in \[1, TEST_MAX_VALIDATOR_SET_SIZE\]
/// Pool powers = 100_000, 200_000, ... , 6_400_000
pub(crate) fn create_full_pools_in_nvp<E, V>(
    ws: &mut ExecutionState<'_, SimpleStore, E, V>,
    add_operators_deposit: bool,
    operators_auto_stake_rewards: bool,
) where
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    NetworkAccount::nvp(&mut ws.ctx.gas_meter).clear();
    for i in 1..TEST_MAX_VALIDATOR_SET_SIZE + 1 {
        let (address, power, rate) = init_setup_pool_power(i);
        let mut pool = NetworkAccount::pools(&mut ws.ctx.gas_meter, address);
        pool.set_operator(address);
        pool.set_power(power);
        pool.set_commission_rate(rate);
        pool.set_operator_stake(Some(Stake {
            owner: address,
            power,
        }));
        NetworkAccount::nvp(&mut ws.ctx.gas_meter)
            .insert(PoolKey {
                operator: address,
                power,
            })
            .unwrap();
        if add_operators_deposit {
            NetworkAccount::deposits(&mut ws.ctx.gas_meter, address, address).set_balance(power);
            NetworkAccount::deposits(&mut ws.ctx.gas_meter, address, address)
                .set_auto_stake_rewards(operators_auto_stake_rewards);
        }
    }
    assert_eq!(
        NetworkAccount::nvp(&mut ws.ctx.gas_meter).length(),
        TEST_MAX_VALIDATOR_SET_SIZE as u32
    );
}

/// Stake address = [i, 1, 1, 1, 1, 1, 1, 1, ...]
/// Pool powers = 100_000 * (i)
/// Commission_rate = i % 100
pub(crate) fn init_setup_pool_power(i: u16) -> (PublicAddress, u64, u8) {
    let mut address = [1u8; 32];
    address[0] = i as u8;
    let power = 100_000 * i as u64;
    (address, power, i as u8 % 100)
}

/// Staker address range from \[X, X, X, X, ... , 2\] where X starts with u32(\[2,2,2,2\]). Number of stakers = TEST_MAX_STAKES_PER_POOL
/// Stake powers = 200_000, 300_000, ...
pub(crate) fn create_full_stakes_in_pool<E, V>(
    ws: &mut ExecutionState<'_, SimpleStore, E, V>,
    operator: PublicAddress,
) where
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
        .delegated_stakes()
        .clear();
    let mut sum = 0;
    let mut vs = vec![];
    for i in 0..TEST_MAX_STAKES_PER_POOL {
        let (address, power) = init_setup_stake_of_owner(i);
        sum += power;
        let stake = StakeValue::new(Stake {
            owner: address,
            power,
        });
        vs.push(stake);
    }
    NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
        .delegated_stakes()
        .reset(vs)
        .unwrap();
    let operator_stake = NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
        .operator_stake()
        .map_or(0, |p| p.map_or(0, |v| v.power));
    NetworkAccount::pools(&mut ws.ctx.gas_meter, operator).set_operator(operator);
    NetworkAccount::pools(&mut ws.ctx.gas_meter, operator).set_power(sum + operator_stake);
    NetworkAccount::nvp(&mut ws.ctx.gas_meter).change_key(PoolKey {
        operator,
        power: sum + operator_stake,
    });
    assert_eq!(
        NetworkAccount::pools(&mut ws.ctx.gas_meter, operator)
            .delegated_stakes()
            .length(),
        TEST_MAX_STAKES_PER_POOL as u32
    );
}

/// Stake address = [X, X, X, X, 2, 2, 2, 2, ...] where X,X,X,X is i as LE bytes
/// Stake powers = 100_000 * (i+2)
pub(crate) fn init_setup_stake_of_owner(i: u16) -> (PublicAddress, u64) {
    let start = u32::from_le_bytes([2u8, 2, 2, 2]);
    let mut address = [2u8; 32];
    address[0..4].copy_from_slice(&(start + i as u32).to_le_bytes().to_vec());
    (address, 100_000 * (i + 2) as u64)
}

/// Staker address range from \[X, X, X, X, ... , 2\] where X starts with u32(\[2,2,2,2\]). Number of stakers = TEST_MAX_STAKES_PER_POOL
/// Deposits = 200_000, 300_000, ...
pub(crate) fn create_full_deposits_in_pool<E, V>(
    ws: &mut ExecutionState<'_, SimpleStore, E, V>,
    operator: PublicAddress,
    auto_stake_rewards: bool,
) where
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    for i in 0..TEST_MAX_STAKES_PER_POOL {
        let (address, balance) = init_setup_stake_of_owner(i);
        NetworkAccount::deposits(&mut ws.ctx.gas_meter, operator, address).set_balance(balance);
        NetworkAccount::deposits(&mut ws.ctx.gas_meter, operator, address)
            .set_auto_stake_rewards(auto_stake_rewards);
    }
}
pub(crate) fn create_full_nvp_pool_stakes_deposits<E, V>(
    ws: &mut ExecutionState<'_, SimpleStore, E, V>,
    auto_stake_rewards: bool,
    add_operators_deposit: bool,
    operators_auto_stake_rewards: bool,
) where
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    create_full_pools_in_nvp(ws, add_operators_deposit, operators_auto_stake_rewards);
    let mut nvps = vec![];
    for i in 0..TEST_MAX_VALIDATOR_SET_SIZE {
        let p = NetworkAccount::nvp(&mut ws.ctx.gas_meter)
            .get(i as u32)
            .unwrap();
        nvps.push(p);
    }
    for p in nvps {
        create_full_stakes_in_pool(ws, p.operator);
        create_full_deposits_in_pool(ws, p.operator, auto_stake_rewards);
    }
}

// pool (account a) in world state, included in nvp.
//      with delegated stakes of account b, auto_stake_reward = false
//      with non-zero value of Operator Stake, auto_stake_reward = false
// pool[A].power = 100_000
// pool[A].operator_stake = 10_000
// pool[A].delegated_stakes[B] = 90_000
// deposits[A, A] = 10_000
// deposits[A, B] = 90_000

// pool[A].power = 100_000
// pool[A].operator_stake = 10_000
// pool[A].delegated_stakes[B] = 90_000
// deposits[A, A] = 10_000
// deposits[A, B] = 90_000
pub(crate) fn setup_pool<E, V>(
    state: &mut ExecutionState<'_, SimpleStore, E, V>,
    operator: PublicAddress,
    operator_power: u64,
    owner: PublicAddress,
    owner_power: u64,
    auto_stake_rewards_a: bool,
    auto_stake_rewards_b: bool,
) where
    V: VersionProvider + Send + Sync + Clone + 'static,
{
    let mut pool = NetworkAccount::pools(&mut state.ctx.gas_meter, operator);
    pool.set_operator(operator);
    pool.set_power(operator_power + owner_power);
    pool.set_commission_rate(1);
    pool.set_operator_stake(Some(Stake {
        owner: operator,
        power: operator_power,
    }));
    NetworkAccount::pools(&mut state.ctx.gas_meter, operator)
        .delegated_stakes()
        .insert(StakeValue::new(Stake {
            owner: owner,
            power: owner_power,
        }))
        .unwrap();
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, operator);
    deposit.set_balance(operator_power);
    deposit.set_auto_stake_rewards(auto_stake_rewards_a);
    let mut deposit = NetworkAccount::deposits(&mut state.ctx.gas_meter, operator, owner);
    deposit.set_balance(owner_power);
    deposit.set_auto_stake_rewards(auto_stake_rewards_b);
    NetworkAccount::nvp(&mut state.ctx.gas_meter)
        .insert(PoolKey {
            operator,
            power: operator_power + owner_power,
        })
        .unwrap();
}

pub(crate) fn execute_next_epoch_test_v1(
    state: ExecutionStateV1<SimpleStore>,
) -> ExecutionStateV1<SimpleStore> {
    let ret = execute_next_epoch_v1(state, vec![Command::NextEpoch]);

    let receipt = ret.receipt.as_ref().expect("Receipt expected");
    assert_eq!(ret.error.as_ref(), None);
    if let Some(c) = receipt.last() {
        assert_eq!(c.exit_code, ExitCodeV1::Success);
    } else {
        panic!("Expected command receipt");
    }

    assert_eq!(extract_gas_used(&ret), 0);
    println!(
        "new validators {}",
        ret.validator_changes
            .as_ref()
            .unwrap()
            .new_validator_set
            .len()
    );
    println!(
        "remove validators {}",
        ret.validator_changes
            .as_ref()
            .unwrap()
            .remove_validator_set
            .len()
    );
    create_state_v1(Some(ret.new_state))
}

pub(crate) fn execute_next_epoch_test_v2(
    state: ExecutionStateV2<SimpleStore>,
) -> ExecutionStateV2<SimpleStore> {
    let ret = execute_next_epoch_v2(state, vec![Command::NextEpoch]);

    assert!(verify_receipt_content_v2(
        ret.receipt.as_ref().expect("Receipt expected"),
        0,
        0,
        ExitCodeV2::Ok,
        0
    ));

    println!(
        "new validators {}",
        ret.validator_changes
            .as_ref()
            .unwrap()
            .new_validator_set
            .len()
    );
    println!(
        "remove validators {}",
        ret.validator_changes
            .as_ref()
            .unwrap()
            .remove_validator_set
            .len()
    );
    create_state_v2(Some(ret.new_state))
}

pub(crate) fn extract_gas_used(ret: &TransitionV1Result<SimpleStore, V1>) -> u64 {
    ret.receipt
        .as_ref()
        .unwrap()
        .iter()
        .map(|g| g.gas_used)
        .sum::<u64>()
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
        .map(|g| types::gas_used_and_exit_code_v2(g).0)
        .sum::<u64>();

    let count = receipt
        .command_receipts
        .iter()
        .rev()
        .map(types::gas_used_and_exit_code_v2)
        .take_while(|(_, e)| e == &ExitCodeV2::NotExecuted)
        .count();

    gas_used_in_header == total_gas_used
        && gas_used_in_commands == commands_gas_used
        && receipt.exit_code == receipt_exit_code
        && count == non_executed_count
}
