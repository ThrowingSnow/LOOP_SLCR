//! Sweeps a directory of real files, checking the reader against `hound`.
//!
//! Skipped unless `LOOPSLCR_ARCHIVE` points at a directory, since the archive
//! is not part of the repository. This is the M1 exit criterion in the form it
//! can take before `cut` exists: every file parses, and every sample agrees
//! with an independent reader.
//!
//! ```sh
//! LOOPSLCR_ARCHIVE="$HOME/AUDIO/DRUMLOOPS" cargo test --test archive_sweep -- --nocapture
//! ```

#![allow(clippy::float_arithmetic)]

use std::io::Cursor;
use std::path::PathBuf;

use loopslcr_core::wav::{SampleFormat, Wav};

fn archive() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("LOOPSLCR_ARCHIVE")?);
    dir.is_dir().then_some(dir)
}

#[test]
fn every_file_parses_and_matches_hound() {
    let Some(dir) = archive() else {
        eprintln!("LOOPSLCR_ARCHIVE not set — skipping archive sweep");
        return;
    };

    let mut checked = 0usize;
    let mut skipped = 0usize;
    let mut frames_total = 0u64;
    let mut failures = Vec::new();

    for entry in std::fs::read_dir(&dir).expect("archive unreadable") {
        let path = entry.expect("bad dir entry").path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).expect("file unreadable");
        // Not everything in the archive is a WAVE file; the sweep is about the
        // ones that are, whatever their extension says.
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            skipped += 1;
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

        let wav = match Wav::parse(&bytes) {
            Ok(w) => w,
            Err(e) => {
                failures.push(format!("{name}: parse failed: {e}"));
                continue;
            }
        };
        let ours = match wav.decode() {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{name}: decode failed: {e}"));
                continue;
            }
        };

        let mut theirs = match hound::WavReader::new(Cursor::new(&bytes)) {
            Ok(r) => r,
            Err(e) => {
                // hound refusing a file we accepted is worth knowing about,
                // but it is not automatically our bug.
                eprintln!("  note: hound rejected {name}: {e}");
                checked += 1;
                continue;
            }
        };

        let spec = theirs.spec();
        if spec.sample_rate != wav.sample_rate() || spec.channels as usize != wav.channel_count() {
            failures.push(format!(
                "{name}: header mismatch — ours {} Hz/{} ch, hound {} Hz/{} ch",
                wav.sample_rate(),
                wav.channel_count(),
                spec.sample_rate,
                spec.channels
            ));
            continue;
        }

        let channels = wav.channel_count();
        let mut mismatches = 0usize;
        match wav.format().format {
            SampleFormat::Int => {
                let scale = (1i64 << (wav.format().bits_per_sample - 1)) as f64;
                for (i, sample) in theirs.samples::<i32>().enumerate() {
                    let Ok(raw) = sample else { break };
                    let (ch, frame) = (i % channels, i / channels);
                    if frame >= ours.frames() {
                        break;
                    }
                    if ours.channel(ch)[frame] != raw as f64 / scale {
                        mismatches += 1;
                    }
                }
            }
            SampleFormat::Float => {
                for (i, sample) in theirs.samples::<f32>().enumerate() {
                    let Ok(raw) = sample else { break };
                    let (ch, frame) = (i % channels, i / channels);
                    if frame >= ours.frames() {
                        break;
                    }
                    if ours.channel(ch)[frame] != raw as f64 {
                        mismatches += 1;
                    }
                }
            }
        }
        if mismatches > 0 {
            failures.push(format!("{name}: {mismatches} samples differ from hound"));
        }

        frames_total += ours.frames() as u64;
        checked += 1;
    }

    eprintln!(
        "archive sweep: {checked} WAVE files checked, {skipped} non-WAVE skipped, \
         {frames_total} frames verified"
    );
    assert!(checked > 0, "no WAVE files found in {}", dir.display());
    assert!(failures.is_empty(), "{} problem(s):\n{}", failures.len(), failures.join("\n"));
}
