//! Reading WAVE files into an [`AudioBuffer`].
//!
//! The core takes bytes, never a path: on Android the file arrives through SAF
//! as a stream, on the desktop the CLI reads it. Keeping `std::fs` out of here
//! is what lets the same code serve both.

// Decoding is the crossing point into the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;
use crate::error::{Error, Result};

use super::chunks::{
    parse_info_list, AcidChunk, ChunkId, Chunks, Format, SampleFormat, SmplChunk, ACID, DATA, FMT,
    LIST, SMPL,
};

/// Everything a source file declares about itself, without its samples.
#[derive(Clone, Debug, PartialEq)]
pub struct Tags {
    pub acid: Option<AcidChunk>,
    pub smpl: Option<SmplChunk>,
    pub info: Vec<(ChunkId, String)>,
}

impl Tags {
    /// The tempo the file declares, if any.
    ///
    /// Preferred over guessing from the filename — this is why reading unknown
    /// chunks was worth the effort.
    pub fn declared_tempo(&self) -> Option<f32> {
        self.acid
            .filter(|a| a.tempo > 0.0 && !a.is_one_shot())
            .map(|a| a.tempo)
    }
}

/// A parsed WAVE file: headers and tags resolved, samples still encoded.
///
/// Parsing is cheap and touches no sample data, so sweeping an archive for
/// `info` costs a header read per file rather than a full decode.
#[derive(Clone, Debug)]
pub struct Wav<'a> {
    format: Format,
    data: &'a [u8],
    tags: Tags,
}

impl<'a> Wav<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut format = None;
        let mut data = None;
        let mut acid = None;
        let mut smpl = None;
        let mut info = Vec::new();

        for chunk in Chunks::parse(bytes)? {
            match chunk.id {
                FMT if format.is_none() => format = Some(Format::parse(chunk.body)?),
                DATA if data.is_none() => data = Some(chunk.body),
                ACID if acid.is_none() => acid = AcidChunk::parse(chunk.body),
                SMPL if smpl.is_none() => smpl = SmplChunk::parse(chunk.body),
                LIST => info.extend(parse_info_list(chunk.body)),
                _ => {}
            }
        }

        let format = format.ok_or(Error::MissingChunk("fmt "))?;
        let data = data.ok_or(Error::MissingChunk("data"))?;

        Ok(Wav {
            format,
            data,
            tags: Tags { acid, smpl, info },
        })
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn tags(&self) -> &Tags {
        &self.tags
    }

    pub fn sample_rate(&self) -> u32 {
        self.format.sample_rate
    }

    pub fn channel_count(&self) -> usize {
        self.format.channels as usize
    }

    /// Whole frames present in the `data` chunk.
    ///
    /// A trailing partial frame is ignored rather than zero-filled: half a
    /// stereo frame is damage, and inventing the other half would hide it.
    pub fn frames(&self) -> usize {
        self.data.len() / self.format.frame_size()
    }

    /// True when the `data` chunk does not divide evenly into frames.
    pub fn has_partial_frame(&self) -> bool {
        self.data.len() % self.format.frame_size() != 0
    }

    /// Duration in seconds. Display only.
    pub fn duration_seconds(&self) -> f64 {
        self.frames() as f64 / self.format.sample_rate as f64
    }

    /// Decodes the samples into planar `f64`.
    pub fn decode(&self) -> Result<AudioBuffer> {
        self.decode_range(0, self.frames())
    }

    /// Decodes a half-open frame range, clamped to what the file holds.
    ///
    /// Lets a cut read only the region it keeps instead of the whole file —
    /// which for path A is half of it.
    pub fn decode_range(&self, start: usize, end: usize) -> Result<AudioBuffer> {
        let frame_size = self.format.frame_size();
        let bytes_per_sample = self.format.bytes_per_sample();
        let channels = self.channel_count();

        let start = start.min(self.frames());
        let end = end.clamp(start, self.frames());

        let mut out = vec![vec![0.0f64; end - start]; channels];
        for (frame_index, frame) in self.data[start * frame_size..end * frame_size]
            .chunks_exact(frame_size)
            .enumerate()
        {
            for (channel, sample) in frame.chunks_exact(bytes_per_sample).enumerate() {
                out[channel][frame_index] = decode_sample(sample, self.format)?;
            }
        }

        Ok(AudioBuffer::new(out, self.format.sample_rate))
    }
}

