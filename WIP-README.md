TODO delete this file after refactoring
Note. the numbered TODOs are not be in sequence as some have been merged or closed

TODO v0.5 changes

### GAS METER REFACTORING (DONE except TODOS)

RuntimeGasMeter is a Singleton struct on TransitionContext

- Facade for all non-Wasmer operations which are chargeable (see Wasmer Gas Accounting for Wasmer)

#### General Features

- Exposed methods deduct gas, encapsulate business logic, and delegate (to RWSet, if needed)

- Lives and holds Gas state for the entire txn

  - Saves `txn_inclusion_gas`
  - Intermediate per command gas stored in `command_gas_used`
  - After each command, lifecycle has to call `finalize_command_gas()`
    - Calls `get_gas_used_for_current_command()`, capped to gas limit, receipt stores this
    - Saves to `total_command_gas_used`

- Preserves old behaviour of (generally) charging before performing the operation

#### Categories of methods

A. World state read-write costs (DONE)

- Facade for WorldState (rw*set), prefixed by `ws*`, e.g. `ws_get_app_data(address: PublicAddress, app_key: AppKey)`
- Facade for NetworkAccountStorage( through trait implementation)

B. Transaction Storage Costs (DONE)

- Prefixed by `charge_txn*`
- Pre-execution portion (i.e. TransactionInclusionCost)

  - `charge_txn_pre_exec_inclusion(txsize: usize, commands_len: usize)`

- Post-execution portion (i.e. portion known after execution)

  - `charge_txn_post_exec_return_value(ret_val: Vec<u8>)` - see TODO 6

  - `charge_txn_post_exec_log(log: Log)` - see TODO 6

C. Crytographic operations on host machine (DONE)

- Facade methods for cryptographic operations on host machine callable by contracts
- Prefixed by `host_`
- E.g. `host_sha256(input: Vec<u8>)`

#### Wasmer Gas Accounting

- Env (i.e. WasmerEnv) now only tracks gas from two sources

  D. read/write by host to Wasm guest linear memory by calling Env::write_bytes() and Env::read_bytes()
  E. Wasm compute cost using Function Middleware by automatic exeuction of smart contract code

  - Decision to keep this way so that RuntimeGasMeter won't have to wrap and interact with the lifecycle of Wasmer instances during the transaction

- Lifecycle

  - No change to initialization and lifecycle
  - Gas from the 2 sources will mutate `wasmer_metering_remaining_points`
  - Running total is brought back to RuntimeGasMeter eventually by calling `charge_wasmer_gas(gas: s)`

- TODO 7 - `non_wasmer_gas_amount` is no longer needed, can remove every where

  - Potentially means can simply the `GasMeter` struct which holds this field (found in wasmer_env.rs)

- Other GasMeter TODOs

  - TODO 4 - temp keeping the total_gas_used_clamped field, but should remove if no use

  - TODO 6 - should check using RuntimeGasMeter: gas_meter.gas_limit > (gas_meter.get_gas_to_be_used_in_theory() + wasm gas used)
    - To preserve previous behaviour to halt further execution at the same point as before
    - Explaination: Because `wasmer_metering_remaining_points` previously would be deducted against all (A, B, C, D, E), now only (D and E)
    - So now, `wasmer_metering_remaining_points` could be > 0 but actually gas is fully used, due to category A, B, C usage during contract execution

#### REFERENCE ONLY : Existing Wasmer initialization and gas meter interactions (UNCHANGED)

```

Account.rs/call() {
  calls CallInstance.instantiate()
}

-> CallInstance.instantiate() {
  loads and checks CBI, storage
  then fetches grand_total (= gas_used + write_gas + read_gas + receipt_write_gas)
  then if grand_total > limit (due to init phase), return Error, else
  then calculates TX.limit - grand_total as RemainingCallGas
  then module.instantiate()
}

-> ContractModule.instantiate() {
  creates wasmer_env::Env::new()
  creates importable from wasmer_env::Env and store
  then module.instantiate()
}

-> Module.instantiate() {
   IMPORTANT!
   creates wasmer_instance = wasmer::Instance::new(), passing in Env
   calls wasmer_middlewares::metering::set_remaining_points, passing in &wasmer_instance, gas_limit = RemainingCallGas
   // sets remaining on wasmer instance global var "wasmer_metering_remaining_points"
  return wasmer_instance (wrapped in a struct)
}

-> ContractModule.instantiate() {
  returns ContractInstance (wrapping struct wrapping wasmer_instance)
}

-> CallInstance.instantiate() {
  returns CallInstance (wrapping ContractInstance wrapping struct wrapping wasmer_instance)
}

-> Account.rs/call() {
  calls instance.call()
}

-> CallInstance.call() {
  self.instance.call() (i.e. inner ContractInstance.call() )
}

-> ContractInstance.call() {
  calls "init_wasmer_remaining_points", points wasmer_gas to the Wasmer global "wasmer_metering_remaining_points"
  calls wasmer_instance.call_method() which returns call_result containing
  calculates total_gas = gas_limit (RemainingCallGas) - remaining_gas ("wasmer_metering_remaining_points") + non_wasmer_gas
  returns total_gas (i.e. WasmerExecutionGas)
}

-> CallInstance.call() {
  updates TransitionContext.gas_used using TransitionContext.set_gas_consumed by adding WasmerExecutionGas to the total
  then fetches grand_total (= gas_used + write_gas + read_gas + receipt_write_gas)

  // TODO facade receipt_gas_gas
  then if grand_total > limit, return Error
}

-> Execution.rs/call()
```

