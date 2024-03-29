/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct that implements operations of reading and writing byte arrays into the linear memory of the Wasm instance.

use anyhow::{anyhow, Result};
use wasmer::{Array, Memory, NativeFunc, WasmPtr};

/// Provides read-write access to Wasm linear memory through the [Wasmer Environment](crate::contract::wasmer::env).
/// This Memory context interfaces with Wasmer's exports to facilitate memory operations.
pub trait MemoryContext {
    fn memory(&self) -> &Memory;
    fn alloc(&self) -> &NativeFunc<u32, WasmPtr<u8, Array>>;

    /// set the return values to memory and return the length
    fn write_bytes_to_memory(&self, value: Vec<u8>, val_ptr_ptr: u32) -> Result<u32> {
        let memory = self.memory();
        let alloc = self.alloc();

        // Allocate segment.
        let segment_ptr = alloc
            .call(value.len() as u32)
            .map_err(|err| anyhow!("MODERATE: fail to allocate linear memory: {}", err))?;

        // Write bytes.
        let segment = segment_ptr
            .deref(memory, 0, value.len() as u32)
            .ok_or(anyhow!("MODERATE: fail to dereference linear memory"))?;

        for i in 0..value.len() {
            segment[i].set(value[i]);
        }

        let val_offset = segment_ptr.offset();
        let value_len = value.len();

        // Write linear memory offset (val_offset) to the memory segment pointed to by `val_ptr_ptr`
        let val_ptr_ptr: WasmPtr<u32, wasmer::Array> = WasmPtr::new(val_ptr_ptr);
        let val_ptr_segment = val_ptr_ptr.deref(memory, 0, 1).unwrap();
        val_ptr_segment[0].set(val_offset);

        Ok(value_len as u32)
    }

    /// read bytes from memory given the offset and len of the memory location
    fn read_bytes_from_memory(&self, offset: u32, len: u32) -> Result<Vec<u8>> {
        let memory = self.memory();
        let bytes_ptr: WasmPtr<u8, Array> = WasmPtr::new(offset);

        let bytes = bytes_ptr
            .deref(memory, 0, len)
            .ok_or(anyhow!("MODERATE: fail to read bytes from linear memory"))?;

        let mut bytes_copy = Vec::new();
        for byte in bytes {
            bytes_copy.push(byte.get());
        }

        Ok(bytes_copy)
    }
}
