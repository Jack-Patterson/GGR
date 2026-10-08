//! The engine-free leaf of Guildmaster's Seat.
//!
//! Everything here is pure and deterministic: our own RNG (never `rand`'s, whose algorithms
//! may change between versions), FNV-1a hashing, and the one error type every fallible
//! operation in the game returns.

pub mod error;
pub mod hash;
pub mod rng;

pub use error::GameError;
pub use hash::{fnv1a64, StateHasher};
pub use rng::{RngStreams, SplitMix64, StreamName, Xoshiro256StarStar};

/// The stamp every boot and every save carries.
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");