### LIFECYCLE AND FILE ORGANISATION (TODO)

- Potential areas for the lifecycle refactor marked with TODO 8 as follows:

  - "TODO 8 - Potentially part of command lifecycle refactor"

- General Ideas

  - Consistent interface for triggering commands (e.g. All Commands impl Executable interface)
    - Enables pre- and post- execution life cycle
  - Better sort Actions and Phases

- Specific places:

  - `phase::finalize_gas_consumption()`

    - Does every single command method need to individually call this?
    - It basically just checks for GasExhaustion and decides whether to abort
    - Should be part of a fixed step after successful Command exeuction

  - `phase::abort()`
    - This is technically not a phase, it's an Event that transits the phase
    - Can be re-organised

#### REREFENCE - SAMPLE FILE STRUCTURE

```
// Example
|-- transition.rs (entrypoint)
|-- commands/
|   |-- account/ // either single file or many separate files for each command
|   |-- staking/ // either single file or many separate files for each command
|   |-- protocol/ // either single file or many separate files for each command
|   |-- command_executable.rs // defines an executable trait
|-- execution/ (previously execution.rs)
|   |-- transactions/
|   |   |-- execute_commands.rs // CHARGING is involved - traces the life cycle from pre-charge to charge
|   |   |-- phases/ // either single file or many separate files for each phase - Charge/ Precharge
|   |   |    |-- charge.rs
|   |   |    |-- pre_charge.rs
|   |   |-- abort.rs // describes abort action
|   |   |-- finalize_gas_consumption.rs // helper to finalise gas consumption
|   |-- view/
|   |   |-- execute_view.rs // no charging involved
|   |-- next_epoch/
|   |-- |-- execute_next_epoch.rs // purely for next epoch
|-- contract/
|   |-- cbi.rs
|   |-- cbi_version.rs (previously contract/version.rs)
|   |-- contract.rs (previously contract/context.rs) // base definitions and contract
|   |-- host_functions // all host function and helper logic here
|   |   |-- functions.rs
|   |-- wasm/
|       |-- module.rs // represents a wasm module
|       |-- instance.rs // represents a wasm instance
|-- calculation/
|   |-- gas_cost.rs
|   |-- reward_formulas.rs
```

### V5 Changes (TODO)

#### CHANGES

All these methods, or their refactored versions, will have to take in Strategy (or its parent TransitionContext as a parameter)

Refer to v0.5 CHANGELOG: https://hackmd.io/@V8G7dGj7QPG84VX_SNL6Wg/HkL1cff0n

1. At the end of each Command's lifecyle:
   Create a ReceiptV2 with extra fields other than Vec<CommandReceipt>
   E.g. will need to fetch `txn_inclusion_gas` now available on RuntimeGasMeter instance
   Now called in `StateChangesResult::finalize()`

1. At the end of each Command's lifecylce:
   Create specific variants of CommandReceiptsV2 based on Command variant, instead of a single CommandReceipt type
   Now called in `TransitionContext::extract()`

1. At the pre-charge phase:
   Update the formula for the Transaction Inclusion Cost (i.e. pre-determined portion of Command Receipt inclusion)
   Now called in tx_inclusion_cost()
   PENDING updated constant

1. Variable portion of TransactionInclusionCost
   Individual commands will now write to individual fields, e.g. `AmountWithdrawn` for `WithdrawDeposit`
   This will need methods to be replaced instead of generically calling `charge_txn_post_exec_return_value()`

1. New gas formulas for MPT operations that do not “double charge” 32 bytes in the key length.

   - Remove the extra address.len() in CacheKey.len() method, matching the CacheKey::App variant
   - https://github.com/parallelchain-io/parallelchain-protocol/issues/3

