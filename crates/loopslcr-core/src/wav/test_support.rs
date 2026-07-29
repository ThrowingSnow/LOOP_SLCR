//! Building WAVE files in memory, for tests.
//!
//! Writing bytes by hand in every test would make the tests agree with
//! whatever mistake the reader makes. These helpers assemble the bytes from
//! the spec independently.

use super::chunks::{ChunkId, DATA, FMT, FORMAT_IEEE_FLOAT, FORMAT_PCM, RIFF, WAVE};

/// One encoded sample, as it appears in a `data` chunk.
pub trait TestSample: Copy {
    fn encode(self, bits: u16) -> Vec<u8>;
}

impl TestSample for i32 {
    fn encode(self, bits: u16) -> Vec<u8> {
        match bits {
            16 => (self as i16).to_le_bytes().to_vec(),
            24 => self.to_le_bytes()[..3].to_vec(),
            32 => self.to_le_bytes().to_vec(),
            other => panic!("no integer encoding for {other} bits"),
        }
    }
}

impl TestSample for f32 {
    fn encode(self, bits: u16) -> Vec<u8> {
        match bits {
            32 => self.to_le_bytes().to_vec(),
            64 => (self as f64).to_le_bytes().to_vec(),
            other => panic!("no float encoding for {other} bits"),
        }
    }
}

/// The shape of a file to build.
#[derive(Clone, Debug)]
pub struct WavSpec {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
    /// Extra chunks, written before `data`.
    pub extra: Vec<(ChunkId, Vec<u8>)>,
}

impl WavSpec {
    pub fn int(channels: u16, sample_rate: u32, bits: u16) -> Self {
        WavSpec {
            format_tag: FORMAT_PCM,
            channels,
            sample_rate,
            bits,
            extra: Vec::new(),
        }
    }

    pub fn float(channels: u16, sample_rate: u32, bits: u16) -> Self {
        WavSpec {
            format_tag: FORMAT_IEEE_FLOAT,
            channels,
            sample_rate,
            bits,
            extra: Vec::new(),
        }
    }

    pub fn with_chunk(mut self, id: ChunkId, body: Vec<u8>) -> Self {
        self.extra.push((id, body));
        self
    }

    fn fmt_body(&self) -> Vec<u8> {
        let block_align = self.channels * self.bits / 8;
        let mut v = Vec::new();
        v.extend_from_slice(&self.format_tag.to_le_bytes());
        v.extend_from_slice(&self.channels.to_le_bytes());
        v.extend_from_slice(&self.sample_rate.to_le_bytes());
        v.extend_from_slice(&(self.sample_rate * block_align as u32).to_le_bytes());
        v.extend_from_slice(&block_align.to_le_bytes());
        v.extend_from_slice(&self.bits.to_le_bytes());
        v
    }
}

/// Assembles a complete WAVE file. `samples` are interleaved.
pub fn riff<S: TestSample>(spec: WavSpec, samples: &[S]) -> Vec<u8> {
    let mut data = Vec::new();
    for s in samples {
        data.extend_from_slice(&s.encode(spec.bits));
    }

    let mut body = Vec::from(WAVE);
    push_chunk(&mut body, FMT, &spec.fmt_body());
    for (id, chunk) in &spec.extra {
        push_chunk(&mut body, *id, chunk);
    }
    push_chunk(&mut body, DATA, &data);

    let mut out = Vec::from(RIFF);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn push_chunk(out: &mut Vec<u8>, id: ChunkId, body: &[u8]) {
    out.extend_from_slice(&id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0); // word alignment
    }
}
