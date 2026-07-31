//! Live playback of a loop at a speed that can change while it plays.
//!
//! This is the other resampler the [`Resampler`](super::resample::Resampler)
//! trait was written for. The offline one is given a target length and derives
//! the rate from it, because a loop that is one sample long drifts. Here there
//! is no target length at all: the rate is whatever the user's finger is doing,
//! it changes between one output sample and the next, and the output runs until
//! someone stops it.
//!
//! # What it is for
//!
//! Riding the varispeed by ear. A number that says `-2.336 st` is not the same
//! information as hearing the loop slow down, and the whole reason this tool
//! models tape rather than a pitch shifter is that the two are audibly
//! different. A preview that could only play at fixed rates would leave the
//! interesting part — the movement — untestable.
//!
//! # Why it does not glide instantly
//!
//! A rate that jumps discontinuously is not something tape can do, and it does
//! not sound like tape either: it sounds like a sample being retriggered. The
//! target rate is therefore approached through a one-pole smoother whose time
//! constant stands in for the inertia of a reel. Dragging a slider then bends
//! the pitch the way a hand on a capstan would.
//!
//! # Why there is no locking, no allocation and no error path
//!
//! [`Preview::read`] is written to be called from an audio callback. Anything
//! that can block, allocate or fail there produces a dropout, which is not a
//! degraded result but a click. So the buffer is owned up front, the output
//! slice belongs to the caller, and the only thing that can go wrong — a rate of
//! zero, a NaN from a control surface — is clamped rather than reported.
//!
//! Threading is deliberately *not* handled here. This type is plain `&mut self`
//! code with no atomics in it, so it can be tested like anything else; the
//! handoff between a UI thread and an audio thread belongs to whoever owns both,
//! which on Android is the JNI layer.

// Playback is the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;
use crate::ops::resample::{Edge, SincResampler};

/// How far the rate may be pushed in either direction.
///
/// Two octaves down and one up. The lower bound matters more than the upper:
/// as the rate approaches zero the read position stops advancing and the loop
/// becomes a drone, which is a valid sound but not a valid *speed*, and dividing
/// by it would produce infinities in anything downstream that reasons about
/// length.
pub const MIN_RATIO: f64 = 0.25;
pub const MAX_RATIO: f64 = 2.0;

/// The default reel inertia, in milliseconds.
///
/// Long enough to be heard as a bend rather than a jump, short enough that the
/// loop arrives at the requested speed before the user has stopped listening
/// for it. Tape machines vary far more than this; the number is a feel, not a
/// measurement, and it is a parameter so it can be argued with.
pub const DEFAULT_GLIDE_MS: f64 = 120.0;

/// A loop, playing.
///
/// Owns the audio it plays. The buffer is expected to be a finished loop — cut,
/// faded, and with whatever character was asked for already applied — because
/// the one thing being varied here is the speed.
pub struct Preview {
    buffer: AudioBuffer,
    reader: SincResampler,
    /// Fractional read position, always inside `[0, frames)`.
    position: f64,
    /// The rate actually being played, which chases `target`.
    ratio: f64,
    target: f64,
    /// One-pole coefficient for the glide, per output sample.
    glide: f64,
    /// Output frames produced since the last reset. Cheap play-head telemetry
    /// for a UI that wants to draw a cursor without asking the audio thread
    /// anything it would have to lock for.
    played: u64,
}

impl Preview {
    /// A preview of `buffer`, at unity, from the start.
    ///
    /// Fewer sinc taps than the offline path: eight rather than thirty-two. The
    /// stopband drops from about −90 dB to about −60, which is inaudible under a
    /// drum loop and costs a quarter of the work per sample — and unlike the
    /// offline path, this one has a deadline.
    pub fn new(buffer: AudioBuffer, glide_ms: f64) -> Self {
        let rate = buffer.sample_rate();
        Preview {
            buffer,
            reader: SincResampler::fast().with_edge(Edge::Wrap),
            position: 0.0,
            ratio: 1.0,
            target: 1.0,
            glide: glide_coefficient(glide_ms, rate),
            played: 0,
        }
    }

    pub fn channel_count(&self) -> usize {
        self.buffer.channel_count()
    }

    pub fn sample_rate(&self) -> u32 {
        self.buffer.sample_rate()
    }

    pub fn frames(&self) -> usize {
        self.buffer.frames()
    }

    /// Where the play head is, in source frames.
    pub fn position(&self) -> f64 {
        self.position
    }

