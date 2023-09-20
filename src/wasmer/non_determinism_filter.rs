/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines a middleware filter to disallow non-deterministic operations.
//!
//! Non-deterministic operations refer to some specific Opcodes in wasm execution,
//! for example, floating point operations.

use loupe::MemoryUsage;
use wasmer::{
    wasmparser::Operator, FunctionMiddleware, LocalFunctionIndex, MiddlewareError,
    MiddlewareReaderState, ModuleMiddleware,
};

/// NonDeterminismFilterConfig defines boolean flags specific to each opcode family.
/// This is an attribute for the NonDeterminismFilter middleware defined below.
#[derive(Debug, MemoryUsage, Clone, Copy)]
struct NonDeterminismFilterConfig {
    /// allow_floating_point_ops is a flag to enable/disable sequential floating point operations.
    /// Note: This feature is known to induce non-determinism and is encouraged to be set as false.
    /// See <https://github.com/WebAssembly/design/blob/main/Nondeterminism.md>
    allow_floating_point_ops: bool,
    /// allow_simd_ops is a flag to enable/disable fixed width SIMD operations.
    /// Note: There are floating point operations described in WASM SIMD Instructions
    /// which are known to induce non-determinism and is encouraged to be set as false.
    /// See <https://github.com/WebAssembly/simd/blob/main/proposals/simd/SIMD.md>  <https://github.com/WebAssembly/design/blob/main/Nondeterminism.md>
    allow_simd_ops: bool,
    /// allow_atomic_ops is a flag to enable/disable atomic operations with WASM threads.
    /// Note: They are known to induce non-determinism due to hardware standardization constraints and are encouraged to be set as false.
    /// See <https://github.com/WebAssembly/design/blob/main/Nondeterminism.md>
    allow_atomic_ops: bool,
    /// allow_bulk_memory_operations is a flag to enable/disable bulk memory operations.
    /// See <https://github.com/WebAssembly/bulk-memory-operations>
    allow_bulk_memory_operations: bool,
    /// allow_reference_types is a flag to enable/disable reference types.
    /// See <https://github.com/WebAssembly/reference-types>
    allow_reference_types: bool,
    /// allow_exception_handling is a flag to enable/disable exception handling.
    /// See <https://github.com/WebAssembly/exception-handling/blob/main/proposals/exception-handling/Exceptions.md>
    allow_exception_handling: bool,
}

/// NonDeterminismFilter is the middleware that disallows use of features from WASM which may induce non-determinism.
#[derive(Debug, MemoryUsage)]
#[non_exhaustive]
pub struct NonDeterminismFilter {
    config: NonDeterminismFilterConfig,
}

impl NonDeterminismFilter {
    // create spins up a new instance for NonDeterminismFilter middleware with custom config setting.
    // Currently set to private.The access is given through a default implementation with a preset
    // config setting.
    fn create(config: NonDeterminismFilterConfig) -> Self {
        Self { config }
    }
}

impl Default for NonDeterminismFilter {
    // default is an implementation for NonDeterminismFilter that loads
    // a set of boolean flags on NonDeterminismFilterConfig when the method "default" is called.
    fn default() -> Self {
        Self::create(NonDeterminismFilterConfig {
            // floating point operations are set to false to promote determinism inside the ParallelChain Mainnet ecosystem.
            allow_floating_point_ops: false,
            // simd ops are set to false to promote determinism inside the ParallelChain Mainnet ecosystem.
            allow_simd_ops: false,
            // atomic operations are set to false as they need WASM threads to execute.
            allow_atomic_ops: false,
            // bulk memory operations are set to true.
            allow_bulk_memory_operations: true,
            // reference types are set to true.
            allow_reference_types: true,
            // exception handling has been set to true.
            allow_exception_handling: true,
        })
    }
}

impl ModuleMiddleware for NonDeterminismFilter {
    fn generate_function_middleware(&self, _: LocalFunctionIndex) -> Box<dyn FunctionMiddleware> {
        Box::new(FunctionNonDeterminismFilter::new(self.config))
    }
}

#[derive(Debug)]
#[non_exhaustive]
struct FunctionNonDeterminismFilter {
    config: NonDeterminismFilterConfig,
}

