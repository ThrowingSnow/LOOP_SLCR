use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// `PartialEq` but not `Eq`: one variant carries an `f64`, and reporting the
/// offending ratio as a number is worth more than a total-equality bound nothing
/// asks for.
#[derive(Debug, Error, PartialEq, Clone)]
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

    #[error("invalid speed ratio {0}: expected a positive, finite number")]
    SpeedRatio(f64),

    #[error("cannot resample an empty buffer")]
    EmptyResampleInput,

    #[error("cannot resample to zero frames")]
    EmptyResampleOutput,

    #[error("a loop of zero length cannot be folded")]
    EmptyLoop,

    #[error("source holds {have} frames but the loop needs {need}: too short to fold")]
    SourceShorterThanLoop { have: usize, need: usize },

    #[error("no tempo known — pass --bpm (no acid chunk, none in the name)")]
    NoTempo,

    #[error("cannot tell how long the loop is — pass --bars")]
    NoLoopLength,

    #[error("a loop of {0} frames is longer than this machine can index")]
    LoopTooLong(u64),

    #[error("no sample-exact tempo within ±{window} BPM of {landing} for {bars} bars")]
    NoSampleExactTempo {
        landing: String,
        window: u32,
        bars: u64,
    },
}
