//! Operations on an [`AudioBuffer`](crate::AudioBuffer), in pipeline order.
//!
//! The order is binding, and the reason is timing: [`cut`] and [`foldback`]
//! work in the original time domain, where the bar grid is exact. Anything that
//! resamples has to come after them, so that the output length is rounded once
//! rather than compounding with the cut.
//!
//! Each operation reports what it did rather than deciding what it means. A
//! short cut, a foldback past full scale — those are the caller's to act on,
//! because the right answer differs between a batch run and a preview.

pub mod cut;
pub mod dither;
pub mod fade;
pub mod foldback;
pub mod gain;
pub mod resample;

pub use cut::{cut, Cut};
pub use dither::Dither;
pub use fade::{Fade, FadeShape};
pub use foldback::{foldback, Foldback};
pub use gain::Peak;
pub use resample::{resample, Resampler};
