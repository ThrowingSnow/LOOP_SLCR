//! `loopslcr batch` end to end, through the actual binary.
//!
//! Driving the binary rather than calling into it: the batch's job is to keep
//! going past failures and to say what happened, and both of those are
//! observable only as output and an exit code. A test that called an internal
//! function would check the parts that were never in doubt.
//!
//! Everything here is synthesised at 8 kHz so a whole archive's worth of cases
//! costs a few hundred kilobytes. One bar at 200 BPM in 4/4 is exactly 9600
//! frames at that rate, which keeps the arithmetic in the test as exact as the
//! arithmetic under test.

use std::path::{Path, PathBuf};
use std::process::Command;

use loopslcr_core::wav::{write, BitDepth, Metadata, WriteSpec};
use loopslcr_core::AudioBuffer;

const RATE: u32 = 8_000;
/// One bar of 4/4 at 200 BPM, at [`RATE`]. Exact, deliberately.
const BAR: usize = 9_600;

/// A scratch directory of its own, removed when the guard drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("loopslcr-batch-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("could not make a scratch directory");
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes a `bars`-long loop, named so the tempo can be read off it.
fn loop_file(dir: &Path, name: &str, bars: usize) {
    let frames = bars * BAR;
    // A quiet tone rather than silence: the tail measurement and the peak report
    // both have something to find, as they would in a real render.
    let data: Vec<f64> = (0..frames)
        .map(|i| 0.25 * (i as f64 * 0.05).sin())
        .collect();
    let buffer = AudioBuffer::new(vec![data], RATE);
    let bytes = write::write(
        &buffer,
        &WriteSpec {
            // 24-bit so a `--depth 16` in a preset has a visible effect —
            // dither only applies when the depth actually drops.
            depth: BitDepth::Int24,
            metadata: Metadata::default(),
        },
    )
    .expect("could not encode a test loop");
    std::fs::write(dir.join(name), bytes).expect("could not write a test loop");
}

struct Run {
    stdout: String,
    ok: bool,
}

fn batch(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_loopslcr"))
        .arg("batch")
        .args(args)
        .output()
        .expect("could not run loopslcr");
    Run {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        ok: output.status.success(),
    }
}

#[test]
fn a_file_that_cannot_be_cut_does_not_stop_the_others() {
    let scratch = Scratch::new("carry-on");
    let dir = scratch.path();
    loop_file(dir, "200 first.wav", 4);
    loop_file(dir, "200 second.wav", 4);
    // No tempo in the name and no `acid` chunk: nothing to cut against.
    loop_file(dir, "nameless.wav", 4);
    // Not audio at all.
    std::fs::write(dir.join("notes.txt"), b"not a wave file").unwrap();

    let run = batch(&[dir.to_str().unwrap(), "--dry-run"]);

    assert!(run.stdout.contains("2 cut, 1 failed, 1 not WAVE files"), "{}", run.stdout);
    assert!(run.stdout.contains("no tempo known"), "{}", run.stdout);
    // Both good files were still reported, which is the whole point.
    assert!(run.stdout.contains("200 first.wav"), "{}", run.stdout);
    assert!(run.stdout.contains("200 second.wav"), "{}", run.stdout);
    // A failure is a failure, even though the batch carried on: a script that
    // sees success here would go on to use files that were never written.
    assert!(!run.ok, "exit code was success despite a failure");
}

#[test]
fn a_batch_where_everything_works_exits_zero() {
    let scratch = Scratch::new("all-good");
    let dir = scratch.path();
    loop_file(dir, "200 one.wav", 4);
    loop_file(dir, "200 two.wav", 8);

    let run = batch(&[dir.to_str().unwrap(), "--dry-run"]);
    assert!(run.stdout.contains("2 cut, 0 failed, 0 not WAVE files"), "{}", run.stdout);
    assert!(run.ok, "{}", run.stdout);
    // The loop length came from each file's own duration, not from a default.
    assert!(run.stdout.contains("4 bars at 200 BPM"), "{}", run.stdout);
    assert!(run.stdout.contains("8 bars at 200 BPM"), "{}", run.stdout);
}

/// The bug the real archive found: `Path::file_stem` strips whatever follows the
/// last dot, so two extension-less files differing only after a dot were named
/// the same output and the second refused to overwrite the first.
#[test]
fn two_files_that_differ_only_after_a_dot_produce_two_outputs() {
    let scratch = Scratch::new("stems");
    let dir = scratch.path();
    let out = scratch.path().join("out");
    loop_file(dir, "200-SMPL.BRN-21OCT23-01", 4);
    loop_file(dir, "200-SMPL.BRN-21OCT23-02", 4);

    let run = batch(&[dir.to_str().unwrap(), "--out-dir", out.to_str().unwrap()]);
    assert!(run.stdout.contains("2 cut, 0 failed"), "{}", run.stdout);
    assert!(run.ok, "{}", run.stdout);

    let mut written: Vec<String> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    written.sort();
    assert_eq!(
        written,
        vec![
            "200-SMPL.BRN-21OCT23-01_200bpm_4bars.wav",
            "200-SMPL.BRN-21OCT23-02_200bpm_4bars.wav",
        ]
    );
}