/// Converts one encoded sample to `f64` in roughly `[-1, 1)`.
///
/// Integers are divided by `2^(bits-1)`, so the most negative value maps to
/// exactly −1.0 and the most positive falls just short of +1.0. That is the
/// asymmetry two's complement actually has; the alternative — scaling by
/// `2^(bits-1) - 1` — reaches +1.0 but makes the round trip lossy.
fn decode_sample(bytes: &[u8], format: Format) -> Result<f64> {
    Ok(match (format.format, format.bits_per_sample) {
        (SampleFormat::Int, 16) => {
            i16::from_le_bytes([bytes[0], bytes[1]]) as f64 / 32_768.0
        }
        (SampleFormat::Int, 24) => {
            // Three bytes little-endian, sign-extended into i32 by placing
            // them in the top bits and shifting back down arithmetically.
            let raw = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8;
            raw as f64 / 8_388_608.0
        }
        (SampleFormat::Int, 32) => {
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64 / 2_147_483_648.0
        }
        (SampleFormat::Float, 32) => {
            f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
        }
        (SampleFormat::Float, 64) => f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        (_, bits) => return Err(Error::UnsupportedBitDepth(bits)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::test_support::{riff, WavSpec};

    #[test]
    fn reads_16_bit_stereo() {
        let samples: Vec<i32> = vec![0, 32_767, -32_768, 1234, -1234, 0];
        let bytes = riff(WavSpec::int(2, 44_100, 16), &samples);
        let wav = Wav::parse(&bytes).unwrap();

        assert_eq!(wav.sample_rate(), 44_100);
        assert_eq!(wav.channel_count(), 2);
        assert_eq!(wav.frames(), 3);
        assert!(!wav.has_partial_frame());

        let buf = wav.decode().unwrap();
        assert_eq!(buf.channel(0), &[0.0, -1.0, -1234.0 / 32_768.0]);
        assert_eq!(buf.channel(1), &[32_767.0 / 32_768.0, 1234.0 / 32_768.0, 0.0]);
    }

    #[test]
    fn full_scale_negative_is_exactly_minus_one() {
        // The reason for dividing by 2^(bits-1): the round trip stays exact.
        for (bits, min) in [(16u16, -32_768i32), (24, -8_388_608), (32, i32::MIN)] {
            let bytes = riff(WavSpec::int(1, 44_100, bits), &[min]);
            let buf = Wav::parse(&bytes).unwrap().decode().unwrap();
            assert_eq!(buf.channel(0)[0], -1.0, "{bits}-bit minimum");
        }
    }

    #[test]
    fn reads_24_bit_including_sign_extension() {
        let samples = vec![0, 1, -1, 8_388_607, -8_388_608, 12_345];
        let bytes = riff(WavSpec::int(1, 48_000, 24), &samples);
        let buf = Wav::parse(&bytes).unwrap().decode().unwrap();
        let expected: Vec<f64> = samples.iter().map(|&s| s as f64 / 8_388_608.0).collect();
        assert_eq!(buf.channel(0), expected.as_slice());
        // Negative values must not come out as huge positives.
        assert!(buf.channel(0)[2] < 0.0);
        assert!(buf.channel(0)[4] == -1.0);
    }

    #[test]
    fn reads_32_bit_float() {
        let samples = vec![0.0f32, 1.0, -1.0, 0.5, -0.25, 1.5];
        let bytes = riff(WavSpec::float(1, 44_100, 32), &samples);
        let buf = Wav::parse(&bytes).unwrap().decode().unwrap();
        let expected: Vec<f64> = samples.iter().map(|&s| s as f64).collect();
        assert_eq!(buf.channel(0), expected.as_slice());
        // Float files may legitimately exceed unity; that is not our business
        // to clamp on read.
        assert_eq!(buf.peak(), 1.5);
    }

    #[test]
    fn decode_range_is_clamped_and_half_open() {
        let samples: Vec<i32> = (0..10).collect();
        let bytes = riff(WavSpec::int(1, 44_100, 16), &samples);
        let wav = Wav::parse(&bytes).unwrap();

        assert_eq!(wav.decode_range(2, 5).unwrap().frames(), 3);
        assert_eq!(wav.decode_range(0, 100).unwrap().frames(), 10);
        assert_eq!(wav.decode_range(20, 30).unwrap().frames(), 0);
        assert_eq!(wav.decode_range(5, 2).unwrap().frames(), 0);

        // A range decodes to the same values as the full decode.
        let full = wav.decode().unwrap();
        let part = wav.decode_range(3, 7).unwrap();
        assert_eq!(part.channel(0), &full.channel(0)[3..7]);
    }

    #[test]
    fn a_trailing_partial_frame_is_reported_not_invented() {
        // Stereo 16-bit needs 4 bytes per frame; give it 10.
        let mut bytes = riff(WavSpec::int(2, 44_100, 16), &[1i32, 2, 3, 4]);
        bytes.extend_from_slice(&[0, 0]);
        let len = bytes.len();
        bytes[4..8].copy_from_slice(&((len - 8) as u32).to_le_bytes());
        // Patch the data chunk size to include the stray two bytes.
        let data_size_at = len - 2 - 8 - 4;
        let old = u32::from_le_bytes([
            bytes[data_size_at],
            bytes[data_size_at + 1],
            bytes[data_size_at + 2],
            bytes[data_size_at + 3],
        ]);
        bytes[data_size_at..data_size_at + 4].copy_from_slice(&(old + 2).to_le_bytes());

        let wav = Wav::parse(&bytes).unwrap();
        assert!(wav.has_partial_frame());
        assert_eq!(wav.frames(), 2); // the complete ones only
        assert_eq!(wav.decode().unwrap().frames(), 2);
    }

    #[test]
    fn missing_chunks_are_named_in_the_error() {
        // fmt but no data.
        let bytes = riff(WavSpec::int(1, 44_100, 16), &[] as &[i32]);
        let mut no_data = Vec::from(&bytes[..12]);
        // Rebuild with only the fmt chunk.
        let chunks: Vec<_> = Chunks::parse(&bytes).unwrap().collect();
        for c in chunks.iter().filter(|c| c.id == FMT) {
            no_data.extend_from_slice(&c.id);
            no_data.extend_from_slice(&(c.body.len() as u32).to_le_bytes());
            no_data.extend_from_slice(c.body);
        }
        let size = (no_data.len() - 8) as u32;
        no_data[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(Wav::parse(&no_data).err(), Some(Error::MissingChunk("data")));
    }

    #[test]
    fn reads_tempo_from_the_acid_chunk() {
        let mut acid = vec![0u8; 24];
        acid[12..16].copy_from_slice(&32u32.to_le_bytes());
        acid[16..18].copy_from_slice(&4u16.to_le_bytes());
        acid[18..20].copy_from_slice(&4u16.to_le_bytes());
        acid[20..24].copy_from_slice(&103.0f32.to_le_bytes());

        let bytes = riff(
            WavSpec::int(1, 44_100, 16).with_chunk(ACID, acid),
            &[0i32, 1, 2, 3],
        );
        let wav = Wav::parse(&bytes).unwrap();
        assert_eq!(wav.tags().declared_tempo(), Some(103.0));
        assert_eq!(wav.tags().acid.unwrap().beats, 32);
    }

    #[test]
    fn a_one_shot_declares_no_tempo() {
        let mut acid = vec![0u8; 24];
        acid[0..4].copy_from_slice(&AcidChunk::FLAG_ONE_SHOT.to_le_bytes());
        acid[20..24].copy_from_slice(&120.0f32.to_le_bytes());
        let bytes = riff(WavSpec::int(1, 44_100, 16).with_chunk(ACID, acid), &[0i32]);
        // The tempo field is there but meaningless for a one-shot.
        let wav = Wav::parse(&bytes).unwrap();
        assert!(wav.tags().acid.is_some());
        assert_eq!(wav.tags().declared_tempo(), None);
    }

    #[test]
    fn unknown_chunks_are_skipped_not_fatal() {
        let bytes = riff(
            WavSpec::int(1, 44_100, 16)
                .with_chunk(*b"junk", vec![0; 7]) // odd size, exercises padding
                .with_chunk(*b"fact", vec![0; 4]),
            &[1i32, 2, 3],
        );
        let wav = Wav::parse(&bytes).unwrap();
        assert_eq!(wav.frames(), 3);
    }
}