1. New function for contract address which has command index as a parameter.
   Double check where this should be

#### IMPLEMENTATION

- Dependency: Import both pchain-types v0.4 and v0.5 and disambuguate

- For each change, if using strategy

  - Implement specific methods on Strategy variant
  - Callers (or refactored callers) need to take in Strategy (or its parent TransitionContext) as a param

- Otherwise fork using if-else

#### FOR REF - PROPOSED STRATEGY PATTERN

Proposed implementation: Strategy Pattern

- Two separate entry points - transitionV4() and transitionV5() with slightly different input signatures

- Each entry point function selects either Strategy variant(V4Strategy or V5Strategy) and attaches it to TransitionContext

- At relevant points in the code, call strategy-dependent methods:

  ```rs
  let contract_address = strategy.calculate_contract_address(state, command_idx);
  ```

##### SAMPLE STRATEGY DEFINITION FILE

```rs
use pchain_types::cryptography::PublicAddress;
use pchain_world_state::storage::WorldStateStorage;

use super::state::ExecutionState;

pub(crate) struct V5Strategy;
pub(crate) struct V4Strategy;

//
// Umbrella strategy enum
//
// let contract_address = strategy.calculate_contract_address(state, command_idx);
//
pub(crate) enum Strategy {
    V4(V4Strategy),
    V5(V5Strategy),
}

trait StrategyTrait {
    fn calculate_contract_address<S>(
        &self,
        state: &ExecutionState<S>,
        command_idx: usize,
    ) -> [u8; 32]
    where
        S: WorldStateStorage + Send + Sync + Clone;
}

impl StrategyTrait for Strategy {
    fn calculate_contract_address<S>(
        &self,
        state: &ExecutionState<S>,
        command_idx: usize,
    ) -> [u8; 32]
    where
        S: WorldStateStorage + Send + Sync + Clone,
    {
        match self {
            Strategy::V4(strategy) => strategy.calculate_contract_address(
                V4Strategy::build_calculation_input(state, command_idx),
            ),
            Strategy::V5(strategy) => strategy.calculate_contract_address(
                V5Strategy::build_calculation_input(state, command_idx),
            ),
        }
    }
}

//
// Per-method implementations
//
pub(crate) enum ContractAddressCalculationInput {
    V4 {
        signer: PublicAddress,
        nonce: u64,
    },
    V5 {
        signer: PublicAddress,
        nonce: u64,
        deploy_command_idx: usize,
    },
}

pub(crate) trait InputBuilder<S>
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn build_calculation_input(
        state: &ExecutionState<S>,
        command_idx: usize,
    ) -> ContractAddressCalculationInput;
}

impl<S> InputBuilder<S> for V4Strategy
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn build_calculation_input(
        state: &ExecutionState<S>,
        command_idx: usize,
    ) -> ContractAddressCalculationInput {
        ContractAddressCalculationInput::V4 {
            signer: state.tx.signer,
            nonce: state.tx.nonce,
        }
    }
}

impl<S> InputBuilder<S> for V5Strategy
where
    S: WorldStateStorage + Send + Sync + Clone,
{
    fn build_calculation_input(
        state: &ExecutionState<S>,
        command_idx: usize,
    ) -> ContractAddressCalculationInput {
        ContractAddressCalculationInput::V5 {
            signer: state.tx.signer,
            nonce: state.tx.nonce,
            deploy_command_idx: command_idx,
        }
    }
}

pub(crate) trait ContractAddressCalculationStrategy {
    fn calculate_contract_address(&self, input: ContractAddressCalculationInput) -> [u8; 32];
}

impl ContractAddressCalculationStrategy for V4Strategy {
    fn calculate_contract_address(&self, input: ContractAddressCalculationInput) -> [u8; 32] {
        match input {
            ContractAddressCalculationInput::V4 { signer, nonce } => {
                pchain_types::cryptography::sha256(
                    [signer.to_vec(), nonce.to_le_bytes().to_vec()].concat(),
                )
            }
            // TODO
            _ => panic!("Invalid input for V4Strategy"),
        }
    }
}

impl ContractAddressCalculationStrategy for V5Strategy {
    fn calculate_contract_address(&self, input: ContractAddressCalculationInput) -> [u8; 32] {
        match input {
            ContractAddressCalculationInput::V5 {
                signer,
                nonce,
                deploy_command_idx,
            } => pchain_types::cryptography::sha256(
                [
                    signer.to_vec(),
                    nonce.to_le_bytes().to_vec(),
                    deploy_command_idx.to_le_bytes().to_vec(),
                ]
                .concat(),
            ),
            // TODO
            _ => panic!("Invalid input for V5Strategy"),
        }
    }
}
```