impl FunctionNonDeterminismFilter {
    fn new(config: NonDeterminismFilterConfig) -> Self {
        Self { config }
    }
}

/// FunctionMiddleware enables checks for each WASM opcode family
/// Raises MiddlewareError if the corresponding flag in NonDeterminismFilterConfig is false
impl FunctionMiddleware for FunctionNonDeterminismFilter {
    // Process the given operator.
    fn feed<'a>(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> Result<(), MiddlewareError> {
        match operator {
            // Opcode family of Reference types
            Operator::RefNull { .. }
            | Operator::RefIsNull
            | Operator::RefFunc { .. }
            | Operator::ReturnCall { .. }
            | Operator::ReturnCallIndirect { .. }
            | Operator::TypedSelect { .. }
            | Operator::TableGet { .. }
            | Operator::TableSet { .. }
            | Operator::TableGrow { .. }
            | Operator::TableSize { .. } => {
                if self.config.allow_reference_types {
                    state.push_operator(operator);
                    Ok(())
                } else {
                    let msg = "OpcodeError: Reference Types";
                    Err(MiddlewareError::new(" ", msg))
                }
            }
            // Opcode family of Atomic operations using WASM threads
            Operator::MemoryAtomicNotify { .. }
            | Operator::MemoryAtomicWait32 { .. } | Operator::MemoryAtomicWait64 { .. } | Operator::AtomicFence { .. } | Operator::I32AtomicLoad { .. }
            | Operator::I64AtomicLoad { .. } | Operator::I32AtomicLoad8U { .. } | Operator::I32AtomicLoad16U { .. } | Operator::I64AtomicLoad8U { .. }
            | Operator::I64AtomicLoad16U { .. } | Operator::I64AtomicLoad32U { .. } | Operator::I32AtomicStore { .. } | Operator::I64AtomicStore { .. }
            | Operator::I32AtomicStore8 { .. } | Operator::I32AtomicStore16 { .. } | Operator::I64AtomicStore8 { .. } | Operator::I64AtomicStore16 { .. }
            | Operator::I64AtomicStore32 { .. } | Operator::I32AtomicRmwAdd { .. } | Operator::I64AtomicRmwAdd { .. } | Operator::I32AtomicRmw8AddU { .. }
            | Operator::I32AtomicRmw16AddU { .. } | Operator::I64AtomicRmw8AddU { .. } | Operator::I64AtomicRmw16AddU { .. } | Operator::I64AtomicRmw32AddU { .. }
            | Operator::I32AtomicRmwSub { .. } | Operator::I64AtomicRmwSub { .. } | Operator::I32AtomicRmw8SubU { .. } | Operator::I32AtomicRmw16SubU { .. }
            | Operator::I64AtomicRmw8SubU { .. } | Operator::I64AtomicRmw16SubU { .. } | Operator::I64AtomicRmw32SubU { .. } | Operator::I32AtomicRmwAnd { .. }
            | Operator::I64AtomicRmwAnd { .. } | Operator::I32AtomicRmw8AndU { .. } | Operator::I32AtomicRmw16AndU { .. } | Operator::I64AtomicRmw8AndU { .. }
            | Operator::I64AtomicRmw16AndU { .. } | Operator::I64AtomicRmw32AndU { .. } | Operator::I32AtomicRmwOr { .. } | Operator::I64AtomicRmwOr { .. }
            | Operator::I32AtomicRmw8OrU { .. } | Operator::I32AtomicRmw16OrU { .. } | Operator::I64AtomicRmw8OrU { .. } | Operator::I64AtomicRmw16OrU { .. }
            | Operator::I64AtomicRmw32OrU { .. } | Operator::I32AtomicRmwXor { .. } | Operator::I64AtomicRmwXor { .. } | Operator::I32AtomicRmw8XorU { .. }
            | Operator::I32AtomicRmw16XorU { .. } | Operator::I64AtomicRmw8XorU { .. } | Operator::I64AtomicRmw16XorU { .. } | Operator::I64AtomicRmw32XorU { .. }
            | Operator::I32AtomicRmwXchg { .. } | Operator::I64AtomicRmwXchg { .. } | Operator::I32AtomicRmw8XchgU { .. } | Operator::I32AtomicRmw16XchgU { .. }
            | Operator::I64AtomicRmw8XchgU { .. } | Operator::I64AtomicRmw16XchgU { .. } | Operator::I64AtomicRmw32XchgU { .. } | Operator::I32AtomicRmwCmpxchg { .. }
            | Operator::I64AtomicRmwCmpxchg { .. } | Operator::I32AtomicRmw8CmpxchgU { .. } | Operator::I32AtomicRmw16CmpxchgU { .. } | Operator::I64AtomicRmw8CmpxchgU { .. }
            | Operator::I64AtomicRmw16CmpxchgU { .. } | Operator::I64AtomicRmw32CmpxchgU { .. } 
            => {
                if self.config.allow_atomic_ops {
                    state.push_operator(operator);
                    Ok(())
                } else {
                    let msg = "OpcodeError: Atomic Operations";
                    Err(MiddlewareError::new(" ", msg))
                }
            }
            // Opcode family of logical operations, memory and integer type mathematical operators
            Operator::Unreachable
            | Operator::Nop | Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } | Operator::Else | Operator::End | Operator::Br { .. }
            | Operator::BrIf { .. } | Operator::BrTable { .. } | Operator::Return | Operator::Call { .. } | Operator::CallIndirect { .. } | Operator::Drop
            | Operator::Select | Operator::LocalGet { .. } | Operator::LocalSet { .. } | Operator::LocalTee { .. } | Operator::GlobalGet { .. } | Operator::GlobalSet { .. }
            | Operator::I32Load { .. } | Operator::I64Load { .. } | Operator::I32Load8S { .. } | Operator::I32Load8U { .. } | Operator::I32Load16S { .. } | Operator::I32Load16U { .. }
            | Operator::I64Load8S { .. } | Operator::I64Load8U { .. } | Operator::I64Load16S { .. } | Operator::I64Load16U { .. } | Operator::I64Load32S { .. } | Operator::I64Load32U { .. }
            | Operator::I32Store { .. } | Operator::I64Store { .. } | Operator::I32Store8 { .. } | Operator::I32Store16 { .. } | Operator::I64Store8 { .. } | Operator::I64Store16 { .. }
            | Operator::I64Store32 { .. } | Operator::MemorySize { .. } | Operator::MemoryGrow { .. } | Operator::I32Const { .. } | Operator::I64Const { .. } | Operator::I64LtS
            | Operator::I64LtU | Operator::I64GtS | Operator::I64GtU | Operator::I64LeS | Operator::I64LeU | Operator::I64GeS
            | Operator::I64GeU | Operator::I32Clz | Operator::I32Ctz | Operator::I32Eqz | Operator::I32Eq | Operator::I32Ne
            | Operator::I32LtS | Operator::I32LtU | Operator::I32GtS | Operator::I32GtU | Operator::I32LeS | Operator::I32LeU
            | Operator::I32GeS | Operator::I32GeU | Operator::I64Eqz | Operator::I64Eq | Operator::I64Ne | Operator::I32Popcnt
            | Operator::I32Add | Operator::I32Sub | Operator::I32Mul | Operator::I32DivS | Operator::I32DivU | Operator::I32RemS
            | Operator::I32RemU | Operator::I32And | Operator::I32Or | Operator::I32Xor | Operator::I32Shl | Operator::I32ShrS
            | Operator::I32ShrU | Operator::I32Rotl | Operator::I32Rotr | Operator::I64Clz | Operator::I64Ctz | Operator::I64Popcnt
            | Operator::I64Add | Operator::I64Sub | Operator::I64Mul | Operator::I64DivS | Operator::I64DivU | Operator::I64RemS
            | Operator::I64RemU | Operator::I64And | Operator::I64Or | Operator::I64Xor | Operator::I64Shl | Operator::I64ShrS
            | Operator::I64ShrU | Operator::I64Rotl | Operator::I64Rotr | Operator::I32WrapI64 | Operator::I32Extend8S | Operator::I32Extend16S
            | Operator::I64Extend8S | Operator::I64Extend16S | Operator::I64ExtendI32S | Operator::I64Extend32S | Operator::I64ExtendI32U 
            => {
                state.push_operator(operator);
                Ok(())
            }
            // Opcode family of fixed width SIMD operations
            Operator::V128Load { .. } | Operator::V128Store { .. } | Operator::V128Const { .. } | Operator::I8x16Splat | Operator::I8x16ExtractLaneS { .. }
            | Operator::I8x16ExtractLaneU { .. } | Operator::I8x16LaneSelect { .. } | Operator::I8x16ReplaceLane { .. } | Operator::I8x16RelaxedSwizzle { .. } | Operator::I16x8Splat
            | Operator::I16x8ExtractLaneS { .. } | Operator::I16x8ExtractLaneU { .. } | Operator::I16x8LaneSelect { .. } | Operator::I16x8ReplaceLane { .. } | Operator::I32x4Splat
            | Operator::I32x4ExtractLane { .. } | Operator::I32x4LaneSelect { .. } | Operator::I32x4ReplaceLane { .. } | Operator::I64x2Splat | Operator::I64x2ExtractLane { .. }
            | Operator::I64x2LaneSelect { .. } | Operator::I64x2ReplaceLane { .. } | Operator::I8x16Eq | Operator::I8x16Ne | Operator::I8x16LtS
            | Operator::I8x16LtU | Operator::I8x16GtS | Operator::I8x16GtU | Operator::I8x16LeS | Operator::I8x16LeU
            | Operator::I8x16GeS | Operator::I8x16GeU | Operator::I16x8Eq | Operator::I16x8Ne | Operator::I16x8LtS
            | Operator::I16x8LtU | Operator::I16x8GtS | Operator::I16x8GtU | Operator::I16x8LeS | Operator::I16x8LeU
            | Operator::I16x8GeS | Operator::I16x8GeU | Operator::I32x4Eq | Operator::I32x4Ne | Operator::I32x4LtS
            | Operator::I32x4LtU | Operator::I32x4GtS | Operator::I32x4GtU | Operator::I32x4LeS | Operator::I32x4LeU
            | Operator::I32x4GeS | Operator::I32x4GeU | Operator::V128Not | Operator::V128And | Operator::V128AndNot
            | Operator::V128Or | Operator::V128Xor | Operator::V128Bitselect | Operator::I8x16Abs | Operator::I8x16Neg
            | Operator::V128AnyTrue | Operator::I8x16AllTrue | Operator::I8x16Bitmask | Operator::I8x16Shl | Operator::I8x16ShrS
            | Operator::I8x16ShrU | Operator::I8x16Add | Operator::I8x16AddSatS | Operator::I8x16AddSatU | Operator::I8x16Sub
            | Operator::I8x16SubSatS | Operator::I8x16SubSatU | Operator::I8x16MinS | Operator::I8x16MinU | Operator::I8x16MaxS
            | Operator::I8x16MaxU | Operator::I16x8Abs | Operator::I16x8Neg | Operator::I16x8AllTrue | Operator::I16x8Bitmask
            | Operator::I16x8Shl | Operator::I16x8ShrS | Operator::I16x8ShrU | Operator::I16x8Add | Operator::I16x8AddSatS
            | Operator::I16x8AddSatU | Operator::I16x8Sub | Operator::I16x8SubSatS | Operator::I16x8SubSatU | Operator::I16x8Mul
            | Operator::I16x8MinS | Operator::I16x8MinU | Operator::I16x8MaxS | Operator::I16x8MaxU | Operator::I32x4Abs
            | Operator::I32x4Neg | Operator::I32x4AllTrue | Operator::I32x4Bitmask | Operator::I32x4Shl | Operator::I32x4ShrS
            | Operator::I32x4ShrU | Operator::I32x4Add | Operator::I32x4Sub | Operator::I32x4Mul | Operator::I32x4MinS
            | Operator::I32x4MinU | Operator::I32x4MaxS | Operator::I32x4MaxU | Operator::I32x4DotI16x8S | Operator::I64x2Neg
            | Operator::I64x2Shl | Operator::I64x2ShrS | Operator::I64x2ShrU | Operator::I64x2Add | Operator::I64x2Sub
            | Operator::I64x2Mul | Operator::I8x16Swizzle | Operator::I8x16Shuffle { .. } | Operator::V128Load8Splat { .. } | Operator::V128Load16Splat { .. }
            | Operator::V128Load32Splat { .. } | Operator::V128Load32Zero { .. } | Operator::V128Load64Splat { .. } | Operator::V128Load64Zero { .. } | Operator::I8x16NarrowI16x8S
            | Operator::I8x16NarrowI16x8U | Operator::I16x8NarrowI32x4S | Operator::I16x8NarrowI32x4U | Operator::I16x8ExtendLowI8x16S | Operator::I16x8ExtendHighI8x16S
            | Operator::I16x8ExtendLowI8x16U | Operator::I16x8ExtendHighI8x16U | Operator::I32x4ExtendLowI16x8S | Operator::I32x4ExtendHighI16x8S | Operator::I32x4ExtendLowI16x8U
            | Operator::I32x4ExtendHighI16x8U | Operator::V128Load8x8S { .. } | Operator::V128Load8x8U { .. } | Operator::V128Load16x4S { .. } | Operator::V128Load16x4U { .. }
            | Operator::V128Load32x2S { .. } | Operator::V128Load32x2U { .. } | Operator::I8x16RoundingAverageU | Operator::I16x8RoundingAverageU | Operator::V128Load8Lane { .. }
            | Operator::V128Load16Lane { .. } | Operator::V128Load32Lane { .. } | Operator::V128Load64Lane { .. } | Operator::V128Store8Lane { .. } | Operator::V128Store16Lane { .. }
            | Operator::V128Store32Lane { .. } | Operator::V128Store64Lane { .. } | Operator::I64x2Eq | Operator::I64x2Ne | Operator::I64x2LtS
            | Operator::I64x2GtS | Operator::I64x2LeS | Operator::I64x2GeS | Operator::I8x16Popcnt | Operator::I16x8ExtAddPairwiseI8x16S
            | Operator::I16x8ExtAddPairwiseI8x16U | Operator::I16x8Q15MulrSatS | Operator::I16x8ExtMulLowI8x16S | Operator::I16x8ExtMulHighI8x16S | Operator::I16x8ExtMulLowI8x16U
            | Operator::I16x8ExtMulHighI8x16U | Operator::I32x4ExtAddPairwiseI16x8S | Operator::I32x4ExtAddPairwiseI16x8U | Operator::I32x4ExtMulLowI16x8S | Operator::I32x4ExtMulHighI16x8S
            | Operator::I32x4ExtMulLowI16x8U | Operator::I32x4ExtMulHighI16x8U | Operator::I64x2Abs | Operator::I64x2AllTrue | Operator::I64x2Bitmask
            | Operator::I64x2ExtendLowI32x4S | Operator::I64x2ExtendHighI32x4S | Operator::I64x2ExtendLowI32x4U | Operator::I64x2ExtendHighI32x4U | Operator::I64x2ExtMulLowI32x4S
            | Operator::I64x2ExtMulHighI32x4S | Operator::I64x2ExtMulLowI32x4U | Operator::I64x2ExtMulHighI32x4U | Operator::I32x4TruncSatF64x2SZero | Operator::I32x4TruncSatF64x2UZero
            | Operator::I32x4RelaxedTruncSatF64x2SZero | Operator::I32x4RelaxedTruncSatF64x2UZero | Operator::F64x2ConvertLowI32x4S | Operator::F64x2ConvertLowI32x4U | Operator::F32x4DemoteF64x2Zero
            | Operator::F64x2PromoteLowF32x4 => {
                if self.config.allow_simd_ops {
                    state.push_operator(operator);
                    Ok(())
                } else {
                    let msg = "OpcodeError: SIMD Operations";
                    Err(MiddlewareError::new(" ", msg))
                }
            }
            // Opcode family of floating point operations
            Operator::F32Load { .. } | Operator::F64Load { .. } | Operator::F32Store { .. } | Operator::F64Store { .. } | Operator::F32Const { .. }
            | Operator::F64Const { .. } | Operator::F32Eq | Operator::F32Ne | Operator::F32Lt | Operator::F32Gt
            | Operator::F32Le | Operator::F32Ge | Operator::F64Eq | Operator::F64Ne | Operator::F64Lt
            | Operator::F64Gt | Operator::F64Le | Operator::F64Ge | Operator::F32Abs | Operator::F32Neg
            | Operator::F32Ceil | Operator::F32Floor | Operator::F32Trunc | Operator::F32Nearest | Operator::F32Sqrt
            | Operator::F32Add | Operator::F32Sub | Operator::F32Mul | Operator::F32Div | Operator::F32Min
            | Operator::F32Max | Operator::F32Copysign | Operator::F64Abs | Operator::F64Neg | Operator::F64Ceil
            | Operator::F64Floor | Operator::F64Trunc | Operator::F64Nearest | Operator::F64Sqrt | Operator::F64Add
            | Operator::F64Sub | Operator::F64Mul | Operator::F64Div | Operator::F64Min | Operator::F64Max
            | Operator::F64Copysign | Operator::I32TruncF32S | Operator::I32TruncF32U | Operator::I32TruncF64S | Operator::I32TruncF64U
            | Operator::I64TruncF32S | Operator::I64TruncF32U | Operator::I64TruncF64S | Operator::I64TruncF64U | Operator::F32ConvertI32S
            | Operator::F32ConvertI32U | Operator::F32ConvertI64S | Operator::F32ConvertI64U | Operator::F32DemoteF64 | Operator::F64ConvertI32S
            | Operator::F64ConvertI32U | Operator::F64ConvertI64S | Operator::F64ConvertI64U | Operator::F64PromoteF32 | Operator::I32ReinterpretF32
            | Operator::I64ReinterpretF64 | Operator::F32ReinterpretI32 | Operator::F64ReinterpretI64 | Operator::I32TruncSatF32S | Operator::I32TruncSatF32U
            | Operator::I32TruncSatF64S | Operator::I32TruncSatF64U | Operator::I64TruncSatF32S | Operator::I64TruncSatF32U | Operator::I64TruncSatF64S
            | Operator::I64TruncSatF64U | Operator::F32x4Splat | Operator::F32x4ExtractLane { .. } | Operator::F32x4ReplaceLane { .. } | Operator::F64x2Splat
            | Operator::F64x2ExtractLane { .. } | Operator::F64x2ReplaceLane { .. } | Operator::F32x4Ceil | Operator::F32x4Floor | Operator::F32x4Trunc
            | Operator::F32x4Nearest | Operator::F64x2Ceil | Operator::F64x2Floor | Operator::F64x2Trunc | Operator::F64x2Nearest
            | Operator::F32x4Abs | Operator::F32x4Neg | Operator::F32x4Sqrt | Operator::F32x4Add | Operator::F32x4Fma
            | Operator::F32x4Fms | Operator::F32x4Sub | Operator::F32x4Mul | Operator::F32x4Div | Operator::F32x4Min
            | Operator::F32x4RelaxedMin | Operator::F32x4Max | Operator::F32x4RelaxedMax | Operator::F32x4PMin | Operator::F32x4PMax
            | Operator::F32x4Eq | Operator::F32x4Ne | Operator::F32x4Lt | Operator::F32x4Gt | Operator::F32x4Le
            | Operator::F32x4Ge | Operator::F64x2Eq | Operator::F64x2Ne | Operator::F64x2Lt | Operator::F64x2Gt
            | Operator::F64x2Le | Operator::F64x2Ge | Operator::F64x2Abs | Operator::F64x2Neg | Operator::F64x2Sqrt
            | Operator::F64x2Add | Operator::F64x2Fma | Operator::F64x2Fms | Operator::F64x2Sub | Operator::F64x2Mul
            | Operator::F64x2Div | Operator::F64x2Min | Operator::F64x2RelaxedMin | Operator::F64x2Max | Operator::F64x2RelaxedMax
            | Operator::F64x2PMin | Operator::F64x2PMax | Operator::I32x4TruncSatF32x4S | Operator::I32x4TruncSatF32x4U | Operator::I32x4RelaxedTruncSatF32x4S
            | Operator::I32x4RelaxedTruncSatF32x4U | Operator::F32x4ConvertI32x4S | Operator::F32x4ConvertI32x4U
            => {
                if self.config.allow_floating_point_ops {
                    state.push_operator(operator);
                    Ok(())
                } else {
                    let msg = "OpcodeError: Floating Point Operations";
                    Err(MiddlewareError::new(" ", msg))
                }
            }
            // Opcode family of bulk memory operations
            Operator::MemoryInit { .. }
            | Operator::DataDrop { .. }
            | Operator::TableCopy { .. }
            | Operator::MemoryCopy { .. }
            | Operator::MemoryFill { .. }
            | Operator::TableInit { .. }
            | Operator::ElemDrop { .. }
            | Operator::TableFill { .. } => {
                if self.config.allow_bulk_memory_operations {
                    state.push_operator(operator);
                    Ok(())
                } else {
                    let msg = "OpcodeError: Bulk Memory Operations";
                    Err(MiddlewareError::new(" ", msg))
                }
            }
            // Opcode family for exception handling
            Operator::Try { .. }
            | Operator::Delegate { .. }
            | Operator::Catch { .. }
            | Operator::Throw { .. }
            | Operator::Rethrow { .. }
            | Operator::CatchAll => {
                if self.config.allow_exception_handling {
                    state.push_operator(operator);
                    Ok(())
                } else {
                    let msg = "OpcodeError: Exception Handling";
                    Err(MiddlewareError::new(" ", msg))
                }
            }
        }
    }
}

