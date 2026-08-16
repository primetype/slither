//! Story-level acceptance tests for ruling 208's path validation.
//!
//! Paused-clock, two endpoints over `testutil::FlakyWire`. Written blind to
//! the implementers from `CONTRACT-7b.md`.
//!
//! Skeleton:
//!
//! 1. Migration over a fresh address completes via challenge/response
//! 2. An off-path forger cannot lift the amplification limit
//! 3. Contested probe (ruling 176) still resolves