#[test]
fn out_dir_recreates_the_tree_below_the_source() {
    let scratch = Scratch::new("mirror");
    let dir = scratch.path();
    let out = scratch.path().join("cut");
    std::fs::create_dir_all(dir.join("kit a")).unwrap();
    std::fs::create_dir_all(dir.join("kit b")).unwrap();
    // The same filename in two folders: flattening would lose one of them.
    loop_file(&dir.join("kit a"), "200 loop.wav", 4);
    loop_file(&dir.join("kit b"), "200 loop.wav", 4);

    let run = batch(&[
        dir.to_str().unwrap(),
        "--recursive",
        "--out-dir",
        out.to_str().unwrap(),
    ]);
    assert!(run.stdout.contains("2 cut, 0 failed"), "{}", run.stdout);
    assert!(out.join("kit a/200 loop_200bpm_4bars.wav").is_file(), "{}", run.stdout);
    assert!(out.join("kit b/200 loop_200bpm_4bars.wav").is_file(), "{}", run.stdout);
}

#[test]
fn without_recursive_a_subdirectory_is_left_alone() {
    let scratch = Scratch::new("shallow");
    let dir = scratch.path();
    std::fs::create_dir_all(dir.join("deeper")).unwrap();
    loop_file(dir, "200 top.wav", 4);
    loop_file(&dir.join("deeper"), "200 below.wav", 4);

    let run = batch(&[dir.to_str().unwrap(), "--dry-run"]);
    assert!(run.stdout.contains("1 cut, 0 failed"), "{}", run.stdout);
    assert!(!run.stdout.contains("200 below.wav"), "{}", run.stdout);
}

#[test]
fn the_report_is_the_same_however_many_threads_ran_it() {
    // Parallelism must not reach the output. Without the sort in `collect` this
    // fails intermittently, which is the worst way for it to fail.
    let scratch = Scratch::new("determinism");
    let dir = scratch.path();
    for i in 1..=12 {
        loop_file(dir, &format!("200 loop {i:02}.wav"), 4);
    }

    let one = batch(&[dir.to_str().unwrap(), "--dry-run", "--jobs", "1"]);
    let many = batch(&[dir.to_str().unwrap(), "--dry-run", "--jobs", "8"]);
    assert_eq!(one.stdout, many.stdout);
    assert!(one.stdout.contains("12 cut, 0 failed"), "{}", one.stdout);
}

#[test]
fn a_preset_applies_and_the_command_line_overrides_it() {
    let scratch = Scratch::new("preset");
    let dir = scratch.path();
    loop_file(dir, "200 loop.wav", 8);
    std::fs::write(
        dir.join("loopslcr.args"),
        "# how this folder was rendered\n--bars 4\n--depth 16\n",
    )
    .unwrap();

    let run = batch(&[dir.to_str().unwrap(), "--dry-run", "--verbose"]);
    // The preset is named in the output — a file that changes what the command
    // does must not do so invisibly.
    assert!(run.stdout.contains("preset"), "{}", run.stdout);
    assert!(run.stdout.contains("loopslcr.args"), "{}", run.stdout);
    assert!(run.stdout.contains("4 bars"), "{}", run.stdout);
    assert!(run.stdout.contains("at 16 bit"), "{}", run.stdout);

    // Typed flags win, because the preset is spliced in ahead of them.
    let over = batch(&[dir.to_str().unwrap(), "--dry-run", "--verbose", "--bars", "8"]);
    assert!(over.stdout.contains("8 bars"), "{}", over.stdout);

    // And the preset can be ignored wholesale, which is the honest way out of a
    // switch a preset turned on.
    let without = batch(&[dir.to_str().unwrap(), "--dry-run", "--no-preset"]);
    assert!(!without.stdout.contains("loopslcr.args"), "{}", without.stdout);
    assert!(without.stdout.contains("8 bars"), "{}", without.stdout);
}

#[test]
fn the_preset_file_is_not_treated_as_material() {
    let scratch = Scratch::new("preset-not-input");
    let dir = scratch.path();
    loop_file(dir, "200 loop.wav", 4);
    std::fs::write(dir.join("loopslcr.args"), "--bars 4\n").unwrap();

    let run = batch(&[dir.to_str().unwrap(), "--dry-run"]);
    // One file, and the preset is not counted as a non-WAVE skip either.
    assert!(run.stdout.contains("1 cut, 0 failed, 0 not WAVE files"), "{}", run.stdout);
}

#[test]
fn a_malformed_preset_is_refused_rather_than_ignored() {
    let scratch = Scratch::new("bad-preset");
    let dir = scratch.path();
    loop_file(dir, "200 loop.wav", 4);
    std::fs::write(dir.join("loopslcr.args"), "bars 4\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_loopslcr"))
        .arg("batch")
        .arg(dir)
        .arg("--dry-run")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "a broken preset was accepted");
    assert!(stderr.contains("not a flag"), "{stderr}");
    assert!(stderr.contains("line 1"), "{stderr}");
}

#[test]
fn an_empty_directory_is_an_error_not_a_silent_success() {
    let scratch = Scratch::new("empty");
    let run = batch(&[scratch.path().to_str().unwrap(), "--dry-run"]);
    assert!(!run.ok);
}
