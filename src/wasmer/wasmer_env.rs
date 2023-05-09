/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! wasmer_env defines environment for constructing instance of wasmer execution.

use std::{
    convert::TryInto, 
    sync::{Arc, Mutex}, 
    mem::MaybeUninit
};
use pchain_world_state::{
    storage::WorldStateStorage
};
use wasmer::{LazyInit, Memory, NativeFunc, Global};
use anyhow::Result;

use crate::{
    transition::TransitionContext,
    wasmer::wasmer_memory::MemoryContext, 
    gas::{CostChange, self}, 
    types::CallTx, BlockchainParams, contract::FuncError
};

/// Env provides the functions in `exports` (which are in turn 'imported' by WASM smart contracts)
/// access to complex functionality that typically cannot cross the host-WASM barrier.
///
/// Wasmer handles everything for us.
#[derive(wasmer::WasmerEnv, Clone)]
pub(crate) struct Env<S> where S: WorldStateStorage + Send + Sync + Clone + 'static {
    /// Transition Context
    pub context: Arc<Mutex<TransitionContext<S>>>,

    /// counter of calls. It starts with zero and increases for every Internal Calls
    pub call_counter: u32,

    /// gas meter for wasm execution.
    pub gas_meter: Arc<Mutex<MaybeUninit<GasMeter>>>,

    /// Call Transaction consists of information such as target_address, gas limit, and data which is parameters provided to contract.
    /// In Internal Call, target address of the contract being called could be child contract.
    pub call_tx: CallTx,

    /// Blockchain data as an input to state transition
    pub params_from_blockchain: BlockchainParams,

    /// Indicator of whether this environment is used in view calls.
    pub is_view: bool,

    #[wasmer(export)]
    pub memory: LazyInit<Memory>,

    #[wasmer(export(name="alloc"))]
    pub alloc: LazyInit<NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>>>,
}

impl<S> Env<S> where S: WorldStateStorage + Send + Sync + Clone + 'static {
    /// env is a helper function to create an Env, which is an object used in functions exported to smart
    /// contract modules.
    pub fn new(
        context: Arc<Mutex<TransitionContext<S>>>,
        call_counter: u32,
        is_view: bool,
        call_tx: CallTx,
        params_from_blockchain: BlockchainParams
    ) -> Env<S> {

        Env {
            context,
            call_counter,
            gas_meter: Arc::new(Mutex::new(MaybeUninit::uninit())),
            memory: LazyInit::default(),
            alloc: LazyInit::default(),
            call_tx,
            params_from_blockchain,
            is_view
        }
    }

    /// initialize the variable wasmer_remaining_points
    pub fn init_wasmer_remaining_points(&self, global: Global) {
        self.gas_meter.lock().unwrap().write(
            GasMeter { 
                wasmer_gas: global, 
                non_wasmer_gas_amount: 0 
        });
    }

    /// drop the variable wasmer_remaining_points
    pub fn drop_wasmer_remaining_points(&self) {
        unsafe { self.gas_meter.lock().unwrap().assume_init_drop() };
    }

    /// get remaining points (gas) of wasm execution
    pub fn get_wasmer_remaining_points(&self) -> u64 {
        unsafe {
            self.gas_meter.lock().unwrap().assume_init_ref().wasmer_gas.get().try_into().unwrap()
        }
    }

    /// get the recorded non-wasm gas amount from gas meter
    pub fn get_non_wasm_gas_amount(&self) -> u64 {
        unsafe {
            self.gas_meter.lock().unwrap().assume_init_ref().non_wasmer_gas_amount
        }
    }

    /// substract remaining points of wasm execution and record the amount to non_wasmer_gas_amount
    pub fn consume_non_wasm_gas(&self, change: CostChange) {
        // rewards is not useful in substracting remaining points. It is fine because it will 
        // eventaully be used to reduce gas consumption of the transaction, but here we just do not 
        // want to extend the wasm execution time.
        let (deduct, _) = change.values();
        if deduct > 0 {
            unsafe {
                self.gas_meter.lock().unwrap()
                    .assume_init_mut()
                    .substract_non_wasmer_gas(deduct);
            }
        }
    }

    /// substract remaining points of wasm execution
    pub fn consume_wasm_gas(&self, gas_consumed: u64) -> u64 {
        let gas_meter_lock = self.gas_meter.lock().unwrap();
        unsafe {
            gas_meter_lock.assume_init_ref().substract(gas_consumed)
        }
    }

    /// write the data to memory, charge the write cost and return the length
    pub fn write_bytes(&self, value :Vec<u8>, val_ptr_ptr :u32) -> Result<u32, FuncError> {
        let (deduct, _) = gas::wasm_memory_write_cost(value.len()).values();
        if deduct > 0 {
            self.consume_wasm_gas(deduct);
        }
        MemoryContext::write_bytes_to_memory(self, value, val_ptr_ptr).map_err(FuncError::Runtime)
    }

    /// read data from memory and charge the read cost
    pub fn read_bytes(&self, offset: u32, len: u32) -> Result<Vec<u8>, FuncError> {
        let (deduct, _) = gas::wasm_memory_read_cost(len as usize).values();
        if deduct > 0 {
            self.consume_wasm_gas(deduct);
        }
        MemoryContext::read_bytes_from_memory(self, offset, len).map_err(FuncError::Runtime)
    }

}

impl<S> MemoryContext for Env<S> where S: WorldStateStorage + Send + Sync + Clone + 'static {
    fn get_memory(&self) -> &Memory {
        self.memory_ref().unwrap()
    }

    fn get_alloc(&self) -> &NativeFunc<u32, wasmer::WasmPtr<u8, wasmer::Array>> {
        self.alloc_ref().unwrap()
    }
}

/// GasMeter defines gas metering logics. It is composed of the gas related to Wasmer Execution and the
/// non-Wasmer related gas amount.
pub(crate) struct GasMeter {
    /// global vaiable of wasmer_middlewares::metering remaining points.
    wasmer_gas: wasmer::Global,
        
    /// the gas accounted as part of the wasm execution gas during execution for eariler exiting when
    /// gas becomes insufficient. This value is useful in deriving the gas used only for wasm execution.
    non_wasmer_gas_amount: u64
}

impl GasMeter {
    /// substract amount from wasmer_gas
    fn substract(&self, amount: u64) -> u64 {
        let current_remaining_points: u64 = self.wasmer_gas.get().try_into().unwrap();
        let new_remaining_points = current_remaining_points.saturating_sub(amount);
        self.wasmer_gas.set(new_remaining_points.into()).expect("Can't subtract `wasmer_metering_remaining_points` in Env");
        new_remaining_points
    }

    /// subtract amount from wasmer_gas, and record the amount of non_wasmer_gas.
    fn substract_non_wasmer_gas(&mut self, amount: u64) -> u64 {
        self.non_wasmer_gas_amount = self.non_wasmer_gas_amount.saturating_add(amount);
        self.substract(amount)
    }
}