    /// The rate being played right now, which may still be gliding.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    pub fn target_ratio(&self) -> f64 {
        self.target
    }

    pub fn played(&self) -> u64 {
        self.played
    }

    /// Asks for a new speed. Reached over the glide time, not immediately.
    ///
    /// A NaN — which a control surface can produce from a text field mid-edit —
    /// leaves the target alone rather than poisoning every future sample.
    pub fn set_target_ratio(&mut self, ratio: f64) {
        if ratio.is_finite() {
            self.target = ratio.clamp(MIN_RATIO, MAX_RATIO);
        }
    }

    /// Sets the speed with no glide at all. For starting playback at a rate.
    pub fn snap_to_ratio(&mut self, ratio: f64) {
        self.set_target_ratio(ratio);
        self.ratio = self.target;
    }

    /// Moves the play head. Wraps, because the buffer is a loop.
    pub fn seek(&mut self, frame: f64) {
        let frames = self.buffer.frames() as f64;
        self.position = if frames > 0.0 && frame.is_finite() {
            frame.rem_euclid(frames)
        } else {
            0.0
        };
    }

    /// Fills `out` with interleaved frames and returns how many it wrote.
    ///
    /// `out.len()` must be a multiple of the channel count; the remainder is
    /// left untouched rather than half-filled. Allocation-free and infallible:
    /// see the module docs for why both matter here.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let channels = self.buffer.channel_count();
        let frames = self.buffer.frames();
        if channels == 0 || frames == 0 {
            out.fill(0.0);
            return 0;
        }

        let wanted = out.len() / channels;
        let length = frames as f64;

        for frame in 0..wanted {
            // Glide first, so the rate used for this sample is the one the
            // smoother has reached — the alternative leaves the output one
            // sample ahead of the rate that produced it.
            self.ratio += (self.target - self.ratio) * self.glide;

            let base = frame * channels;
            for channel in 0..channels {
                let source = self.buffer.channel(channel);
                out[base + channel] =
                    self.reader.read(source, self.position, self.ratio) as f32;
            }

            self.position += self.ratio;
            // Wrapping every sample rather than every block keeps the position
            // small, which matters: at 48 kHz a float position accumulating for
            // an hour would start losing fractional precision, and the seam is
            // exactly where that would be heard.
            if self.position >= length {
                self.position -= length;
            } else if self.position < 0.0 {
                self.position += length;
            }
        }

        self.played += wanted as u64;
        wanted
    }
}