/// Covering integer and floating point unit tests
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wasmer::{CompilerConfig, Module, Store, Universal};
    use wasmer_compiler_singlepass::Singlepass;

    #[test]
    fn check_i64_add() {
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "sum") (param i64 i64) (result i64)
                    (local.get 0)
                    (local.get 1)
                    (i64.add)
                ))
            "#,
        )
        .unwrap();

        let deterministic = Arc::new(NonDeterminismFilter::default());
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(deterministic);
        let store = Store::new(&Universal::new(compiler_config).engine());
        let result = Module::new(&store, &wasm);
        assert!(result.is_ok());
    }

    #[test]
    fn check_floating_point_ops() {
        let wasm = wat::parse_str(
            r#"
            (module
                (func $to_float (param i64) (result f64)
                    local.get 0
                    f64.convert_i64_u
                ))
            "#,
        )
        .unwrap();

        // Default: not allow Floating Point
        let deterministic = Arc::new(NonDeterminismFilter::default());
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(deterministic);
        let store = Store::new(&Universal::new(compiler_config).engine());
        let result = Module::new(&store, &wasm);
        assert!(result.unwrap_err().to_string().contains("OpcodeError"));

        // Allow Floating Point
        let mut fitler = NonDeterminismFilter::default();
        fitler.config.allow_floating_point_ops = true;
        let deterministic = Arc::new(fitler);
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(deterministic);
        let store = Store::new(&Universal::new(compiler_config).engine());
        let result = Module::new(&store, &wasm);
        assert!(result.is_ok())
    }

    #[test]
    fn check_simd() {
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "simd")
                    i32.const 0
                    v128.const i32x4 1 2 3 4
                    v128.store
                )
                (memory $memory (export "memory") 1)
            )
            "#,
        )
        .unwrap();

        // Default: not allow SIMD
        let deterministic = Arc::new(NonDeterminismFilter::default());
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(deterministic);
        let store = Store::new(&Universal::new(compiler_config).engine());
        let result = Module::new(&store, &wasm);
        assert!(result.unwrap_err().to_string().contains("OpcodeError"));

        // Allow SIMD
        let mut fitler = NonDeterminismFilter::default();
        fitler.config.allow_simd_ops = true;
        let deterministic = Arc::new(fitler);
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(deterministic);
        let store = Store::new(&Universal::new(compiler_config).engine());
        let result = Module::new(&store, &wasm);
        assert!(!result.unwrap_err().to_string().contains("OpcodeError"));
    }
}
