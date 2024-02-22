/*
    Copyright © 2023, ParallelChain Lab
    Licensed under the Apache License, Version 2.0: http://www.apache.org/licenses/LICENSE-2.0
*/

//! Defines versioning of the ParallelChain Mainnet Contract Binary Interface (CBI).
//!
//! Each version codifies specifications that smart contracts need to follow.

/// current CBI version
pub const CBI_VERSION: u32 = CBIVER_ADAM;

/// CBI version defined in protocol v0.4 and v0.5.
const CBIVER_ADAM: u32 = 0;

/// check if the given CBI version is compatible with the current CBI version
#[allow(clippy::absurd_extreme_comparisons)]
pub(crate) const fn is_cbi_compatible(version: u32) -> bool {
    version <= CBI_VERSION
}

/// returns present CBI versin
#[inline]
pub const fn cbi_version() -> u32 {
    CBI_VERSION
}
