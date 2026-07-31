//! Exact bar-grid timing: tempo, meter, and where a cut lands.

pub mod grid;
pub mod note;
pub mod signature;
pub mod speed;
pub mod tempo;

pub use grid::{Align, Grid, Region, Residual};
pub use note::{Flavour, NoteLength, NoteValue};
pub use signature::{BpmUnit, TimeSignature};
pub use speed::Ratio;
pub use tempo::Tempo;
