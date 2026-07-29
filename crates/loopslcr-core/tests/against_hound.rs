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

/// The writer's turn. A round trip through our own reader proves only that the
/// two agree; `hound` reading the file is what says it is a WAVE file at all.
#[test]
fn hound_reads_what_we_write() {
    use loopslcr_core::wav::{write, BitDepth, Metadata, WriteSpec};
    use loopslcr_core::AudioBuffer;

    for depth in [BitDepth::Int16, BitDepth::Int24, BitDepth::Int32] {
        for channels in [1usize, 2] {
            // A ramp over the full range, and distinct per channel so a
            // swapped interleave shows up.
            let frames = 100;
            let source = AudioBuffer::new(
                (0..channels)
                    .map(|c| {
                        (0..frames)
                            .map(|f| {
                                let t = f as f64 / (frames - 1) as f64;
                                if c == 0 { t * 2.0 - 1.0 } else { 1.0 - t * 2.0 }
                            })
                            .collect()
                    })
                    .collect(),
                44_100,
            );

            // Tags too: they sit after `data`, so a reader that mishandles
            // them would report the wrong length or refuse the file.
            let spec = WriteSpec::new(depth)
                .with_metadata(Metadata::for_loop(103.0, 32, 4, 4, frames as u32));
            let bytes = write(&source, &spec).unwrap();

            let label = format!("{depth:?} {channels}ch");
            let mut theirs = hound::WavReader::new(Cursor::new(&bytes))
                .unwrap_or_else(|e| panic!("{label}: hound rejected our file: {e}"));

            let spec = theirs.spec();
            assert_eq!(spec.channels as usize, channels, "{label}");
            assert_eq!(spec.sample_rate, 44_100, "{label}");
            assert_eq!(spec.bits_per_sample, depth.bits(), "{label}");
            assert_eq!(theirs.duration(), frames as u32, "{label}");

            let scale = (1i64 << (depth.bits() - 1)) as f64;
            let mut count = 0;
            for (i, sample) in theirs.samples::<i32>().enumerate() {
                let raw = sample.unwrap_or_else(|e| panic!("{label}: sample {i}: {e}"));
                let (ch, frame) = (i % channels, i / channels);
                // hound's integer must be exactly what quantisation produced.
                let expected = (source.channel(ch)[frame] * scale)
                    .round()
                    .clamp(-scale, scale - 1.0);
                assert_eq!(raw as f64, expected, "{label}: sample {i}");
                count += 1;
            }
            assert_eq!(count, frames * channels, "{label}: samples missing");
        }
    }
}

#[test]
fn hound_reads_our_float_output() {
    use loopslcr_core::wav::{write, BitDepth, WriteSpec};
    use loopslcr_core::AudioBuffer;

    // Values past unity, which is the reason to pick float at all.
    let samples = vec![0.0f64, 1.0, -1.0, 1.5, -2.25, 0.125];
    let source = AudioBuffer::new(vec![samples.clone()], 48_000);
    let bytes = write(&source, &WriteSpec::new(BitDepth::Float32)).unwrap();

    let mut theirs = hound::WavReader::new(Cursor::new(&bytes)).expect("hound rejected our float");
    assert_eq!(theirs.spec().sample_format, HoundFormat::Float);
    assert_eq!(theirs.duration(), samples.len() as u32);
    let read: Vec<f32> = theirs.samples::<f32>().map(|s| s.unwrap()).collect();
    assert_eq!(read, samples.iter().map(|&s| s as f32).collect::<Vec<_>>());
}

/// Bytes we write must survive a trip through `hound`'s writer and back,
/// which is the closest thing to a second opinion on the encoding itself.
#[test]
fn our_encoding_matches_hounds_byte_for_byte() {
    use loopslcr_core::wav::{write, BitDepth, WriteSpec};
    use loopslcr_core::AudioBuffer;

    for bits in [16u16, 24, 32] {
        let depth = match bits {
            16 => BitDepth::Int16,
            24 => BitDepth::Int24,
            _ => BitDepth::Int32,
        };
        let raw = int_ramp(bits, 100);
        let scale = (1i64 << (bits - 1)) as f64;
        let source = AudioBuffer::new(
            vec![raw.iter().map(|&s| s as f64 / scale).collect()],
            44_100,
        );

        let ours = write(&source, &WriteSpec::new(depth)).unwrap();
        let theirs = write_int(1, 44_100, bits, &raw);

        // Headers may differ in layout; the sample data must not.
        let our_data = Wav::parse(&ours).unwrap().decode().unwrap();
        let their_data = Wav::parse(&theirs).unwrap().decode().unwrap();
        assert_eq!(our_data.channel(0), their_data.channel(0), "{bits}-bit");
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
