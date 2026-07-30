//! Min/max buckets for drawing a waveform.
//!
//! A waveform display is not a downsample. Averaging or picking every nth sample
//! makes a transient vanish at some zoom levels and reappear at others, which is
//! precisely wrong for a tool whose whole job is deciding *where* to cut. Each
//! bucket therefore keeps the extremes of the samples it covers, so a single-
//! sample spike is visible at every zoom level — the display can lose detail but
//! never a peak.

// Measuring is the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;

/// The extremes of one bucket of samples.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bucket {
    pub min: f64,
    pub max: f64,
}

impl Bucket {
    /// An empty bucket, reading as silence rather than as ±infinity.
    pub const SILENT: Bucket = Bucket { min: 0.0, max: 0.0 };

    /// Largest excursion either way — what a level meter would show.
    pub fn magnitude(&self) -> f64 {
        self.min.abs().max(self.max.abs())
    }
}

/// One channel's worth of buckets, and the resolution they were taken at.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    /// Buckets per channel, outer index the channel.
    pub channels: Vec<Vec<Bucket>>,
    /// Frames each bucket covers, except possibly the last.
    pub frames_per_bucket: usize,
    pub frames: usize,
}

impl Peaks {
    /// Reduces `buffer` to at most `buckets` buckets per channel.
    ///
    /// Fewer buckets than asked for when the buffer is shorter than the request:
    /// there is no sense in a bucket covering a fraction of a sample, and
    /// stretching the data to fill a width is the display's job, not this one's.
    pub fn measure(buffer: &AudioBuffer, buckets: usize) -> Self {
        let frames = buffer.frames();
        if buckets == 0 || frames == 0 {
            return Peaks {
                channels: vec![Vec::new(); buffer.channel_count()],
                frames_per_bucket: 0,
                frames,
            };
        }

        // Round up, so the buckets cover the whole buffer. Rounding down would
        // leave a remainder longer than one bucket unexamined — the tail, which
        // is the part this tool cares most about.
        let frames_per_bucket = frames.div_ceil(buckets);

        let channels = buffer
            .channels()
            .iter()
            .map(|samples| {
                samples
                    .chunks(frames_per_bucket)
                    .map(|chunk| Bucket {
                        min: chunk.iter().copied().fold(f64::INFINITY, f64::min),
                        max: chunk.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    })
                    .collect()
            })
            .collect();

        Peaks {
            channels,
            frames_per_bucket,
            frames,
        }
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Buckets actually produced, which can be fewer than requested.
    pub fn len(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The frame range bucket `i` covers, clamped to the buffer.
    pub fn range(&self, i: usize) -> std::ops::Range<usize> {
        let start = (i * self.frames_per_bucket).min(self.frames);
        let end = (start + self.frames_per_bucket).min(self.frames);
        start..end
    }

    /// The loudest bucket across all channels, as `(index, magnitude)`.
    ///
    /// The *first* on a tie, matching [`gain::Peak`](crate::ops::gain::Peak): on
    /// a display the earliest transient is the one worth pointing at, and
    /// `max_by` would have returned the last, which on silence is an arbitrary
    /// index that looks like a finding.
    pub fn loudest(&self) -> Option<(usize, f64)> {
        (0..self.len())
            .map(|i| {
                let magnitude = self
                    .channels
                    .iter()
                    .map(|c| c[i].magnitude())
                    .fold(0.0f64, f64::max);
                (i, magnitude)
            })
            .fold(None, |best: Option<(usize, f64)>, next| match best {
                Some((_, m)) if m >= next.1 => best,
                _ => Some(next),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize) -> AudioBuffer {
        AudioBuffer::new(
            vec![
                (0..frames).map(|i| i as f64 / frames as f64).collect(),
                (0..frames).map(|i| -(i as i64) as f64 / frames as f64).collect(),
            ],
            44_100,
        )
    }

    #[test]
    fn buckets_cover_the_whole_buffer() {
        let p = Peaks::measure(&ramp(1000), 10);
        assert_eq!(p.len(), 10);
        assert_eq!(p.frames_per_bucket, 100);
        assert_eq!(p.range(0), 0..100);
        assert_eq!(p.range(9), 900..1000);
        // Nothing past the end.
        assert_eq!(p.range(10), 1000..1000);
    }

    #[test]
    fn a_length_that_does_not_divide_evenly_still_covers_everything() {
        // 1000 frames into 3 buckets: 334 each, the last one short. Rounding
        // down to 333 would leave frame 999 unlooked at.
        let p = Peaks::measure(&ramp(1000), 3);
        assert_eq!(p.frames_per_bucket, 334);
        assert_eq!(p.len(), 3);
        assert_eq!(p.range(2), 668..1000);

        let covered: usize = (0..p.len()).map(|i| p.range(i).len()).sum();
        assert_eq!(covered, 1000, "buckets do not cover the buffer");
    }

    #[test]
    fn a_single_sample_spike_survives_every_zoom_level() {
        // The reason for min/max rather than averaging or decimating. One frame
        // at full scale in a quiet buffer must be visible at any width.
        let mut b = AudioBuffer::new(vec![vec![0.01; 100_000]], 44_100);
        b.channel_mut(0)[54_321] = 1.0;

        for buckets in [1, 7, 100, 1000, 50_000] {
            let p = Peaks::measure(&b, buckets);
            let (_, magnitude) = p.loudest().unwrap();
            assert_eq!(magnitude, 1.0, "spike lost at {buckets} buckets");
        }
    }

    #[test]
    fn min_and_max_are_both_kept() {
        // A bucket spanning a full cycle has to report both extremes, or the
        // waveform would be drawn as a one-sided shape.
        let b = AudioBuffer::new(vec![vec![0.5, -0.75, 0.25, -0.1]], 44_100);
        let p = Peaks::measure(&b, 1);
        assert_eq!(p.channels[0][0], Bucket { min: -0.75, max: 0.5 });
        assert_eq!(p.channels[0][0].magnitude(), 0.75);
    }

    #[test]
    fn channels_stay_separate() {
        let p = Peaks::measure(&ramp(100), 4);
        assert_eq!(p.channel_count(), 2);
        // The second channel is the negated first, so its extremes mirror.
        for i in 0..p.len() {
            assert_eq!(p.channels[0][i].max, -p.channels[1][i].min);
            assert_eq!(p.channels[0][i].min, -p.channels[1][i].max);
        }
    }

    #[test]
    fn asking_for_more_buckets_than_frames_gives_one_per_frame() {
        let p = Peaks::measure(&ramp(10), 1000);
        assert_eq!(p.frames_per_bucket, 1);
        assert_eq!(p.len(), 10);
        // Each bucket is then a single sample, min equal to max.
        for bucket in &p.channels[0] {
            assert_eq!(bucket.min, bucket.max);
        }
    }

    #[test]
    fn an_empty_buffer_or_zero_buckets_yields_nothing() {
        let empty = Peaks::measure(&AudioBuffer::silence(2, 0, 44_100), 100);
        assert!(empty.is_empty());
        assert_eq!(empty.channel_count(), 2);
        assert_eq!(empty.loudest(), None);

        let none = Peaks::measure(&ramp(100), 0);
        assert!(none.is_empty());
        // And no division by zero in `range`.
        assert_eq!(none.range(0), 0..0);
    }

    #[test]
    fn silence_reads_as_zero_not_infinity() {
        // `fold` starting from ±infinity has to be overwritten by real samples;
        // an empty chunk must never leak the initial value into the display.
        let p = Peaks::measure(&AudioBuffer::silence(1, 50, 44_100), 5);
        for bucket in &p.channels[0] {
            assert_eq!(*bucket, Bucket::SILENT);
        }
        // Every bucket is equally loud, so the first is reported rather than an
        // arbitrary one that would read as a finding.
        assert_eq!(p.loudest(), Some((0, 0.0)));
    }

    #[test]
    fn the_loudest_bucket_is_where_the_transient_is() {
        let mut b = AudioBuffer::new(vec![vec![0.0; 1000]], 44_100);
        b.channel_mut(0)[650] = -0.9;
        let p = Peaks::measure(&b, 10);
        let (index, magnitude) = p.loudest().unwrap();
        assert_eq!(index, 6);
        assert_eq!(magnitude, 0.9);
        assert!(p.range(index).contains(&650));
    }
}
