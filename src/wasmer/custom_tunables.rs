/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines structs that are used as configuration of wasmer store for limiting the memory
//! used for wasm module instantiation.

use loupe::MemoryUsage;
use std::ptr::NonNull;
use std::sync::Arc;
use wasmer::{
    vm::{self, MemoryError, MemoryStyle, TableStyle, VMMemoryDefinition, VMTableDefinition},
    MemoryType, Pages, TableType, Tunables,
};

/// CustomTunables allows the setting of an upper limit on the guest memory of the VM.
/// CustomTunables also conists of a base Tunables attribute which all the existing logic is delegated to after
/// guest memory adjustment  
#[derive(MemoryUsage)]
pub struct CustomTunables<T: Tunables> {
    /// maximum allowable guest memory (in WASM pages, each approximately 65KiB in size)
    limit: Pages,

    /// base implementation we delegate all the logic to after guest memory adjustment
    base: T,
}

impl<T: Tunables> Tunables for CustomTunables<T> {
    /// `memory_style` is used to construct a WebAssembly `MemoryStyle` for the provided `MemoryType` using base tunables
    /// For more information on `memory_style`: See <https://github.com/wasmerio/wasmer/blob/master/examples/tunables_limit_memory.rs>
    fn memory_style(&self, memory: &MemoryType) -> MemoryStyle {
        let adjusted = self.adjust_memory(memory);
        self.base.memory_style(&adjusted)
    }

    /// `table_style` is used to construct a WebAssembly `TableStyle` for the provided `TableType` using base tunables
    /// For more information on `table_style`: See <https://github.com/wasmerio/wasmer/blob/master/examples/tunables_limit_memory.rs>
    fn table_style(&self, table: &TableType) -> TableStyle {
        self.base.table_style(table)
    }

    /// `create_host_memory` creates memory owned by the host given a WebAssembly `MemoryType` and a `MemoryStyle`
    /// The requested memory type is validated, adjusted to the limited and then passed to base tunables
    /// For more information on `create_host_memory`: See <https://github.com/wasmerio/wasmer/blob/master/examples/tunables_limit_memory.rs>
    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<Arc<dyn vm::Memory>, MemoryError> {
        let adjusted = self.adjust_memory(ty);
        self.validate_memory(&adjusted)?;
        self.base.create_host_memory(&adjusted, style)
    }

    /// `create_vm_memory` creates memory owned by the VM given a WebAssembly `MemoryType` and a `MemoryStyle`.
    /// For more information on `create_vm_memory`: See <https://github.com/wasmerio/wasmer/blob/master/examples/tunables_limit_memory.rs>
    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<Arc<dyn vm::Memory>, MemoryError> {
        let adjusted = self.adjust_memory(ty);
        self.validate_memory(&adjusted)?;
        self.base
            .create_vm_memory(&adjusted, style, vm_definition_location)
    }

    // `create_host_table` creates a table owned by the host given a WebAssembly `TableType` and a `TableStyle`.
    // For more information on `create_host_table`: See <https://github.com/wasmerio/wasmer/blob/master/examples/tunables_limit_memory.rs>
    fn create_host_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
    ) -> Result<Arc<dyn vm::Table>, String> {
        self.base.create_host_table(ty, style)
    }

    // `create_vm_table` creates a table owned by the VM given a WebAssembly `TableType` and a `TableStyle`.
    // For more information on `create_vm_table`: See <https://github.com/wasmerio/wasmer/blob/master/examples/tunables_limit_memory.rs>
    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<Arc<dyn vm::Table>, String> {
        self.base.create_vm_table(ty, style, vm_definition_location)
    }
}

impl<T: Tunables> CustomTunables<T> {
    pub fn new(base: T, limit: Pages) -> Self {
        Self { limit, base }
    }

    // `adjust_memory` accepts an input memory descriptor requested by guest and sets
    // a maximum limit for the descriptor, if not assigned earlier.
    fn adjust_memory(&self, requested: &MemoryType) -> MemoryType {
        let mut adjusted = *requested;
        if requested.maximum.is_none() {
            adjusted.maximum = Some(self.limit);
        }
        adjusted
    }

    // `validate_memory` ensures that the number of pages in the memory descriptor does not
    // exceed the preset memory limit. It should be called in sequence after `adjust_memory`.
    fn validate_memory(&self, ty: &MemoryType) -> Result<(), MemoryError> {
        if ty.minimum > self.limit {
            return Err(MemoryError::Generic(
                "Minimum limit exceeds the allowed memory limit".to_string(),
            ));
        }

        if let Some(max) = ty.maximum {
            if max > self.limit {
                return Err(MemoryError::Generic(
                    "Maximum limit exceeds the allowed memory limit".to_string(),
                ));
            }
        } else {
            return Err(MemoryError::Generic("Maximum limit unset".to_string()));
        }

        Ok(())
    }
}
