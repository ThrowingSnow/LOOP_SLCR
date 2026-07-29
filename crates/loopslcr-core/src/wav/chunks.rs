//! RIFF chunk walking and the chunks LOOP_SLCR cares about.
//!
//! Hand-rolled rather than `hound`, for one reason above all: this tool has to
//! *write* `acid` and `smpl` chunks, which `hound` cannot do. Since the chunk
//! layer has to exist for the writer anyway, reading through the same layer
//! keeps the round trip symmetric — and lets `info` report the tempo a file
//! already declares instead of guessing from its name.

use crate::error::{Error, Result};

/// A four-character RIFF identifier.
pub type ChunkId = [u8; 4];

pub const RIFF: ChunkId = *b"RIFF";
pub const WAVE: ChunkId = *b"WAVE";
pub const FMT: ChunkId = *b"fmt ";
pub const DATA: ChunkId = *b"data";
pub const ACID: ChunkId = *b"acid";
pub const SMPL: ChunkId = *b"smpl";
pub const LIST: ChunkId = *b"LIST";
pub const INFO: ChunkId = *b"INFO";
pub const ICMT: ChunkId = *b"ICMT";

/// One chunk: its id and its body, padding byte already excluded.
#[derive(Copy, Clone, Debug)]
pub struct Chunk<'a> {
    pub id: ChunkId,
    pub body: &'a [u8],
}

/// Walks the chunks of a RIFF/WAVE file.
///
/// Stops at the first malformed header rather than erroring, so a file with a
/// truncated trailing chunk still yields everything before it — which is the
/// difference between "unreadable" and "readable, minus a tag".
pub struct Chunks<'a> {
    rest: &'a [u8],
}

impl<'a> Chunks<'a> {
    /// Validates the `RIFF….WAVE` envelope and returns a walker over its body.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < 12 {
            return Err(Error::NotRiff);
        }
        if read_id(bytes, 0) != RIFF {
            return Err(Error::NotRiff);
        }
        if read_id(bytes, 8) != WAVE {
            return Err(Error::NotWave);
        }

        // The RIFF size field is advisory only. Encoders get it wrong in both
        // directions: truncated downloads overstate it, and Caustic exports
        // understate it by the 44 bytes of a classic WAVE header, which would
        // cost real audio at the end of every file if it were believed. The
        // actual file length is the authority; individual chunks are clamped
        // to what remains as they are walked.
        Ok(Chunks { rest: &bytes[12..] })
    }

    /// How far the RIFF size field is from the real file length, in bytes.
    /// Positive means the header claims more than the file holds.
    ///
    /// Diagnostic only — nothing depends on it. Worth surfacing in `info`
    /// because a mismatch says something about which tool wrote the file.
    pub fn size_field_error(bytes: &[u8]) -> Option<i64> {
        (bytes.len() >= 8 && read_id(bytes, 0) == RIFF)
            .then(|| read_u32(bytes, 4) as i64 + 8 - bytes.len() as i64)
    }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Chunk<'a>;

    fn next(&mut self) -> Option<Chunk<'a>> {
        if self.rest.len() < 8 {
            return None;
        }
        let id = read_id(self.rest, 0);
        let size = read_u32(self.rest, 4) as usize;

        // A chunk claiming more than remains: hand back what is actually
        // there. For `data` that is a short file, not a broken one.
        let available = self.rest.len() - 8;
        let size = size.min(available);
        let body = &self.rest[8..8 + size];

        // Chunks are word-aligned: an odd size is followed by a pad byte that
        // is not part of the body.
        let advance = 8 + size + (size & 1);
        self.rest = self.rest.get(advance..).unwrap_or(&[]);

        Some(Chunk { id, body })
    }
}

/// Sample encoding as declared by the `fmt ` chunk.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// Signed integer PCM, little-endian.
    Int,
    /// IEEE 754 floating point.
    Float,
}

pub const FORMAT_PCM: u16 = 0x0001;
pub const FORMAT_IEEE_FLOAT: u16 = 0x0003;
pub const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// The contents of the `fmt ` chunk, after resolving `WAVE_FORMAT_EXTENSIBLE`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Format {
    pub format: SampleFormat,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    /// Bytes per frame across all channels.
    pub block_align: u16,
}

