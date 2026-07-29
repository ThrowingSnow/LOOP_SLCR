use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum Error {
    #[error("invalid time signature {0:?}: expected N/D with N > 0 and D > 0, e.g. 7/8")]
    TimeSignature(String),

    #[error("invalid BPM unit {0:?}: expected a/b with a > 0 and b > 0, e.g. 1/4 or 3/8")]
    BpmUnit(String),

    #[error("invalid tempo {0:?}: expected a positive number, e.g. 103 or 103.5")]
    Tempo(String),

    #[error("not a RIFF file")]
    NotRiff,

    #[error("RIFF file is not WAVE")]
    NotWave,

    #[error("malformed {0:?} chunk")]
    MalformedChunk(&'static str),

    #[error("no {0:?} chunk")]
    MissingChunk(&'static str),

    #[error("unsupported WAVE format tag {0:#06x}: only PCM and IEEE float are read")]
    UnsupportedFormat(u16),

    #[error("unsupported bit depth {0}: expected 16, 24 or 32-bit int, or 32/64-bit float")]
    UnsupportedBitDepth(u16),

    #[error("invalid bit depth {0:?}: expected 16, 24, 32 or 32f")]
    BitDepthName(String),

    #[error("{0} channels is more than a WAVE header can declare")]
    TooManyChannels(usize),

    #[error("output would be {0} bytes: past the 4 GiB a RIFF file can address")]
    FileTooLarge(u64),

    #[error("a loop of zero length cannot be folded")]
    EmptyLoop,

    #[error("source holds {have} frames but the loop needs {need}: too short to fold")]
    SourceShorterThanLoop { have: usize, need: usize },
}
