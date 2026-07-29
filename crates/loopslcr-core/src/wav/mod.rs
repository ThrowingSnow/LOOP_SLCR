//! WAVE reading and writing, hand-rolled over a shared RIFF chunk layer.

pub mod chunks;
pub mod read;
pub mod write;

#[cfg(test)]
pub(crate) mod test_support;

pub use chunks::{AcidChunk, Format, SampleFormat, SampleLoop, SmplChunk};
pub use read::{Tags, Wav};
pub use write::{write, BitDepth, Metadata, WriteSpec};
