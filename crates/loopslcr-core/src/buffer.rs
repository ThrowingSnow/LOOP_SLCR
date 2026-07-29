//! The audio buffer everything operates on.

// The sample domain. Exactness lives in `timing`; here f64 is the point.
#![allow(clippy::float_arithmetic)]

/// Planar, `f64` internally. Bit depth is an I/O concern and does not appear here.
///
/// f64 rather than f32 because the chain is short and offline, and it removes
/// any question about accumulation in foldback summation and resampling.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioBuffer {
    channels: Vec<Vec<f64>>,
    sample_rate: u32,
}

impl AudioBuffer {
    /// # Panics
    /// If `channels` is empty, the channels differ in length, or `sample_rate`
    /// is zero.
    pub fn new(channels: Vec<Vec<f64>>, sample_rate: u32) -> Self {
        assert!(!channels.is_empty(), "audio buffer with no channels");
        assert!(sample_rate > 0, "sample rate must be positive");
        let len = channels[0].len();
        assert!(
            channels.iter().all(|c| c.len() == len),
            "channels differ in length"
        );
        AudioBuffer {
            channels,
            sample_rate,
        }
    }

    pub fn silence(channel_count: usize, frames: usize, sample_rate: u32) -> Self {
        AudioBuffer::new(vec![vec![0.0; frames]; channel_count], sample_rate)
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Frames, not samples: a stereo buffer of 100 frames holds 200 samples.
    pub fn frames(&self) -> usize {
        self.channels[0].len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames() == 0
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channel(&self, index: usize) -> &[f64] {
        &self.channels[index]
    }

    pub fn channel_mut(&mut self, index: usize) -> &mut [f64] {
        &mut self.channels[index]
    }

    pub fn channels(&self) -> &[Vec<f64>] {
        &self.channels
    }

    pub fn channels_mut(&mut self) -> &mut [Vec<f64>] {
        &mut self.channels
    }

    pub fn into_channels(self) -> Vec<Vec<f64>> {
        self.channels
    }

    /// Duration in seconds. Display only — the bar grid never asks this.
    pub fn duration_seconds(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    /// Largest absolute sample value across all channels.
    pub fn peak(&self) -> f64 {
        self.channels
            .iter()
            .flat_map(|c| c.iter())
            .fold(0.0f64, |m, s| m.max(s.abs()))
    }

    /// Half-open frame range `[start, end)`, clamped to the buffer.
    ///
    /// Clamping rather than erroring: a cut region derived from the bar grid
    /// can legitimately run past the end of a file that was rendered a hair
    /// short, and the caller compares the returned length against what it
    /// asked for.
    pub fn slice(&self, start: usize, end: usize) -> AudioBuffer {
        let start = start.min(self.frames());
        let end = end.clamp(start, self.frames());
        AudioBuffer {
            channels: self.channels.iter().map(|c| c[start..end].to_vec()).collect(),
            sample_rate: self.sample_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> AudioBuffer {
        AudioBuffer::new(vec![vec![0.0, 0.5, -1.0, 0.25], vec![0.0, -0.75, 0.5, 0.0]], 44_100)
    }

    #[test]
    fn basic_shape() {
        let b = buf();
        assert_eq!(b.channel_count(), 2);
        assert_eq!(b.frames(), 4);
        assert_eq!(b.sample_rate(), 44_100);
        assert!(!b.is_empty());
        assert_eq!(b.channel(1)[1], -0.75);
    }

    #[test]
    fn peak_spans_all_channels() {
        assert_eq!(buf().peak(), 1.0);
        assert_eq!(AudioBuffer::silence(2, 10, 44_100).peak(), 0.0);
    }

    #[test]
    fn slicing_is_half_open_and_clamped() {
        let b = buf();
        let s = b.slice(1, 3);
        assert_eq!(s.frames(), 2);
        assert_eq!(s.channel(0), &[0.5, -1.0]);

        // Past the end clamps rather than panicking.
        assert_eq!(b.slice(2, 99).frames(), 2);
        assert_eq!(b.slice(99, 200).frames(), 0);
        // An inverted range yields nothing rather than panicking.
        assert_eq!(b.slice(3, 1).frames(), 0);
    }

    #[test]
    fn silence_is_silent() {
        let s = AudioBuffer::silence(1, 5, 48_000);
        assert_eq!(s.channel_count(), 1);
        assert_eq!(s.frames(), 5);
        assert!(s.channel(0).iter().all(|&x| x == 0.0));
    }

    #[test]
    #[should_panic(expected = "channels differ in length")]
    fn ragged_channels_panic() {
        AudioBuffer::new(vec![vec![0.0; 3], vec![0.0; 4]], 44_100);
    }

    #[test]
    #[should_panic(expected = "no channels")]
    fn zero_channels_panic() {
        AudioBuffer::new(vec![], 44_100);
    }
}
