# ParallelChain F Runtime

ParallelChain F Runtime is a **State Transition Function** to transit from an input state of the blockchain to next state. It is also the sole system component to handle Smart Contract that is primarily built from Rust code by using ParallelChain F Smart Contract Development Kit (SDK).

```
f(WS, BD, TX) -> (WS', R)

WS = World state represented by set of key-value pairs
BD = Blockchain Data
TX = Transaction, which is essentially a sequence of Commands
R = Receipt, which is a sequence of Command Receipts correspondingly.
```

## Example usage 

Runtime transits states by minimal number of high-level API method.

```rust
// Step 1. prepare world state (ws), transaction (tx), and blockchain data (bd).
// ...

// Step 2. call transition.
let result = pchain_runtime::new().transition(ws, tx, bd);
```

The result of state transition contains 
- (WS') new key-value pairs which are the changes to world state
- (R) transaction receipt
- transition error

Transaction Receipt could be `None` if the transition exits without `ExitStatus`. Transition error is not included in a block, but it is useful for debugging purposes in diagnosis.

## Commands

In general, Runtime takes blockchain (BD) and transaction (TX) as inputs and compute the changes to data in world state (WS). The computation involves Transaction Commands:

Account Commands
- [Transfer Token](#transfer)
- [Deploy Contract](#deploy)
- [Call Contract](#call)

Network Commands
- [Create Pool](#create-pool)
- [Set Pool Settings](#set-pool-settings)
- [Delete Pool](#delete-pool)
- [Create Deposit](#create-deposit)
- [Set Deposit Settings](#set-deposit-settings)
- [Top Up Deposit](#top-up-deposit)
- [Withdraw Deposit](#withdraw-deposit)
- [Stake Deposit](#stake-deposit)
- [Unstake Deposit](#unstake-deposit)

Administration Commands
- [Next Epoch](#next-epoch)

Runtime restricts a Transaction to include either mixture of Account Commands and Network Commands, or a single Next Epoch Command. Runtime rejects multiple Next Epoch Commands in a transaction.

## Receipts

Transaction execution is deterministic, but because users generally cannot control when and where their transaction is included in a block, the results of a transaction generally cannot be predicted 100% accurately before it becomes part of a block. To inform users about the result of a transaction, every transaction is included in a block along with a “Receipt”.

A receipt describes what happened during the execution of a transaction at a high level of detail. It is a structure of a sequence of Command Receipt which consists of four fields:

- Exit Statuses: it tells whether the corresponding command in the sequence succeeded in doing its operation, and, if it failed, whether the failure is because of gas exhaustion or some other reason.
- Gas Used: how much gas was used in the execution of the transaction. This will at most be the transaction’s gas limit.
- Return Values: the return value of the corresponding command.
- Logs: the logs emitted during the corresponding call command.

Command Receipts are included in a receipt in the order their command is included in the transaction. All commands exit with an exit status, but only call commands can create logs, so transactions that do not have a call command will have empty logs. When a command fails, following commands do not get executed, so a receipt can have less exit statuses than the transaction has commands.

## Execution

The execution of a Transaction proceeds through a fixed sequence of steps, or ’Phases’: 

```
Tentative Charge → Work → Charge
```

Tentative Charge is a common phase for almost all transaction commands. It basically verifies the validity of a transaction before executing it.
- If it succeeds, it must incur state changes with gas consumption, and proceed to Work step.
- If it fails, the state remains unchanged and without gas consumption. In this case, there is no transaction receipt.

Work is execution of a sequence of Transaction Commands. 
- If it fails, the state could be changed, and further gas consumption in Work is possible. In this case, there is a transaction receipt.
- The gas amount created in Tentative Charge step is counted as part of gas consumption to the first Command.

Charge is the final step of state transition. Change of balances of accounts happens in Charge step, and gas consumption is finalized to generate Receipt and the Transition Result.

The above phases do not apply to Next Epoch Command. It does not charge for gas consumption, hence results in zero gas being used in the receipts.

The following content describes the process inside each phase in an abstract way.

### Tentative Charge

Charge signer balance.

```text
1. ws[signer].balance -= gas_limit * (base_fee_per_gas + priority_fee_per_gas);
```
Abort conditions (without receipt)
- Insufficient Gas for transaction base cost
- Incorrect nonce value
- Insufficient balance for gas

### Charge

Update balances of signer, proposer and treasury account. Finally, update signer's nonce.

```text
1. ws[signer].balance += (gas_unused) * (base_fee_per_gas + priority_fee_per_gas);
2. ws[proposer].balance += gas_used * priority_fee_per_gas;
3. ws[treasury].balance += TreasuryShare * (gas_used * base_fee_per_gas);
4. ws[signer].nonce += 1;
```

### Work

Work step transits from Tentative charge to either the charge step or another Work step. The order follows the Command Sequence in the transaction. 

#### Transfer

Transfer balance from signer to recipient.

```text
1. ws[signer].balance -= value;
2. ws[recipient].balance += value;
```
Abort conditions (with receipt)
- Insufficient balance to transfer

#### Deploy

Store contract code to state with CBI version.

```text
1. contract_addr = sha256((signer, nonce));
2. contract_module = instantiate(contract, cbi_version);
3. ws[contract_addr].contract = contract_module;
4. ws[contract_addr].cbi_version = cbi_version;
```
Abort conditions (with receipt)
- Contract Instantiation fails

#### Call

Get contract code from state according to CBI version and call it.

```text
1. contract_module = ws[target].contract_module;
2. cbi_version = ws[target].cbi_version;
3. instance = instantiate(contract_module, cbi_version);
4. call(instance.action, cbi_version);
```
Abort conditions (with receipt)
- Contract Instantiation fails
- Contract Call fails

A called contract can trigger deferred commands which are executed after successfully executing this Call Command. The result of executing deferred commands will update `gas_used` in the `CommandReceipt` of the original Call. If deferred command fails, the `exit_status` in `CommandReceipt` of the orignal call is also failed.

#### Create Pool

Create pool by setting operator, commission rate with empty power and delegated stakes.

```text
1. pools[operator].operator = operator;
2. pools[operator].commission_rate = commission_rate;
3. pools[operator].power = 0;
4. pools[operator].delegated_stakes = [];
```
Abort conditions (with receipt)
- Pool already exists
- Commission Rate > 100

#### Set Pool Settings

Set pool settings by setting value of commission rate.

```text
1. pools[operator].commission_rate = commission_rate;
```
Abort conditions (with receipt)
- Pool does not exist
- Commission Rate > 100 or Commission Rate is already set to same value

#### Delete Pool

Delete pool and remove from next validator set (if applicable).

```text
1. next_validator_set.remove(operator);
2. pools[operator].delete();
```
Abort conditions (with receipt)
- Pool does not exist

#### Create Deposit

Create deposit by transferring balance to deposit and setting up auto-stake-rewards.

```text
1. ws[owner].balance -= balance;
2. deposits[(operator, owner)].balance = balance;
3. deposits[(operator, owner)].auto_stake_rewards = auto_stake_rewards;
```
Abort conditions (with receipt)
- Pool does not exist
- Deposit already exists
- Insufficient balance to create deposit

#### Set Deposit Settings

Set deposit settings by updating auto-stake-rewards.

```text
1. deposits[(operator, owner)].auto_stake_rewards = auto_stake_rewards;
```
Abort conditions (with receipt)
- Deposit does not exist

#### Top Up Deposit

Top up deposit by transferring balance to deposit.

```text
1. ws[owner].balance -= amount;
2. deposits[(operator, owner)].balance += amount;
```
Abort conditions (with receipt)
- Deposit does not exist
- Insufficient balance to transfer to deposit

#### Withdraw Deposit

Withdraw deposit and reduce Pool's power. The requested withdrawl amount could be less even if the transaction succeeds. The actual withdrawal amount is limited by factors: previous epoch locked power, current epoch locked power. The maximum of the two numbers defines a threshold that the resulting deposit cannot be lower than that. It must follow the rule: **Resulting Deposit Balance >= max(previous epoch locked power, current epoch locked power)** and **actual withdrawal amount > 0**.

Example:

|current deposit balance|current epoch locked power|previous epoch locked power| requested withdrawal amount | actual withdrawal amount|
|:---|:---|:---|:---|:---|
|10|8|7|1|1|
|10|**8**|7|3|10 - 8 = 2|
|10|4|**6**|5|10 - 6 = 4|

In the above example, case 1 is fine as the resulting deposit balance (10-1=9) is greater than two locked powers. For the rest of the cases, the actual withdrawal amount is less because it is limited by the locked power.

```text
1. prev_epoch_locked_power = prev_validator_set.get(operator).get_stake(owner);
2. current_epoch_locked_power = current_validator_set.get(operator).get_stake(owner);
3. cur_deposit_balance = deposits[(operator, owner)].balance;
4. new_deposit_balance = calculate_resulting_balance(prev_epoch_locked_power, current_epoch_locked_power, cur_deposit_balance, requested_withdrawal_amount);
5. deposits[(operator, owner)].delete() if new_deposit_balance = 0, otherwise
   deposits[(operator, owner)].balance = new_deposit_balance
6. ws[owner].balance += cur_deposit_balance - new_deposit_balance;
7. stake = pools[operator].get_stake(owner);
8. pools[operator].change_stake_power(owner, new_deposit_balance) if stake.power > new_deposit_balance;
```
Abort conditions (with receipt)
- Deposit does not exist
- Nothing to withdrawal (to be determined by calculating the allowable withdrawal amount)

#### Stake Deposit

Stake deposit to a pool and update Pool's power. The resulting stake must not be greater than the deposit balance.

```text
1. cur_deposit_balance = deposits[(operator, owner)].balance;
2. stake = pools[operator].get_stake(owner);
3. new_stake_power = min(cur_deposit_balance, stake.power + requested_stake_amount);
4. pools[operator].update_stake_power(owner, new_stake_power);
```
Abort conditions (with receipt)
- Deposit does not exist
- Pool does not exist
- Nothing to stake (to be determined by calculating the allowable stake amount)

#### Unstake Deposit

Unstake deposit from a pool and reduce pool's power.

```text
1. stake = pools[operator].get_stake(owner);
2. new_power = stake.power - requested_unstake_amount;
3. pools[operator].remove_stake(owner) if new_power is empty;
4. pools[operator].change_balance(owner, new_power);
```
Abort conditions (with receipt)
- Deposit does not exist
- Pool does not exist
- Pool has no owner's stake to unstake 

#### Next Epoch

The next epoch transaction does the following:
1. Reward each Stake in Current Validator Set
    - Increase deposits of owner and operator
    - Update stakes if auto-stake-rewards is enabled
2. Replace Previous Validator Set with Current Validator Set
3. Replace Current Validator Set with Next Validator Set
4. Bump up Current Epoch by 1
5. Return Next Validator Set for next leader selection

## World State

Runtime's Read Write Operations are not immediately reflected on World State for performance purpose. There is a structure called `ReadWriteSet` as a cache layer on top of World State. It consists of Read Set and Write Set:

- Read Set: cache firsthand data obtained from world state.
- Write Set: cache writes pending to write to world state.

In a Read Operation, Write Set is accessed first. If data is not found, search Read Set. If it fails in both Sets, then finally World State is accessed. The result will then be cached to Read Set.

In a Write Operation, it first performs a Read Operation, and then updates the Write Set with the newest data.

At the end of state transition, if it succeeds, the data in Write Set will be committed to World State. Otherwise, the Write Set is discarded without any changes to World State.

## Gas Cost

There are different categories on Gas Cost:

Blockchain Cost. The cost to include data in a block. For example, the transaction base cost and the cost for adding data (logs, return value) to a receipt.

Storage Cost. The cost to access the data in world state.
- Contains: check existence of a key in storage
- Read: read value for a key in storage
- Write: write value for a key in storage

Wasm Execution Gas Cost. The cost of executing a WASM contract.

Execution of WASM contract can be exited earlier even if Wasm Execution Gas does not reach the gas limit. It is because runtime will reduce the available gas by Non-Wasm execution gas cost (Blockchain Cost, Storage Cost) which is incurred during the execution.


