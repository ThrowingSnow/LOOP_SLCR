//! Micro-fades at the loop boundaries.
//!
//! A fade is a safety net, not the mechanism. A cut on the grid from a settled
//! region already meets itself at the seam, and a foldback loop is seamless by
//! construction. What is left is the case where the waveform happens to be
//! mid-cycle at the cut point: the jump from the last sample to the first is a
//! step, and a step is a click.
//!
//! **The honest cost:** fading in at the start and out at the end drives *both*
//! sides of the seam to zero, so the click is replaced by a short dip in level —
//! a hole twice the fade length, once per repeat. At 0.5 ms that is inaudible on
//! percussive material and faintly audible as a soft pulse on sustained pads.
//! The alternative that has no dip is a circular crossfade using material from
//! before the loop start, which changes the loop's head; that is a different
//! operation and belongs with a proper decision about which material wins.
//! Hence: short by default, and off is a legitimate choice.

// Gain staging is the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;

/// The curve a fade follows.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum FadeShape {
    /// A straight ramp. Its slope jumps at both ends of the fade, which at very
    /// short lengths is itself a faint discontinuity — the thing being avoided.
    Linear,
    /// Raised cosine. Reaches its endpoints with zero slope, so the signal and
    /// its first derivative are both continuous. The default for that reason.
    #[default]
    Cosine,
}

impl FadeShape {
    /// Maps a linear position `t` in `[0, 1]` to a gain in `[0, 1]`.
    fn gain(self, t: f64) -> f64 {
        match self {
            FadeShape::Linear => t,
            FadeShape::Cosine => (1.0 - (std::f64::consts::PI * t).cos()) / 2.0,
        }
    }
}

/// A fade of a given length.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fade {
    pub frames: usize,
    pub shape: FadeShape,
}

impl Fade {
    /// The default micro-fade length: 0.5 ms.
    ///
    /// Long enough to remove a step at any audible frequency, short enough that
    /// the dip it leaves at the seam is below notice on percussive material.
    pub const DEFAULT_MICRO_MILLIS: f64 = 0.5;

    pub fn new(frames: usize, shape: FadeShape) -> Self {
        Fade { frames, shape }
    }

    /// A fade of `millis` at `sample_rate`, rounded to whole frames.
    ///
    /// Rounds up, so a length short enough to round to zero still fades: asking
    /// for a fade and silently getting none would be the wrong surprise.
    pub fn from_millis(millis: f64, sample_rate: u32) -> Self {
        let frames = (millis * sample_rate as f64 / 1000.0).ceil();
        Fade {
            frames: if frames > 0.0 { frames as usize } else { 0 },
            shape: FadeShape::default(),
        }
    }

    /// The default 0.5 ms micro-fade.
    pub fn micro(sample_rate: u32) -> Self {
        Self::from_millis(Self::DEFAULT_MICRO_MILLIS, sample_rate)
    }

    pub fn is_none(&self) -> bool {
        self.frames == 0
    }

    /// Fades in over the first `frames` frames.
    ///
    /// The first sample lands at exactly zero and the frame just past the fade
    /// is untouched, so the ramp is continuous at both ends.
    pub fn apply_in(&self, buffer: &mut AudioBuffer) {
        let n = self.frames.min(buffer.frames());
        if n == 0 {
            return;
        }
        for channel in buffer.channels_mut() {
            for (i, s) in channel[..n].iter_mut().enumerate() {
                *s *= self.shape.gain(i as f64 / n as f64);
            }
        }
    }

    /// Fades out over the last `frames` frames, ending at exactly zero.
    pub fn apply_out(&self, buffer: &mut AudioBuffer) {
        let n = self.frames.min(buffer.frames());
        if n == 0 {
            return;
        }
        let start = buffer.frames() - n;
        for channel in buffer.channels_mut() {
            for (j, s) in channel[start..].iter_mut().enumerate() {
                *s *= self.shape.gain(1.0 - (j + 1) as f64 / n as f64);
            }
        }
    }