impl Format {
    pub fn parse(body: &[u8]) -> Result<Self> {
        if body.len() < 16 {
            return Err(Error::MalformedChunk("fmt "));
        }
        let mut tag = read_u16(body, 0);
        let channels = read_u16(body, 2);
        let sample_rate = read_u32(body, 4);
        let block_align = read_u16(body, 12);
        let bits_per_sample = read_u16(body, 14);

        // WAVE_FORMAT_EXTENSIBLE keeps the real format tag in the first two
        // bytes of the SubFormat GUID, 24 bytes in.
        if tag == FORMAT_EXTENSIBLE {
            if body.len() < 26 {
                return Err(Error::MalformedChunk("fmt "));
            }
            tag = read_u16(body, 24);
        }

        let format = match tag {
            FORMAT_PCM => SampleFormat::Int,
            FORMAT_IEEE_FLOAT => SampleFormat::Float,
            other => return Err(Error::UnsupportedFormat(other)),
        };

        if channels == 0 {
            return Err(Error::MalformedChunk("fmt "));
        }
        if sample_rate == 0 {
            return Err(Error::MalformedChunk("fmt "));
        }
        match (format, bits_per_sample) {
            (SampleFormat::Int, 16 | 24 | 32) | (SampleFormat::Float, 32 | 64) => {}
            _ => return Err(Error::UnsupportedBitDepth(bits_per_sample)),
        }

        Ok(Format {
            format,
            channels,
            sample_rate,
            bits_per_sample,
            block_align,
        })
    }

    pub fn bytes_per_sample(&self) -> usize {
        self.bits_per_sample as usize / 8
    }

    /// Bytes per frame, computed from bit depth and channel count rather than
    /// taken from `block_align` — encoders get that field wrong often enough
    /// that trusting it would misalign the whole stream.
    pub fn frame_size(&self) -> usize {
        self.bytes_per_sample() * self.channels as usize
    }

    /// Whether `block_align` agrees with the computed frame size. A mismatch
    /// is worth reporting in `info`, but is not fatal.
    pub fn block_align_is_consistent(&self) -> bool {
        self.block_align as usize == self.frame_size()
    }
}

/// The `acid` chunk — how ACIDized loops declare tempo and length.
///
/// This is the most reliable tempo source a file can carry, and the reason
/// reading unknown chunks matters: it beats parsing the filename.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AcidChunk {
    pub flags: u32,
    pub root_note: u16,
    pub beats: u32,
    pub meter_denominator: u16,
    pub meter_numerator: u16,
    pub tempo: f32,
}

impl AcidChunk {
    pub const FLAG_ONE_SHOT: u32 = 0x01;
    pub const FLAG_ROOT_NOTE_SET: u32 = 0x02;
    pub const FLAG_STRETCH: u32 = 0x04;
    pub const FLAG_DISK_BASED: u32 = 0x08;

    pub fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < 24 {
            return None;
        }
        Some(AcidChunk {
            flags: read_u32(body, 0),
            root_note: read_u16(body, 4),
            // bytes 6..12 are two fields no documentation agrees on
            beats: read_u32(body, 12),
            meter_denominator: read_u16(body, 16),
            meter_numerator: read_u16(body, 18),
            tempo: f32::from_le_bytes([body[20], body[21], body[22], body[23]]),
        })
    }

    pub fn is_one_shot(&self) -> bool {
        self.flags & Self::FLAG_ONE_SHOT != 0
    }
}

/// One loop region from the `smpl` chunk.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SampleLoop {
    pub start: u32,
    /// Inclusive last frame, per the RIFF spec — not a half-open end.
    pub end: u32,
    pub play_count: u32,
}

/// The `smpl` chunk — loop points and tuning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmplChunk {
    pub midi_unity_note: u32,
    pub midi_pitch_fraction: u32,
    pub loops: Vec<SampleLoop>,
}

impl SmplChunk {
    pub fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < 36 {
            return None;
        }
        let midi_unity_note = read_u32(body, 12);
        let midi_pitch_fraction = read_u32(body, 16);
        let loop_count = read_u32(body, 28) as usize;

        let mut loops = Vec::new();
        for i in 0..loop_count {
            let at = 36 + i * 24;
            if at + 24 > body.len() {
                break; // truncated loop table: keep what parsed
            }
            loops.push(SampleLoop {
                start: read_u32(body, at + 8),
                end: read_u32(body, at + 12),
                play_count: read_u32(body, at + 20),
            });
        }

        Some(SmplChunk {
            midi_unity_note,
            midi_pitch_fraction,
            loops,
        })
    }
}