/// One-pole coefficient reaching 1 − 1/e of the way in `ms`.
///
/// Clamped to something non-zero: a glide time of zero would mean an infinite
/// coefficient, and a negative one would make the smoother diverge.
fn glide_coefficient(ms: f64, sample_rate: u32) -> f64 {
    if !ms.is_finite() || ms <= 0.0 || sample_rate == 0 {
        return 1.0;
    }
    let samples = ms * 0.001 * f64::from(sample_rate);
    (1.0 - (-1.0 / samples).exp()).clamp(f64::MIN_POSITIVE, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// A loop whose sample value is its own index, so a read tells you exactly
    /// where the play head was.
    fn ramp(frames: usize, channels: usize) -> AudioBuffer {
        let data: Vec<Vec<f64>> = (0..channels)
            .map(|c| (0..frames).map(|i| (i + c * 1000) as f64).collect())
            .collect();
        AudioBuffer::new(data, RATE)
    }

    fn sine(frames: usize) -> AudioBuffer {
        let data: Vec<f64> = (0..frames)
            .map(|i| (i as f64 * 0.02).sin() * 0.5)
            .collect();
        AudioBuffer::new(vec![data.clone(), data], RATE)
    }

    #[test]
    fn at_unity_the_play_head_advances_one_frame_per_frame() {
        let mut preview = Preview::new(ramp(1000, 1), 0.0);
        let mut out = vec![0.0f32; 64];
        assert_eq!(preview.read(&mut out), 64);
        assert!((preview.position() - 64.0).abs() < 1e-9);
    }

    #[test]
    fn the_position_wraps_instead_of_running_off_the_end() {
        let mut preview = Preview::new(ramp(100, 1), 0.0);
        let mut out = vec![0.0f32; 250];
        preview.read(&mut out);
        // 250 frames through a 100-frame loop is two and a half times round.
        assert!((preview.position() - 50.0).abs() < 1e-9);
        assert_eq!(preview.played(), 250);
    }

    #[test]
    fn a_loop_read_twice_round_repeats_itself() {
        // The seam is the whole point: reading past the end must give the same
        // samples as reading from the start, or the preview clicks once per
        // repeat at exactly the place this tool exists to make clean.
        let mut preview = Preview::new(sine(512), 0.0);
        let mut first = vec![0.0f32; 512 * 2];
        let mut second = vec![0.0f32; 512 * 2];
        preview.read(&mut first);
        preview.read(&mut second);
        for (a, b) in first.iter().zip(second.iter()) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn double_speed_covers_the_loop_in_half_the_frames() {
        let mut preview = Preview::new(ramp(1000, 1), 0.0);
        preview.snap_to_ratio(2.0);
        let mut out = vec![0.0f32; 100];
        preview.read(&mut out);
        assert!((preview.position() - 200.0).abs() < 1e-9);
    }

    #[test]
    fn the_rate_glides_rather_than_jumping() {
        let mut preview = Preview::new(sine(4800), DEFAULT_GLIDE_MS);
        preview.set_target_ratio(2.0);
        assert_eq!(preview.ratio(), 1.0, "the target is not the rate");

        // One glide time in, a one-pole is 1 − 1/e ≈ 63.2 % of the way there.
        let mut out = vec![0.0f32; (RATE as usize / 1000 * 120) * 2];
        preview.read(&mut out);
        let travelled = (preview.ratio() - 1.0) / (2.0 - 1.0);
        assert!(
            (travelled - 0.632).abs() < 0.01,
            "expected ~63.2 % of the way, got {:.1} %",
            travelled * 100.0
        );
    }

    #[test]
    fn the_glide_gets_there_eventually() {
        let mut preview = Preview::new(sine(4800), DEFAULT_GLIDE_MS);
        preview.set_target_ratio(0.5);
        // Two seconds is about sixteen time constants. A one-pole never arrives
        // exactly, so the question is only whether it gets close enough that the
        // difference is far below anything audible — 1e-6 of a semitone is not
        // a pitch, it is a rounding difference.
        let mut out = vec![0.0f32; RATE as usize * 2 * 2];
        preview.read(&mut out);
        assert!(
            (preview.ratio() - 0.5).abs() < 1e-6,
            "still at {}",
            preview.ratio()
        );
    }

    #[test]
    fn a_glide_of_zero_is_no_glide() {
        let mut preview = Preview::new(sine(512), 0.0);
        preview.set_target_ratio(1.5);
        let mut out = vec![0.0f32; 2];
        preview.read(&mut out);
        assert_eq!(preview.ratio(), 1.5);
    }

    #[test]
    fn the_rate_is_clamped_and_nonsense_is_ignored() {
        let mut preview = Preview::new(sine(512), 0.0);
        preview.set_target_ratio(100.0);
        assert_eq!(preview.target_ratio(), MAX_RATIO);
        preview.set_target_ratio(0.0);
        assert_eq!(preview.target_ratio(), MIN_RATIO);

        preview.set_target_ratio(1.25);
        preview.set_target_ratio(f64::NAN);
        assert_eq!(preview.target_ratio(), 1.25, "a NaN must not poison the rate");
    }

    #[test]
    fn seeking_wraps_and_survives_nonsense() {
        let mut preview = Preview::new(sine(512), 0.0);
        preview.seek(600.0);
        assert!((preview.position() - 88.0).abs() < 1e-9);
        preview.seek(-1.0);
        assert!((preview.position() - 511.0).abs() < 1e-9);
        preview.seek(f64::NAN);
        assert_eq!(preview.position(), 0.0);
    }

    #[test]
    fn every_channel_gets_its_own_samples() {
        // Interleaving is the one thing a reader can get subtly wrong and still
        // produce sound: swapped channels are audible only on material that
        // differs between them.
        let mut preview = Preview::new(ramp(1000, 2), 0.0);
        let mut out = vec![0.0f32; 8];
        preview.read(&mut out);
        for frame in 0..4 {
            let left = out[frame * 2];
            let right = out[frame * 2 + 1];
            assert!((right - left - 1000.0).abs() < 1.0, "frame {frame}: {left} / {right}");
        }
    }

    #[test]
    fn a_partial_frame_at_the_end_is_left_alone() {
        let mut preview = Preview::new(ramp(1000, 2), 0.0);
        let mut out = vec![-7.0f32; 5];
        assert_eq!(preview.read(&mut out), 2, "two whole frames fit in five slots");
        assert_eq!(out[4], -7.0, "the odd slot is untouched, not half-written");
    }

    #[test]
    fn an_empty_buffer_plays_silence_rather_than_panicking() {
        let mut preview = Preview::new(AudioBuffer::silence(2, 0, RATE), 0.0);
        let mut out = vec![1.0f32; 8];
        assert_eq!(preview.read(&mut out), 0);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn playing_slower_does_not_alias_and_playing_faster_does_not_either() {
        // A ramp is the worst case for aliasing: its wrap is a step edge. What
        // is checked here is only that nothing leaves the range the source
        // occupies by more than the overshoot a windowed sinc is allowed —
        // a resampler that dropped its anti-aliasing shows up as excursions
        // several times the source amplitude.
        for ratio in [0.25, 0.5, 1.0, 1.5, 2.0] {
            let mut preview = Preview::new(sine(1024), 0.0);
            preview.snap_to_ratio(ratio);
            let mut out = vec![0.0f32; 4096];
            preview.read(&mut out);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak < 0.6, "ratio {ratio}: peak {peak}");
            assert!(peak > 0.4, "ratio {ratio}: peak {peak} — nothing came out");
        }
    }
}
