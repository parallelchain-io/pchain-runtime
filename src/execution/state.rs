/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct as Execution State which is being updated during execution. 
//! 
//! This state is not as same as the concept of state in World State. Execution encapsulates the changing information 
//! during execution life-cycle. It is the state of execution model, but not referring to blockchain storage.

use std::ops::{Deref, DerefMut};

use pchain_world_state::{storage::WorldStateStorage, network::{network_account::NetworkAccountStorage, constants::NETWORK_ADDRESS}, keys::AppKey};

use crate::{types::BaseTx, BlockchainParams, transition::TransitionContext};


/// ExecutionState is a collection of all useful information required to transit an state through Phases.
/// Methods defined in ExecutionState do not directly update data to world state, but associate with the
/// [crate::read_write_set::ReadWriteSet] in [TransitionContext] which serves as a data cache in between runtime and world state.
pub(crate) struct ExecutionState<S> 
    where S: WorldStateStorage + Send + Sync + Clone +'static 
{
    /*** Transaction ***/

    /// Base Transaction as a transition input
    pub tx: BaseTx,
    /// size of serialized Transaction
    pub tx_size: usize,
    /// length of commands in the transaction
    pub commands_len: usize,

    /*** Blockchain ***/
    
    /// Blockchain data as a transition input
    pub bd: BlockchainParams,
    
    /*** World State ***/

    /// Transition Context which also contains world state as input
    pub ctx: TransitionContext<S>,
}

impl<S> Deref for ExecutionState<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    type Target = TransitionContext<S>;

    fn deref(&self) -> &Self::Target {
        &self.ctx    
    }
}

impl<S> DerefMut for ExecutionState<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ctx    
    }
}

/// ExecutionState implements NetworkAccountStorage with Read Write operations that:
/// - Gas is charged in every Get/Contains/Set
/// - Account Storage State (for app data) is opened in every Set to contract storage
impl<S> NetworkAccountStorage for ExecutionState<S> 
    where S: WorldStateStorage + Send + Sync + Clone
{
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec())).0
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.contains_app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec()))
    }

    fn set(&mut self, key: &[u8], value: Vec<u8>) {
        self.set_app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec()), value);
    }

    fn delete(&mut self, key: &[u8]) {
        self.set_app_data(NETWORK_ADDRESS, AppKey::new(key.to_vec()), Vec::new());
    }
}