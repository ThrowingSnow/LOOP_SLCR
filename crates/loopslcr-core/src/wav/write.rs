//! Writing an [`AudioBuffer`] out as a WAVE file.
//!
//! Bytes in, bytes out — the core never touches a path, so the same code serves
//! the CLI and an Android `OutputStream`.
//!
//! This is the reason the chunk layer is hand-rolled: `acid` and `smpl` are
//! what make an exported loop drop into a sampler at the right tempo and loop
//! seamlessly, and no reader crate writes them.

// Quantisation crosses back out of the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;
use crate::error::{Error, Result};

use super::chunks::{
    build_info_list, AcidChunk, ChunkId, SmplChunk, ACID, DATA, FMT, FORMAT_IEEE_FLOAT, FORMAT_PCM,
    ICMT, LIST, RIFF, SMPL, WAVE,
};

/// The `fact` chunk, required for non-PCM formats.
const FACT: ChunkId = *b"fact";

/// Output sample encoding.
///
/// A deliberately short list: these are the depths a loop is worth exporting
/// at. 8-bit is not offered, and 64-bit float would only make files twice as
/// large than any sampler can use.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BitDepth {
    Int16,
    /// The archive's own format, and the default: transparent, and half the
    /// size of 32-bit float.
    #[default]
    Int24,
    Int32,
    /// Lossless for anything the pipeline produced, including samples past
    /// unity — nothing is clipped and nothing needs dither.
    Float32,
}

impl BitDepth {
    pub fn bits(self) -> u16 {
        match self {
            BitDepth::Int16 => 16,
            BitDepth::Int24 => 24,
            BitDepth::Int32 | BitDepth::Float32 => 32,
        }
    }

    pub fn bytes(self) -> usize {
        self.bits() as usize / 8
    }

    pub fn is_float(self) -> bool {
        self == BitDepth::Float32
    }

    /// Whether quantising to this depth can clip. Float cannot.
    pub fn clips(self) -> bool {
        !self.is_float()
    }
}

impl std::str::FromStr for BitDepth {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "16" => Ok(BitDepth::Int16),
            "24" => Ok(BitDepth::Int24),
            "32" => Ok(BitDepth::Int32),
            "32f" | "f32" | "float" => Ok(BitDepth::Float32),
            other => Err(Error::BitDepthName(other.to_string())),
        }
    }
}

/// What the output file declares about itself, beyond `fmt `.
///
/// All optional: a file with none of it is still a valid WAVE, just a mute one
/// about its own tempo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub acid: Option<AcidChunk>,
    pub smpl: Option<SmplChunk>,
    /// Free text, written as `LIST`/`INFO`/`ICMT`. Human-readable fallback for
    /// hosts that ignore `acid`.
    pub comment: Option<String>,
    /// Anything else, written verbatim after the standard chunks.
    pub extra: Vec<(ChunkId, Vec<u8>)>,
}

impl Metadata {
    /// The tags a finished loop should carry: tempo, beat count, loop points.
    pub fn for_loop(tempo: f32, beats: u32, numerator: u16, denominator: u16, frames: u32) -> Self {
        Metadata {
            acid: Some(AcidChunk::for_loop(tempo, beats, numerator, denominator)),
            smpl: Some(SmplChunk::whole_file(frames)),
            comment: None,
            extra: Vec::new(),
        }
    }

    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
}

/// How to encode the output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriteSpec {
    pub depth: BitDepth,
    pub metadata: Metadata,
}

