//! Measuring a file: where the tail starts, and which workflow it came from.

// The sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;
use crate::timing::Grid;

/// Default silence threshold, in dBFS. Below this the signal counts as decayed.
pub const DEFAULT_THRESHOLD_DBFS: f64 = -60.0;

/// Where a file's audible content ends and its FX decay begins.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Tail {
    /// Last frame at or above the threshold, plus one. Zero for silence.
    pub audible_end: usize,
    /// Frames from `audible_end` to the end of the buffer.
    pub trailing_frames: usize,
    pub threshold_dbfs: f64,
}

impl Tail {
    /// Measures where the signal last crosses `threshold_dbfs`.
    ///
    /// Scans backwards, since the answer is near the end: on a 42-second file
    /// the tail is a fraction of a second, so this touches a fraction of the
    /// samples.
    pub fn measure(buffer: &AudioBuffer, threshold_dbfs: f64) -> Self {
        let threshold = 10.0f64.powf(threshold_dbfs / 20.0);
        let mut audible_end = 0;
        for frame in (0..buffer.frames()).rev() {
            if buffer
                .channels()
                .iter()
                .any(|c| c[frame].abs() >= threshold)
            {
                audible_end = frame + 1;
                break;
            }
        }
        Tail {
            audible_end,
            trailing_frames: buffer.frames() - audible_end,
            threshold_dbfs,
        }
    }

    pub fn measure_default(buffer: &AudioBuffer) -> Self {
        Self::measure(buffer, DEFAULT_THRESHOLD_DBFS)
    }
}

/// Which of the two paths a file was rendered for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Workflow {
    /// About twice the loop plus a tail: bars N+1..2N are already the settled
    /// loop. Cut straight, discard the tail. `skip = bars`.
    WarmupRender,
    /// About one loop plus a tail: the tail has to be folded back onto the
    /// head to get the settled state. `skip = 0`.
    TailFoldback,
    /// Exactly the requested length with no tail at all — the file has already
    /// been through this. Nothing to cut, and nothing to fold back.
    AlreadyTrimmed,
    /// Neither shape fits — the caller has to decide.
    Unclear,
}

/// A workflow suggestion with the numbers behind it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct WorkflowGuess {
    pub workflow: Workflow,
    /// File length divided by the length of one loop.
    pub loops_in_file: f64,
    /// Bars the file spans, tail included.
    pub bars_in_file: f64,
    /// Bars of audible material, tail excluded.
    pub audible_bars: f64,
    pub loop_bars: u64,
    pub skip_bars: u64,
}

