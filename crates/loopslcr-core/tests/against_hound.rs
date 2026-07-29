//! Checks the hand-rolled reader against `hound`.
//!
//! `hound` writes the bytes here, so the reader has to interpret a file it did
//! not produce. A test that both wrote and read with our own code would agree
//! with its own mistakes.

#![allow(clippy::float_arithmetic)]

use std::io::Cursor;

use hound::{SampleFormat as HoundFormat, WavSpec, WavWriter};
use loopslcr_core::wav::Wav;

fn write_int(channels: u16, sample_rate: u32, bits: u16, samples: &[i32]) -> Vec<u8> {
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: bits,
        sample_format: HoundFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut w = WavWriter::new(&mut cursor, spec).unwrap();
        for &s in samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }
    cursor.into_inner()
}

fn write_float(channels: u16, sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: HoundFormat::Float,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut w = WavWriter::new(&mut cursor, spec).unwrap();
        for &s in samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }
    cursor.into_inner()
}

/// Spread across the full range of `bits`, including both extremes.
fn int_ramp(bits: u16, count: usize) -> Vec<i32> {
    let max = (1i64 << (bits - 1)) - 1;
    let min = -(1i64 << (bits - 1));
    (0..count)
        .map(|i| {
            let t = i as f64 / (count - 1) as f64;
            (min as f64 + t * (max - min) as f64).round() as i32
        })
        .collect()
}

#[test]
fn integer_formats_match_hound() {
    for bits in [16u16, 24, 32] {
        for channels in [1u16, 2] {
            for rate in [44_100u32, 48_000] {
                let samples = int_ramp(bits, 64);
                let bytes = write_int(channels, rate, bits, &samples);

                let wav = Wav::parse(&bytes).unwrap_or_else(|e| {
                    panic!("{bits}-bit {channels}ch @ {rate}: parse failed: {e}")
                });
                assert_eq!(wav.sample_rate(), rate);
                assert_eq!(wav.channel_count(), channels as usize);
                assert_eq!(wav.frames(), samples.len() / channels as usize);
                assert!(!wav.has_partial_frame());

                let buf = wav.decode().unwrap();
                let scale = (1i64 << (bits - 1)) as f64;
                for (i, &raw) in samples.iter().enumerate() {
                    let channel = i % channels as usize;
                    let frame = i / channels as usize;
                    assert_eq!(
                        buf.channel(channel)[frame],
                        raw as f64 / scale,
                        "{bits}-bit {channels}ch @ {rate}, sample {i} (raw {raw})"
                    );
                }
            }
        }
    }
}

#[test]
fn float_format_matches_hound() {
    let samples: Vec<f32> = (0..64).map(|i| (i as f32 / 32.0) - 1.0).collect();
    let bytes = write_float(2, 44_100, &samples);

    let wav = Wav::parse(&bytes).unwrap();
    assert_eq!(wav.channel_count(), 2);
    assert_eq!(wav.frames(), 32);

    let buf = wav.decode().unwrap();
    for (i, &s) in samples.iter().enumerate() {
        assert_eq!(buf.channel(i % 2)[i / 2], s as f64);
    }
}

/// The other direction: hound reads the same bytes and must agree with us on
/// the decoded values.
#[test]
fn both_readers_agree_sample_for_sample() {
    for bits in [16u16, 24, 32] {
        let samples = int_ramp(bits, 100);
        let bytes = write_int(2, 44_100, bits, &samples);

        let ours = Wav::parse(&bytes).unwrap().decode().unwrap();

        let mut theirs = hound::WavReader::new(Cursor::new(&bytes)).unwrap();
        let scale = (1i64 << (bits - 1)) as f64;
        for (i, sample) in theirs.samples::<i32>().enumerate() {
            let raw = sample.unwrap();
            assert_eq!(
                ours.channel(i % 2)[i / 2],
                raw as f64 / scale,
                "{bits}-bit, interleaved index {i}"
            );
        }
    }
}

#[test]
fn duration_matches_hound() {
    let bytes = write_int(2, 44_100, 16, &vec![0i32; 44_100 * 2]);
    let wav = Wav::parse(&bytes).unwrap();
    let theirs = hound::WavReader::new(Cursor::new(&bytes)).unwrap();

    assert_eq!(wav.frames() as u32, theirs.duration());
    assert!((wav.duration_seconds() - 1.0).abs() < 1e-12);
}