impl WriteSpec {
    pub fn new(depth: BitDepth) -> Self {
        WriteSpec {
            depth,
            metadata: Metadata::default(),
        }
    }

    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Encodes `buffer` as a complete WAVE file.
///
/// Chunk order is `fmt `, `data`, then the tags. Tags last on purpose: readers
/// that walk chunks find them wherever they sit, while the ones that assume
/// audio starts at byte 44 — hardware samplers, phone players — only work if
/// nothing is inserted ahead of `data`. Being readable everywhere is worth more
/// than matching what any particular DAW happens to emit.
pub fn write(buffer: &AudioBuffer, spec: &WriteSpec) -> Result<Vec<u8>> {
    let channels = u16::try_from(buffer.channel_count())
        .map_err(|_| Error::TooManyChannels(buffer.channel_count()))?;
    let depth = spec.depth;
    let sample_rate = buffer.sample_rate();
    let frames = buffer.frames();

    let block_align = depth.bytes() * channels as usize;
    let data_len = frames
        .checked_mul(block_align)
        .ok_or(Error::FileTooLarge(u64::MAX))?;

    let mut body = Vec::from(WAVE);
    push_chunk(&mut body, FMT, &fmt_body(depth, channels, sample_rate));
    if depth.is_float() {
        // Non-PCM formats declare their length in frames separately.
        push_chunk(&mut body, FACT, &(frames as u32).to_le_bytes());
    }

    // Interleave and quantise straight into the chunk body: one pass, one
    // allocation, no intermediate interleaved copy of the whole file.
    body.extend_from_slice(&DATA);
    body.extend_from_slice(&(data_len as u32).to_le_bytes());
    body.reserve(data_len);
    for frame in 0..frames {
        for channel in buffer.channels() {
            encode_sample(&mut body, channel[frame], depth);
        }
    }
    if data_len % 2 == 1 {
        body.push(0);
    }

    let m = &spec.metadata;
    if let Some(acid) = m.acid {
        push_chunk(&mut body, ACID, &acid.to_body());
    }
    if let Some(smpl) = &m.smpl {
        push_chunk(&mut body, SMPL, &smpl.to_body(sample_rate));
    }
    if let Some(comment) = &m.comment {
        let list = build_info_list(&[(ICMT, comment.clone())]);
        push_chunk(&mut body, LIST, &list);
    }
    for (id, chunk) in &m.extra {
        push_chunk(&mut body, *id, chunk);
    }

    // The RIFF size field, written honestly — the bug that cost audio on read
    // came from an encoder getting this wrong by 44 bytes.
    let total = body.len() as u64 + 8;
    if total > u32::MAX as u64 {
        return Err(Error::FileTooLarge(total));
    }
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(&RIFF);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn fmt_body(depth: BitDepth, channels: u16, sample_rate: u32) -> Vec<u8> {
    let tag = if depth.is_float() {
        FORMAT_IEEE_FLOAT
    } else {
        FORMAT_PCM
    };
    let block_align = depth.bytes() as u16 * channels;

    let mut v = Vec::with_capacity(18);
    v.extend_from_slice(&tag.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&depth.bits().to_le_bytes());
    if depth.is_float() {
        // Non-PCM `fmt ` is at least 18 bytes; no extension data follows.
        v.extend_from_slice(&0u16.to_le_bytes());
    }
    v
}

/// Quantises one sample and appends it.
///
/// Integers scale by `2^(bits-1)`, mirroring the reader exactly, so a decode
/// followed by an encode at the source depth returns the original bytes. That
/// scale makes `+1.0` one step past the largest representable value, hence the
/// clamp — the alternative, scaling by `2^(bits-1) - 1`, would avoid the clamp
/// but make every round trip lossy.
///
/// Rounding is half-away-from-zero. Half-cases are where dither belongs, and
/// dither comes later in the chain; until then this is at least symmetric
/// around zero, so no DC offset is introduced.
fn encode_sample(out: &mut Vec<u8>, sample: f64, depth: BitDepth) {
    if depth.is_float() {
        out.extend_from_slice(&(sample as f32).to_le_bytes());
        return;
    }

    let bits = depth.bits();
    let max = (1i64 << (bits - 1)) - 1;
    let min = -(1i64 << (bits - 1));
    // A float-to-int cast saturates in Rust and maps NaN to zero, so an
    // infinity or a NaN out of a broken filter cannot wrap to full scale.
    let raw = (sample * (1i64 << (bits - 1)) as f64).round() as i64;
    let raw = raw.clamp(min, max) as i32;

    match depth {
        BitDepth::Int16 => out.extend_from_slice(&(raw as i16).to_le_bytes()),
        BitDepth::Int24 => out.extend_from_slice(&raw.to_le_bytes()[..3]),
        BitDepth::Int32 => out.extend_from_slice(&raw.to_le_bytes()),
        BitDepth::Float32 => unreachable!("handled above"),
    }
}

fn push_chunk(out: &mut Vec<u8>, id: ChunkId, body: &[u8]) {
    out.extend_from_slice(&id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0); // chunks are word-aligned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::chunks::{Chunks, SampleFormat, SampleLoop};
    use crate::wav::Wav;

    fn buffer(channels: usize, frames: usize) -> AudioBuffer {
        let data = (0..channels)
            .map(|c| {
                (0..frames)
                    .map(|f| {
                        // A ramp per channel, distinct between channels so a
                        // swapped interleave cannot pass.
                        let t = f as f64 / frames as f64;
                        if c == 0 {
                            t * 2.0 - 1.0
                        } else {
                            1.0 - t * 2.0
                        }
                    })
                    .collect()
            })
            .collect();
        AudioBuffer::new(data, 44_100)
    }

    #[test]
    fn every_depth_round_trips_through_our_own_reader() {
        for depth in [
            BitDepth::Int16,
            BitDepth::Int24,
            BitDepth::Int32,
            BitDepth::Float32,
        ] {
            for channels in [1usize, 2] {
                let source = buffer(channels, 64);
                let bytes = write(&source, &WriteSpec::new(depth)).unwrap();
                let wav = Wav::parse(&bytes).unwrap();

                assert_eq!(wav.channel_count(), channels, "{depth:?}");
                assert_eq!(wav.frames(), 64, "{depth:?}");
                assert_eq!(wav.sample_rate(), 44_100);
                assert!(!wav.has_partial_frame(), "{depth:?}");
                assert_eq!(wav.format().bits_per_sample, depth.bits());
                assert!(wav.format().block_align_is_consistent());

                // Quantisation error must stay inside one step of the target
                // depth — anything larger means a scaling or interleave bug,
                // not rounding.
                let step = if depth.is_float() {
                    2f64.powi(-24)
                } else {
                    1.0 / (1i64 << (depth.bits() - 1)) as f64
                };
                let out = wav.decode().unwrap();
                for c in 0..channels {
                    for f in 0..64 {
                        let diff = (out.channel(c)[f] - source.channel(c)[f]).abs();
                        assert!(diff <= step, "{depth:?} ch{c} frame{f}: off by {diff}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_decode_encode_round_trip_at_the_source_depth_is_lossless() {
        // The point of scaling by 2^(bits-1) on both sides: cutting a file and
        // writing it back at its own depth must not change a single byte.
        for depth in [BitDepth::Int16, BitDepth::Int24, BitDepth::Int32] {
            let source = buffer(2, 200);
            let once = write(&source, &WriteSpec::new(depth)).unwrap();
            let decoded = Wav::parse(&once).unwrap().decode().unwrap();
            let twice = write(&decoded, &WriteSpec::new(depth)).unwrap();
            assert_eq!(once, twice, "{depth:?} changed on the second pass");
        }
    }

    #[test]
    fn full_scale_survives_and_overshoot_clamps() {
        // -1.0 is exactly representable; +1.0 is one step past the top and has
        // to clamp rather than wrap to the most negative value.
        let buf = AudioBuffer::new(vec![vec![-1.0, 1.0, 2.0, -3.0, 0.0]], 44_100);
        for depth in [BitDepth::Int16, BitDepth::Int24, BitDepth::Int32] {
            let bytes = write(&buf, &WriteSpec::new(depth)).unwrap();
            let out = Wav::parse(&bytes).unwrap().decode().unwrap();
            let step = 1.0 / (1i64 << (depth.bits() - 1)) as f64;
            assert_eq!(out.channel(0)[0], -1.0, "{depth:?}");
            assert_eq!(out.channel(0)[1], 1.0 - step, "{depth:?}");
            assert_eq!(out.channel(0)[2], 1.0 - step, "{depth:?} clamp");
            assert_eq!(out.channel(0)[3], -1.0, "{depth:?} clamp");
            assert_eq!(out.channel(0)[4], 0.0);
        }
    }

    #[test]
    fn float_output_keeps_samples_past_unity() {
        // Nothing to clip against, so a hot mix survives intact.
        let buf = AudioBuffer::new(vec![vec![1.5, -2.25, 0.0]], 48_000);
        let bytes = write(&buf, &WriteSpec::new(BitDepth::Float32)).unwrap();
        let wav = Wav::parse(&bytes).unwrap();
        assert_eq!(wav.format().format, SampleFormat::Float);
        assert_eq!(wav.decode().unwrap().channel(0), &[1.5, -2.25, 0.0]);
    }

    #[test]
    fn nan_and_infinity_cannot_wrap_to_full_scale() {
        let buf = AudioBuffer::new(vec![vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY]], 44_100);
        let out = Wav::parse(&write(&buf, &WriteSpec::new(BitDepth::Int16)).unwrap())
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(out.channel(0)[0], 0.0);
        assert_eq!(out.channel(0)[1], 1.0 - 1.0 / 32_768.0);
        assert_eq!(out.channel(0)[2], -1.0);
    }

    #[test]
    fn the_riff_size_field_is_honest() {
        for depth in [BitDepth::Int24, BitDepth::Float32] {
            // An odd frame count at 24-bit mono gives an odd data size, which
            // exercises the pad byte.
            let buf = buffer(1, 101);
            let bytes = write(&buf, &WriteSpec::new(depth)).unwrap();
            assert_eq!(Chunks::size_field_error(&bytes), Some(0), "{depth:?}");
            assert_eq!(bytes.len() % 2, 0, "{depth:?}: file is not word-aligned");
        }
    }

    #[test]
    fn tags_round_trip() {
        let buf = buffer(2, 32);
        let spec = WriteSpec::new(BitDepth::Int24)
            .with_metadata(Metadata::for_loop(103.0, 32, 4, 4, 32).with_comment("103 BPM, 8 bars"));
        let bytes = write(&buf, &spec).unwrap();

        let wav = Wav::parse(&bytes).unwrap();
        assert_eq!(wav.frames(), 32, "tags after data cost audio");
        assert_eq!(wav.tags().declared_tempo(), Some(103.0));

        let acid = wav.tags().acid.unwrap();
        assert_eq!(acid.beats, 32);
        assert_eq!((acid.meter_numerator, acid.meter_denominator), (4, 4));
        assert!(!acid.is_one_shot());

        let smpl = wav.tags().smpl.clone().unwrap();
        assert_eq!(smpl.loops, vec![SampleLoop { start: 0, end: 31, play_count: 0 }]);
        // Written per the spec, so the inclusive read is the right one.
        assert_eq!(smpl.loops[0].frame_count(32), 32);
        assert_eq!(smpl.midi_unity_note, 60);

        assert_eq!(wav.tags().info, vec![(ICMT, "103 BPM, 8 bars".to_string())]);
    }

    #[test]
    fn data_comes_before_the_tags() {
        // A reader that stops at `data` must still get all the audio, and one
        // that assumes audio starts at byte 44 must find it there.
        let bytes = write(
            &buffer(2, 16),
            &WriteSpec::new(BitDepth::Int16).with_metadata(Metadata::for_loop(90.0, 16, 4, 4, 16)),
        )
        .unwrap();
        let ids: Vec<_> = Chunks::parse(&bytes).unwrap().map(|c| c.id).collect();
        assert_eq!(ids, vec![FMT, DATA, ACID, SMPL]);
        assert_eq!(&bytes[36..40], &DATA);
    }

    #[test]
    fn an_odd_length_comment_stays_aligned() {
        // An odd-length INFO string forces a pad byte mid-file; a chunk after
        // it will only parse if the padding was handled.
        let spec = WriteSpec::new(BitDepth::Int16).with_metadata(Metadata {
            comment: Some("odd".to_string()),
            extra: vec![(*b"junk", vec![1, 2, 3, 4])],
            ..Metadata::default()
        });
        let bytes = write(&buffer(1, 8), &spec).unwrap();
        let wav = Wav::parse(&bytes).unwrap();
        assert_eq!(wav.tags().info, vec![(ICMT, "odd".to_string())]);
        let ids: Vec<_> = Chunks::parse(&bytes).unwrap().map(|c| c.id).collect();
        assert_eq!(ids, vec![FMT, DATA, LIST, *b"junk"]);
    }

    #[test]
    fn an_empty_buffer_is_still_a_valid_file() {
        let buf = AudioBuffer::silence(2, 0, 44_100);
        let bytes = write(&buf, &WriteSpec::new(BitDepth::Int24)).unwrap();
        let wav = Wav::parse(&bytes).unwrap();
        assert_eq!(wav.frames(), 0);
        assert!(!wav.has_partial_frame());
    }

    #[test]
    fn bit_depth_parses_from_the_command_line() {
        use std::str::FromStr;
        assert_eq!(BitDepth::from_str("16").unwrap(), BitDepth::Int16);
        assert_eq!(BitDepth::from_str("24").unwrap(), BitDepth::Int24);
        assert_eq!(BitDepth::from_str("32").unwrap(), BitDepth::Int32);
        assert_eq!(BitDepth::from_str("32f").unwrap(), BitDepth::Float32);
        assert_eq!(BitDepth::from_str(" Float ").unwrap(), BitDepth::Float32);
        assert!(BitDepth::from_str("8").is_err());
        assert_eq!(BitDepth::default(), BitDepth::Int24);
    }
}