impl WorkflowGuess {
    /// Infers the workflow from how many loop lengths the file spans.
    ///
    /// `loop_bars` is what the user intends to keep. The file is measured
    /// against that, not the other way round: a 4-bar loop rendered twice and
    /// an 8-bar loop rendered once are the same duration but different jobs.
    pub fn detect(grid: &Grid, frames: usize, audible_frames: usize, loop_bars: u64) -> Self {
        let loop_frames = grid.exact_sample(loop_bars).to_f64();
        let bar_frames = grid.samples_per_bar().to_f64();

        let loops_in_file = if loop_frames > 0.0 {
            frames as f64 / loop_frames
        } else {
            0.0
        };
        let bars_in_file = if bar_frames > 0.0 {
            frames as f64 / bar_frames
        } else {
            0.0
        };
        let audible_bars = if bar_frames > 0.0 {
            audible_frames as f64 / bar_frames
        } else {
            0.0
        };

        // The audible part is what identifies the render, not the total: the
        // tail can be anything from a hair to several bars, as reverb decides.
        let audible_loops = if loop_frames > 0.0 {
            audible_frames as f64 / loop_frames
        } else {
            0.0
        };

        // The windows have to be wide, because `audible_frames` marks where
        // the reverb finally drops below the threshold, not where the last
        // trigger fired. On the 103 BPM reference file the music stops at bar
        // 16 but the decay stays above -60 dBFS until bar 17.57 -- 2.20 loop
        // lengths for an 8-bar loop. A narrow window would call that unclear
        // purely because the reverb was long.
        // A file that is exactly one loop long with no decay left is one this
        // tool already produced. Folding back an empty tail would be harmless
        // but saying so would be misleading.
        let workflow = if audible_frames >= frames && (loops_in_file - 1.0).abs() < 0.001 {
            Workflow::AlreadyTrimmed
        } else if (1.75..=2.6).contains(&audible_loops) {
            Workflow::WarmupRender
        } else if (0.85..=1.5).contains(&audible_loops) {
            Workflow::TailFoldback
        } else {
            Workflow::Unclear
        };

        WorkflowGuess {
            workflow,
            loops_in_file,
            bars_in_file,
            audible_bars,
            loop_bars,
            skip_bars: match workflow {
                Workflow::WarmupRender => loop_bars,
                Workflow::TailFoldback | Workflow::AlreadyTrimmed => 0,
                // Default to the safe path: a straight cut can be listened to
                // and redone, a wrong foldback quietly doubles the tails.
                Workflow::Unclear => loop_bars,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::{Tempo, TimeSignature};

    fn grid_103() -> Grid {
        Grid::new(
            Tempo::bpm(103).unwrap(),
            TimeSignature::FOUR_FOUR,
            44_100,
        )
    }

    /// A buffer of `frames` frames, loud up to `audible`, then silent.
    fn ramp(frames: usize, audible: usize) -> AudioBuffer {
        let mut ch = vec![0.0f64; frames];
        for (i, s) in ch.iter_mut().enumerate() {
            *s = if i < audible { 0.5 } else { 0.0 };
        }
        AudioBuffer::new(vec![ch], 44_100)
    }

    #[test]
    fn finds_where_the_signal_decays() {
        let t = Tail::measure_default(&ramp(1000, 600));
        assert_eq!(t.audible_end, 600);
        assert_eq!(t.trailing_frames, 400);
    }

    #[test]
    fn silence_has_no_audible_content() {
        let t = Tail::measure_default(&AudioBuffer::silence(2, 500, 44_100));
        assert_eq!(t.audible_end, 0);
        assert_eq!(t.trailing_frames, 500);
    }

    #[test]
    fn a_file_with_no_tail_reports_none() {
        let t = Tail::measure_default(&ramp(1000, 1000));
        assert_eq!(t.audible_end, 1000);
        assert_eq!(t.trailing_frames, 0);
    }

    #[test]
    fn the_threshold_is_respected() {
        // A signal at -40 dBFS: audible at -60, silent at -20.
        let quiet = 10.0f64.powf(-40.0 / 20.0);
        let buf = AudioBuffer::new(vec![vec![quiet; 100]], 44_100);
        assert_eq!(Tail::measure(&buf, -60.0).audible_end, 100);
        assert_eq!(Tail::measure(&buf, -20.0).audible_end, 0);
    }

    #[test]
    fn any_channel_counts_as_audible() {
        // Silent left, loud right — the frame is not silence.
        let buf = AudioBuffer::new(vec![vec![0.0; 10], vec![0.5; 10]], 44_100);
        assert_eq!(Tail::measure_default(&buf).audible_end, 10);
    }

    /// The measured numbers from `103 29Jul26 1Punkt1 Cstc.wav`.
    #[test]
    fn the_reference_file_is_recognised() {
        let g = grid_103();
        // 1875540 frames total, signal above -60 dBFS until frame 1805952.
        let guess = WorkflowGuess::detect(&g, 1_875_540, 1_805_952, 8);
        assert_eq!(guess.workflow, Workflow::WarmupRender);
        assert_eq!(guess.skip_bars, 8);
        assert!((guess.bars_in_file - 18.2521).abs() < 0.001, "{}", guess.bars_in_file);
        // 2.20 loop lengths of audible material -- the reverb decaying past
        // bar 16 is why this is not 2.00.
        assert!((guess.audible_bars / 8.0 - 2.197).abs() < 0.001);

        // The cut it leads to is the one the design docs specify.
        let region = g.region(guess.skip_bars, 8, crate::timing::Align::Loop);
        assert_eq!(region.start, 822_058);
        assert_eq!(region.end, 1_644_116);
        // And it sits comfortably inside the file.
        assert!(region.end < 1_875_540);
    }

    #[test]
    fn detects_a_warmup_render() {
        let g = grid_103();
        // 16 bars of material plus 2 bars 1 beat of tail — the reference case.
        let bar = g.samples_per_bar().to_f64();
        let audible = (16.0 * bar) as usize;
        let total = (18.25 * bar) as usize;

        let guess = WorkflowGuess::detect(&g, total, audible, 8);
        assert_eq!(guess.workflow, Workflow::WarmupRender);
        assert_eq!(guess.skip_bars, 8);
        assert!((guess.bars_in_file - 18.25).abs() < 0.01);
        assert!((guess.audible_bars - 16.0).abs() < 0.01);
    }

    #[test]
    fn detects_a_foldback_render() {
        let g = grid_103();
        let bar = g.samples_per_bar().to_f64();
        // 8 bars plus a 1-bar tail.
        let guess = WorkflowGuess::detect(&g, (9.0 * bar) as usize, (8.0 * bar) as usize, 8);
        assert_eq!(guess.workflow, Workflow::TailFoldback);
        assert_eq!(guess.skip_bars, 0);
    }

    #[test]
    fn odd_lengths_stay_unclear_and_default_to_the_safe_path() {
        let g = grid_103();
        let bar = g.samples_per_bar().to_f64();
        for loops in [0.5, 1.6, 3.0, 18.0] {
            let frames = (loops * 8.0 * bar) as usize;
            let guess = WorkflowGuess::detect(&g, frames, frames, 8);
            assert_eq!(
                guess.workflow,
                Workflow::Unclear,
                "{loops} loops should be unclear"
            );
            // Discard, not foldback: a wrong straight cut is audible and
            // reversible, a wrong foldback silently doubles the tails.
            assert_eq!(guess.skip_bars, 8);
        }
    }

    #[test]
    fn the_same_duration_can_be_either_job() {
        let g = grid_103();
        let bar = g.samples_per_bar().to_f64();
        // Eight audible bars plus a one-bar tail: two 4-bar loops rendered for
        // warmup, or one 8-bar loop wanting foldback.
        let (total, audible) = ((9.0 * bar) as usize, (8.0 * bar) as usize);
        assert_eq!(
            WorkflowGuess::detect(&g, total, audible, 4).workflow,
            Workflow::WarmupRender
        );
        assert_eq!(
            WorkflowGuess::detect(&g, total, audible, 8).workflow,
            Workflow::TailFoldback
        );
    }

    /// Feeding a finished loop back in must not propose more work on it.
    #[test]
    fn a_file_this_tool_already_cut_is_recognised() {
        let g = grid_103();
        // What the writer produced from the reference file: exactly 8 bars,
        // no decay past the end.
        let frames = g.exact_sample(8).round_half_up() as usize;
        assert_eq!(frames, 822_058);
        let guess = WorkflowGuess::detect(&g, frames, frames, 8);
        assert_eq!(guess.workflow, Workflow::AlreadyTrimmed);
        assert_eq!(guess.skip_bars, 0);

        // One bar of tail and it is a foldback candidate again.
        let with_tail = frames + g.samples_per_bar().round_half_up() as usize;
        assert_eq!(
            WorkflowGuess::detect(&g, with_tail, frames, 8).workflow,
            Workflow::TailFoldback
        );
    }
}
