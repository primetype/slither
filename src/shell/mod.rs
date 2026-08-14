//! The I/O shell — `SPEC.md` §16.
//!
//! The cores are sans-io state machines that never read a clock; the shell
//! is the single `!Send` tokio actor that gives them a socket and a
//! timer, and the handles the application holds. Slice 0 lands only the
//! substrate's outermost seam, [`wire::Wire`]; the driver, the endpoint,
//! the staged-accept handles and the connection handles arrive with the
//! slices that need them.

pub mod wire;
