/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines versioning of the ParallelChain Mainnet Contract Binary Interface (CBI).
//! Each version references specification that smart contracts need to follow.
pub const CBI_VERSION: u32 = CBIVER_ADAM;

/// CBI version defined in protocol v0.4 and v0.5.
const CBIVER_ADAM: u32 = 0;

/// Check if version is compatible to runtime. Current CBI version = [CBI_VERSION].
#[allow(clippy::absurd_extreme_comparisons)]
pub(crate) const fn is_cbi_compatible(version: u32) -> bool {
    version <= CBI_VERSION
}
