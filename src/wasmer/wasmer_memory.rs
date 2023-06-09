/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a struct that implements operations of reading and writing byte arrays into WASM linear memory.

use anyhow::{anyhow, Result};
use wasmer::{Array, Memory, NativeFunc, WasmPtr};

/// Memory context is used by [Wasmer Environment](super::wasmer_env) for read-write access to WASM linear memory.
pub trait MemoryContext {
    fn get_memory(&self) -> &Memory;
    fn get_alloc(&self) -> &NativeFunc<u32, WasmPtr<u8, Array>>;

    /// set the return values to memory and return the length
    fn write_bytes_to_memory(&self, value: Vec<u8>, val_ptr_ptr: u32) -> Result<u32> {
        let memory = self.get_memory();
        let alloc = self.get_alloc();

        // Allocate segment.
        let segment_ptr = match alloc.call(value.len() as u32) {
            Ok(ptr) => ptr,
            Err(err) => return Err(anyhow!("MODERATE: fail to allocate linear memory: {}", err)),
        };

        // Write bytes.
        let segment = match segment_ptr.deref(memory, 0, value.len() as u32) {
            Some(cell) => cell,
            None => return Err(anyhow!("MODERATE: fail to dereference linear memory")),
        };
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
        let memory = self.get_memory();
        let bytes_ptr: WasmPtr<u8, Array> = WasmPtr::new(offset);

        let bytes = match bytes_ptr.deref(memory, 0, len) {
            Some(bytes) => bytes,
            None => return Err(anyhow!("MODERATE: fail to read bytes from linear memory")),
        };
        let mut bytes_copy = Vec::new();
        for byte in bytes {
            bytes_copy.push(byte.get());
        }

        Ok(bytes_copy)
    }
}
