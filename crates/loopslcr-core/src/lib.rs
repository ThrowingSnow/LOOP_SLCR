//! LOOP_SLCR core — sample-exact loop trimming.
//!
//! Feed it a rendered loop with a warmup head and an FX tail; it yields a
//! seamless N-bar loop. The core knows nothing about any UI: no `clap`, no
//! JNI, no platform types.
//!
//! Timing is exact. Cut points come from `i128` rationals ([`rational`],
//! [`timing`]); floats appear only once the sample domain is reached.
//!
//! ```
//! use loopslcr_core::timing::{Align, Grid, TimeSignature, Tempo};
//!
//! // The reference case: 103 BPM, 4/4, 44.1 kHz — skip 8 warmup bars, keep 8.
//! let grid = Grid::new(Tempo::bpm(103)?, TimeSignature::FOUR_FOUR, 44_100);
//! let region = grid.region(8, 8, Align::Loop);
//! assert_eq!(region.start, 822_058);
//! assert_eq!(region.len(), 822_058);
//! # Ok::<(), loopslcr_core::Error>(())
//! ```

#![deny(clippy::float_arithmetic)]

pub mod buffer;
pub mod error;
pub mod rational;
pub mod timing;
pub mod wav;

pub use buffer::AudioBuffer;
pub use error::{Error, Result};
pub use rational::Rational;
pub use wav::Wav;
