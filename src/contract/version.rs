/*
    Copyright © 2023, ParallelChain Lab 
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines versioning of Contract Binary Interface.

/// Contract Binary Interface is a specification that a smart contract should follow, 
/// in terms of implementing strictly same function signature of the host functions.
pub const CBI_VERSION: u32 = CBIVER_ADAM;

/// CBI versions defined in Protocol Adam.
const CBIVER_ADAM: u32 = 0;

/// Check if version is compatible to runtime. Current CBI version = [CBI_VERSION].
#[allow(clippy::absurd_extreme_comparisons)]
pub(crate) const fn is_cbi_compatible(version: u32) -> bool {
    version <= CBI_VERSION
}