    /// Fades both ends — the loop-boundary case.
    ///
    /// Each fade is capped at half the buffer so the two cannot overlap and
    /// multiply, which would leave a buffer quieter in the middle than at its
    /// ends and no longer resemble the source at all.
    pub fn apply_both(&self, buffer: &mut AudioBuffer) {
        let capped = Fade {
            frames: self.frames.min(buffer.frames() / 2),
            shape: self.shape,
        };
        capped.apply_in(buffer);
        capped.apply_out(buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ones(frames: usize) -> AudioBuffer {
        AudioBuffer::new(vec![vec![1.0; frames], vec![1.0; frames]], 44_100)
    }

    #[test]
    fn a_fade_in_starts_at_silence_and_reaches_unity() {
        let mut b = ones(100);
        Fade::new(10, FadeShape::Linear).apply_in(&mut b);
        assert_eq!(b.channel(0)[0], 0.0);
        assert_eq!(b.channel(0)[5], 0.5);
        // The frame past the fade is untouched, so the ramp is continuous.
        assert_eq!(b.channel(0)[10], 1.0);
        assert_eq!(b.channel(0)[99], 1.0);
        // Both channels, identically.
        assert_eq!(b.channel(1)[0], 0.0);
        assert_eq!(b.channel(1)[5], 0.5);
    }

    #[test]
    fn a_fade_out_ends_at_silence() {
        let mut b = ones(100);
        Fade::new(10, FadeShape::Linear).apply_out(&mut b);
        assert_eq!(b.channel(0)[89], 1.0); // untouched
        assert_eq!(b.channel(0)[90], 0.9);
        assert_eq!(b.channel(0)[99], 0.0);
    }

    #[test]
    fn the_two_fades_are_mirror_images() {
        let n = 32;
        for shape in [FadeShape::Linear, FadeShape::Cosine] {
            let mut a = ones(200);
            let mut b = ones(200);
            Fade::new(n, shape).apply_in(&mut a);
            Fade::new(n, shape).apply_out(&mut b);
            for i in 0..n {
                assert_eq!(
                    a.channel(0)[i],
                    b.channel(0)[199 - i],
                    "{shape:?} is asymmetric at {i}"
                );
            }
        }
    }

    #[test]
    fn the_cosine_shape_is_flat_at_both_ends() {
        let n = 64;
        let mut b = ones(n);
        Fade::new(n, FadeShape::Cosine).apply_in(&mut b);
        let c = b.channel(0);

        assert_eq!(c[0], 0.0);
        // Zero slope at the start: the first step is far smaller than the
        // linear ramp's constant 1/n. That is the whole point of the shape.
        assert!(c[1] < 1.0 / n as f64, "not flat at the start: {}", c[1]);
        // Monotonic throughout — a fade must never go back up.
        for i in 1..n {
            assert!(c[i] > c[i - 1], "not monotonic at {i}");
        }
        // Halfway is exactly half gain.
        assert!((c[n / 2] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn both_ends_cannot_overlap_and_multiply() {
        // A 90-frame fade on a 100-frame buffer: capped to 50 each, so the two
        // ramps meet in the middle instead of multiplying across it.
        let mut b = ones(100);
        Fade::new(90, FadeShape::Linear).apply_both(&mut b);
        assert_eq!(b.channel(0)[0], 0.0);
        assert_eq!(b.channel(0)[99], 0.0);
        // The ramps meet at 1 - 1/50 and are symmetric about the join. Had they
        // overlapped, the middle would be the *product* of two part-way gains —
        // around a quarter, not just under unity.
        assert_eq!(b.channel(0)[49], 0.98);
        assert_eq!(b.channel(0)[50], 0.98);
        assert_eq!(b.peak(), 0.98);

        // A fade that fits leaves a genuinely untouched middle.
        let mut fits = ones(100);
        Fade::new(10, FadeShape::Linear).apply_both(&mut fits);
        assert_eq!(fits.peak(), 1.0);
        assert_eq!(fits.channel(0)[50], 1.0);
    }

    #[test]
    fn a_fade_longer_than_the_buffer_is_clamped() {
        let mut b = ones(5);
        Fade::new(1000, FadeShape::Linear).apply_in(&mut b);
        assert_eq!(b.channel(0)[0], 0.0);
        assert_eq!(b.channel(0)[4], 0.8);
    }

    #[test]
    fn a_zero_length_fade_changes_nothing() {
        let source = ones(50);
        let mut b = source.clone();
        let fade = Fade::new(0, FadeShape::Cosine);
        assert!(fade.is_none());
        fade.apply_in(&mut b);
        fade.apply_out(&mut b);
        fade.apply_both(&mut b);
        assert_eq!(b, source);

        // An empty buffer is not a special case either.
        let mut empty = AudioBuffer::silence(1, 0, 44_100);
        Fade::micro(44_100).apply_both(&mut empty);
        assert_eq!(empty.frames(), 0);
    }

    #[test]
    fn the_default_micro_fade_is_half_a_millisecond() {
        // 0.5 ms at 44.1 kHz is 22.05 frames — rounded up, so the fade is at
        // least as long as asked for.
        assert_eq!(Fade::micro(44_100).frames, 23);
        assert_eq!(Fade::micro(48_000).frames, 24);
        assert_eq!(Fade::micro(96_000).frames, 48);
        assert_eq!(Fade::micro(44_100).shape, FadeShape::Cosine);

        // A length that would round down to nothing still fades one frame.
        assert_eq!(Fade::from_millis(0.001, 44_100).frames, 1);
        assert_eq!(Fade::from_millis(0.0, 44_100).frames, 0);
        assert_eq!(Fade::from_millis(-1.0, 44_100).frames, 0);
    }

    /// What the fade exists for: a step at the seam becomes a ramp.
    #[test]
    fn a_step_at_the_seam_is_removed() {
        // A DC-offset buffer is the worst case: every seam is a full-scale step.
        let mut b = AudioBuffer::new(vec![vec![1.0; 1000]], 44_100);
        Fade::micro(44_100).apply_both(&mut b);
        // Wrapping from the last frame to the first is now continuous, where
        // before it jumped from 1.0 to 1.0 against a silent neighbourhood.
        assert_eq!(b.channel(0)[0], 0.0);
        assert_eq!(b.channel(0)[999], 0.0);
        // The largest sample-to-sample step inside the loop, seam included.
        let c = b.channel(0);
        let seam = (c[0] - c[999]).abs();
        let biggest = c.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max);
        assert_eq!(seam, 0.0);
        assert!(biggest < 0.1, "fade itself steps by {biggest}");
    }
}