/// Reads `LIST`/`INFO` sub-chunks into `(id, text)` pairs.
pub fn parse_info_list(body: &[u8]) -> Vec<(ChunkId, String)> {
    if body.len() < 4 || read_id(body, 0) != INFO {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = &body[4..];
    while rest.len() >= 8 {
        let id = read_id(rest, 0);
        let size = (read_u32(rest, 4) as usize).min(rest.len() - 8);
        let text = String::from_utf8_lossy(&rest[8..8 + size])
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if !text.is_empty() {
            out.push((id, text));
        }
        let advance = 8 + size + (size & 1);
        rest = rest.get(advance..).unwrap_or(&[]);
    }
    out
}

fn read_id(b: &[u8], at: usize) -> ChunkId {
    [b[at], b[at + 1], b[at + 2], b[at + 3]]
}

fn read_u16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn read_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a RIFF/WAVE envelope around the given chunk bodies.
    fn riff(chunks: &[(ChunkId, Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::from(WAVE);
        for (id, data) in chunks {
            body.extend_from_slice(id);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0); // pad byte
            }
        }
        let mut out = Vec::from(RIFF);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn fmt_pcm(channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let block_align = channels * bits / 8;
        let mut v = Vec::new();
        v.extend_from_slice(&FORMAT_PCM.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * block_align as u32).to_le_bytes());
        v.extend_from_slice(&block_align.to_le_bytes());
        v.extend_from_slice(&bits.to_le_bytes());
        v
    }

    #[test]
    fn walks_chunks_in_order() {
        let bytes = riff(&[
            (FMT, fmt_pcm(2, 44_100, 16)),
            (DATA, vec![1, 2, 3, 4]),
            (ACID, vec![0; 24]),
        ]);
        let ids: Vec<ChunkId> = Chunks::parse(&bytes).unwrap().map(|c| c.id).collect();
        assert_eq!(ids, vec![FMT, DATA, ACID]);
    }

    #[test]
    fn odd_sized_chunks_are_padded_but_not_included() {
        // Three bytes of data, then a pad byte that must not leak into the
        // body nor shift the next chunk.
        let bytes = riff(&[(DATA, vec![9, 9, 9]), (ACID, vec![0; 24])]);
        let chunks: Vec<Chunk> = Chunks::parse(&bytes).unwrap().collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].body, &[9, 9, 9]);
        assert_eq!(chunks[1].id, ACID);
    }

    #[test]
    fn rejects_non_riff() {
        assert_eq!(Chunks::parse(b"not a wave file at all").err(), Some(Error::NotRiff));
        assert_eq!(Chunks::parse(&[]).err(), Some(Error::NotRiff));

        let mut bytes = riff(&[(DATA, vec![0; 4])]);
        bytes[8..12].copy_from_slice(b"AVI ");
        assert_eq!(Chunks::parse(&bytes).err(), Some(Error::NotWave));
    }

    #[test]
    fn survives_a_truncated_file() {
        // A data chunk claiming far more than the file holds — what a cut-off
        // download or an interrupted phone export looks like.
        let mut bytes = riff(&[(FMT, fmt_pcm(1, 44_100, 16)), (DATA, vec![7; 100])]);
        bytes.truncate(bytes.len() - 60);
        let chunks: Vec<Chunk> = Chunks::parse(&bytes).unwrap().collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].id, DATA);
        assert_eq!(chunks[1].body.len(), 40); // what actually survived
        assert!(chunks[1].body.iter().all(|&b| b == 7));
    }

    #[test]
    fn a_riff_size_field_that_understates_the_file_costs_no_audio() {
        // Caustic writes the RIFF size 44 bytes short — the length of a
        // classic WAVE header. Believing it truncated the data chunk and left
        // it ending mid-frame, losing audio from the end of every export.
        let mut bytes = riff(&[(FMT, fmt_pcm(2, 44_100, 24)), (DATA, vec![7; 600])]);
        let honest = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        bytes[4..8].copy_from_slice(&(honest - 44).to_le_bytes());

        let chunks: Vec<Chunk> = Chunks::parse(&bytes).unwrap().collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].id, DATA);
        assert_eq!(chunks[1].body.len(), 600, "data chunk was truncated");
        assert_eq!(chunks[1].body.len() % 6, 0, "left ending mid-frame");

        assert_eq!(Chunks::size_field_error(&bytes), Some(-44));
    }

    #[test]
    fn size_field_error_reports_both_directions() {
        let bytes = riff(&[(DATA, vec![0; 8])]);
        assert_eq!(Chunks::size_field_error(&bytes), Some(0));

        let mut over = bytes.clone();
        let honest = u32::from_le_bytes([over[4], over[5], over[6], over[7]]);
        over[4..8].copy_from_slice(&(honest + 100).to_le_bytes());
        assert_eq!(Chunks::size_field_error(&over), Some(100));

        assert_eq!(Chunks::size_field_error(b"nope"), None);
    }

    #[test]
    fn parses_plain_pcm_formats() {
        for (channels, rate, bits) in [(1u16, 44_100u32, 16u16), (2, 48_000, 24), (2, 96_000, 32)] {
            let f = Format::parse(&fmt_pcm(channels, rate, bits)).unwrap();
            assert_eq!(f.format, SampleFormat::Int);
            assert_eq!(f.channels, channels);
            assert_eq!(f.sample_rate, rate);
            assert_eq!(f.bits_per_sample, bits);
            assert_eq!(f.frame_size(), channels as usize * bits as usize / 8);
        }
    }

    #[test]
    fn resolves_wave_format_extensible() {
        // Tag 0xFFFE with the real format hidden in the SubFormat GUID.
        let mut body = fmt_pcm(2, 44_100, 24);
        body[0..2].copy_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
        body.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        body.extend_from_slice(&24u16.to_le_bytes()); // valid bits
        body.extend_from_slice(&3u32.to_le_bytes()); // channel mask
        body.extend_from_slice(&FORMAT_PCM.to_le_bytes()); // GUID starts here
        body.extend_from_slice(&[0; 14]);

        let f = Format::parse(&body).unwrap();
        assert_eq!(f.format, SampleFormat::Int);
        assert_eq!(f.bits_per_sample, 24);

        // The same envelope declaring float.
        let mut float_body = body.clone();
        float_body[24..26].copy_from_slice(&FORMAT_IEEE_FLOAT.to_le_bytes());
        float_body[14..16].copy_from_slice(&32u16.to_le_bytes());
        assert_eq!(Format::parse(&float_body).unwrap().format, SampleFormat::Float);
    }

    #[test]
    fn rejects_formats_we_cannot_read() {
        let mut adpcm = fmt_pcm(2, 44_100, 16);
        adpcm[0..2].copy_from_slice(&0x0011u16.to_le_bytes());
        assert_eq!(Format::parse(&adpcm).err(), Some(Error::UnsupportedFormat(0x11)));

        let eight_bit = fmt_pcm(1, 44_100, 8);
        assert_eq!(
            Format::parse(&eight_bit).err(),
            Some(Error::UnsupportedBitDepth(8))
        );

        assert_eq!(Format::parse(&[0; 4]).err(), Some(Error::MalformedChunk("fmt ")));

        let mut zero_channels = fmt_pcm(1, 44_100, 16);
        zero_channels[2..4].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            Format::parse(&zero_channels).err(),
            Some(Error::MalformedChunk("fmt "))
        );
    }

    #[test]
    fn parses_acid_tempo() {
        let mut body = vec![0u8; 24];
        body[0..4].copy_from_slice(&AcidChunk::FLAG_STRETCH.to_le_bytes());
        body[12..16].copy_from_slice(&32u32.to_le_bytes()); // beats
        body[16..18].copy_from_slice(&4u16.to_le_bytes());
        body[18..20].copy_from_slice(&4u16.to_le_bytes());
        body[20..24].copy_from_slice(&103.0f32.to_le_bytes());

        let acid = AcidChunk::parse(&body).unwrap();
        assert_eq!(acid.tempo, 103.0);
        assert_eq!(acid.beats, 32); // 8 bars of 4/4
        assert_eq!((acid.meter_numerator, acid.meter_denominator), (4, 4));
        assert!(!acid.is_one_shot());

        assert_eq!(AcidChunk::parse(&[0; 10]), None);
    }

    #[test]
    fn parses_smpl_loops() {
        let mut body = vec![0u8; 36];
        body[12..16].copy_from_slice(&60u32.to_le_bytes()); // unity note
        body[28..32].copy_from_slice(&1u32.to_le_bytes()); // one loop
        let mut loop_data = vec![0u8; 24];
        loop_data[8..12].copy_from_slice(&0u32.to_le_bytes());
        loop_data[12..16].copy_from_slice(&822_057u32.to_le_bytes());
        body.extend_from_slice(&loop_data);

        let smpl = SmplChunk::parse(&body).unwrap();
        assert_eq!(smpl.midi_unity_note, 60);
        assert_eq!(smpl.loops.len(), 1);
        assert_eq!(smpl.loops[0].start, 0);
        assert_eq!(smpl.loops[0].end, 822_057);
    }

    #[test]
    fn truncated_loop_table_keeps_what_parsed() {
        let mut body = vec![0u8; 36];
        body[28..32].copy_from_slice(&5u32.to_le_bytes()); // claims five loops
        body.extend_from_slice(&[0u8; 24]); // provides one
        assert_eq!(SmplChunk::parse(&body).unwrap().loops.len(), 1);
    }

    #[test]
    fn parses_info_text() {
        let mut body = Vec::from(INFO);
        for (id, text) in [(ICMT, "103 BPM, 8 bars\0"), (*b"INAM", "loop")] {
            body.extend_from_slice(&id);
            body.extend_from_slice(&(text.len() as u32).to_le_bytes());
            body.extend_from_slice(text.as_bytes());
            if text.len() % 2 == 1 {
                body.push(0);
            }
        }
        let info = parse_info_list(&body);
        assert_eq!(info.len(), 2);
        assert_eq!(info[0], (ICMT, "103 BPM, 8 bars".to_string()));
        assert_eq!(info[1], (*b"INAM", "loop".to_string()));

        // A LIST that is not an INFO list yields nothing.
        assert!(parse_info_list(b"adtl").is_empty());
    }
}
