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
}
