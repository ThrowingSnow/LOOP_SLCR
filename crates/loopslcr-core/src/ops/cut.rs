//! Extracting the region the bar grid names.
//!
//! The whole exactness effort upstream exists to produce two integers; this is
//! where they are applied. The only thing that can go wrong here is silently
//! delivering fewer frames than the region asked for, so that is what the
//! result reports.

use crate::buffer::AudioBuffer;
use crate::timing::Region;

/// A cut region and what the source could actually supply.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    pub buffer: AudioBuffer,
    /// The region that was asked for, in source frames.
    pub region: Region,
    /// Frames the region asked for but the source did not hold.
    ///
    /// Nonzero means the loop is short, and a short loop drifts against a
    /// sequencer on every repeat. Never silently acceptable: the caller has to
    /// warn or refuse.
    pub short_by: u64,
}

impl Cut {
    pub fn frames(&self) -> usize {
        self.buffer.frames()
    }

    pub fn is_short(&self) -> bool {
        self.short_by > 0
    }
}

/// Extracts `region` from `source`.
///
/// The region is measured in whole samples and the buffer in frames, so this
/// is also where the grid's `u64` becomes a `usize` — the one place the width
/// of the machine's index type meets the timing arithmetic. On a 32-bit target
/// a region past 4 G frames cannot be indexed; that shows up as `short_by`
/// rather than as a panic.
pub fn cut(source: &AudioBuffer, region: Region) -> Cut {
    let available = source.frames() as u64;
    let start = region.start.min(available);
    let end = region.end.clamp(start, available);

    // Saturating rather than `as`: on a 32-bit target the truncation would
    // otherwise wrap to a small index and yield plausible-looking audio.
    let buffer = source.slice(
        usize::try_from(start).unwrap_or(usize::MAX),
        usize::try_from(end).unwrap_or(usize::MAX),
    );

    Cut {
        short_by: region.len() - (end - start),
        buffer,
        region,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::{Align, Grid, Tempo, TimeSignature};

    fn ramp(frames: usize) -> AudioBuffer {
        AudioBuffer::new(
            vec![
                (0..frames).map(|i| i as f64).collect(),
                // Negated as an integer: this module has no business doing
                // float arithmetic, not even in its tests.
                (0..frames).map(|i| -(i as i64) as f64).collect(),
            ],
            44_100,
        )
    }

    #[test]
    fn takes_exactly_the_region() {
        let c = cut(&ramp(1000), Region { start: 100, end: 400 });
        assert_eq!(c.frames(), 300);
        assert!(!c.is_short());
        assert_eq!(c.buffer.channel(0)[0], 100.0);
        assert_eq!(c.buffer.channel(0)[299], 399.0);
        // Channels stay paired — a swap would show up as a sign flip.
        assert_eq!(c.buffer.channel(1)[0], -100.0);
    }

    #[test]
    fn a_short_source_is_reported_not_hidden() {
        // The region wants 300 frames; the file ends after 150 of them.
        let c = cut(&ramp(250), Region { start: 100, end: 400 });
        assert_eq!(c.frames(), 150);
        assert!(c.is_short());
        assert_eq!(c.short_by, 150);
        // The region asked for is preserved, so the caller can say by how much.
        assert_eq!(c.region.len(), 300);
    }

    #[test]
    fn a_region_entirely_past_the_end_yields_nothing() {
        let c = cut(&ramp(100), Region { start: 500, end: 800 });
        assert_eq!(c.frames(), 0);
        assert_eq!(c.short_by, 300);
    }

    #[test]
    fn an_empty_region_is_short_by_nothing() {
        let c = cut(&ramp(100), Region { start: 50, end: 50 });
        assert_eq!(c.frames(), 0);
        assert!(!c.is_short());
    }

    /// The reference case, end to end from the grid.
    #[test]
    fn the_reference_region_comes_out_at_the_documented_length() {
        let grid = Grid::new(Tempo::bpm(103).unwrap(), TimeSignature::FOUR_FOUR, 44_100);
        let region = grid.region(8, 8, Align::Loop);
        let c = cut(&AudioBuffer::silence(2, 1_875_540, 44_100), region);
        assert_eq!((region.start, region.end), (822_058, 1_644_116));
        assert_eq!(c.frames(), 822_058);
        assert!(!c.is_short());
    }

    /// The length is what has to be exact, so `Align::Loop` must survive the
    /// trip through the buffer unchanged.
    #[test]
    fn cutting_does_not_re_round_the_length() {
        let grid = Grid::new(Tempo::bpm(103).unwrap(), TimeSignature::FOUR_FOUR, 44_100);
        // Long enough to hold the furthest region asked for below: bar 33 plus
        // 8 bars is 4 213 048 frames.
        let source = AudioBuffer::silence(1, 4_300_000, 44_100);
        for skip in [0u64, 1, 7, 8, 33] {
            let region = grid.region(skip, 8, Align::Loop);
            let c = cut(&source, region);
            assert_eq!(
                c.frames() as u64,
                grid.exact_sample(8).round_half_up() as u64,
                "skip {skip}: length changed"
            );
        }
    }
}
