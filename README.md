# ParallelChain Mainnet Runtime

ParallelChain Mainnet Runtime is a **State Transition Function** to transit from an input state of the blockchain to next state. It is also the sole system component to handle Smart Contract that is primarily built from Rust code by using ParallelChain F Smart Contract Development Kit (SDK).

```
f(WS, BD, TX) -> (WS', R)

WS = World state represented by set of key-value pairs
BD = Blockchain Data
TX = Transaction, which is essentially a sequence of Commands
R = Receipt, which is a sequence of Command Receipts correspondingly.
```

## Getting Started

High level description on implementation of `pchain-runtime` can refer to [Parallelchain Protocol](https://github.com/parallelchain-io/parallelchain-protocol).

Data types related to blockchain that are frequently used can be found on the crate [ParallelChain Types](https://crates.io/crates/pchain-types).

Blockchain State model (World State) can refer to the crate [ParallelChain World State](https://github.com/parallelchain-io/pchain-world-state).

The mentioned Smart Contract Development Kit can refer to the crate [ParallelChain Smart Contract SDK](https://crates.io/crates/pchain-sdk).

## Repository Structure

Major modules of this repository are organized as following:

Execution:

- `transition`: Entry point of **state transition function**.
- `execution`: Implementation of execution process.
- `wasmer`: Implementation of components to use [wasmer](https://wasmer.io/) as Smart Contract Execution Runtime.
- `contract`: Implementation of components related Smart Contract, such as instantiation, host/guess function interface, etc.

Types and Data Model:

- `types`: General data types used in Runtime.
- `read_write_set`: Implementation of data read-write operations to World State.

Constants:

- `formulas`: General constants and equations used in Runtime.
- `gas`: Specific to gas calculation functions and constants.

## Versioning 

The version of this library reflects the version of the ParallelChain Protocol which it implements. For example, the current version is 0.4.2, and this implements protocol version 0.4. Patch version increases are not guaranteed to be non-breaking.

## Opening an issue

Open an issue in GitHub if you:

1. Have a feature request / feature idea,
2. Have any questions (particularly software related questions),
3. Think you may have discovered a bug.

Please try to label your issues appropriately.