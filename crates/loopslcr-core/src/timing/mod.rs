//! Exact bar-grid timing: tempo, meter, and where a cut lands.

pub mod grid;
pub mod signature;
pub mod tempo;

pub use grid::{Align, Grid, Region, Residual};
pub use signature::{BpmUnit, TimeSignature};
pub use tempo::Tempo;
