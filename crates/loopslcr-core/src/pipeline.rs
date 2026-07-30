//! The whole cut, from a parsed file to a finished buffer.
//!
//! # Why this is in the core rather than in the CLI
//!
//! It used to live in `run_cut`, interleaved with the lines it printed. That was
//! fine while there was one front end. It stopped being fine the moment a second
//! one appeared: an Android app that reimplemented this would be a second set of
//! answers to *which tempo, which loop length, which shape, fade or not* — and
//! the two would disagree the first time either was touched.
//!
//! So the pipeline decides, and the caller describes. Everything decided along
//! the way is recorded in [`Outcome`], which is why that struct is as wide as it
//! is: it is the report, in data rather than in text.
//!
//! # What this deliberately does not do
//!
//! It does not read or write files, and it does not refuse a short loop. A
//! source that cannot fill the loop is *recorded* in [`Outcome::short_by`] and
//! the caller decides what that means — the CLI refuses it on a real run and
//! reports it on a dry one, and a UI would want to grey out a button rather than
//! raise an error. Deciding here would make one of those impossible.
//!
//! # Order
//!
//! Binding, and set out in [`crate::ops`]: cut and foldback in the original time
//! domain where the bar grid is exact, then fade, then varispeed, then tape
//! character, then normalize, then dither. Nothing may move after the dither —
//! shaped dither leaves the samples on the output grid, and anything touching
//! them afterwards would need dithering all over again.

// Reporting levels and durations is the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::analysis::{self, Tail, Workflow, WorkflowGuess};
use crate::buffer::AudioBuffer;
use crate::error::{Error, Result};
use crate::naming;
use crate::ops::dither::{Applied, Dither};
use crate::ops::resample::Resampler as _;
use crate::ops::{cut, dither, fade, foldback, gain, resample, tape};
use crate::timing::{Align, BpmUnit, Grid, Ratio, Region, Tempo, TimeSignature};
use crate::wav::{BitDepth, Wav};

/// When to dither, as a policy rather than as a mode.
///
/// Distinct from [`Dither`] because `Auto` is a decision that needs the source's
/// bit depth, which the caller does not have to know about.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DitherPolicy {
    /// Flat TPDF, but only when the bit depth actually drops.
    #[default]
    Auto,
    /// Flat TPDF, always, for integer output.
    Flat,
    /// TPDF plus second-order noise shaping.
    Shaped,
    Off,
}

impl DitherPolicy {
    /// The mode this policy resolves to for a given source depth.
    pub fn resolve(self, source_bits: u16, target: BitDepth) -> Dither {
        match self {
            DitherPolicy::Off => Dither::None,
            DitherPolicy::Flat => Dither::Tpdf,
            DitherPolicy::Shaped => Dither::Shaped,
            DitherPolicy::Auto => {
                if dither::is_called_for(source_bits, target) {
                    Dither::Tpdf
                } else {
                    Dither::None
                }
            }
        }
    }
}

/// Everything the pipeline needs, with `None` meaning "work it out".
#[derive(Clone, Debug)]
pub struct Params {
    /// Overrides every other tempo source.
    pub bpm: Option<Tempo>,
    /// Read the tempo from the filename in preference to the `acid` chunk.
    pub prefer_name_tempo: bool,
    /// Overrides every other loop length.
    pub bars: Option<u64>,
    /// Prefer a loop length stated in the filename over the file's duration.
    pub prefer_name_bars: bool,
    /// Warmup bars to skip. Overrides what the shape implies.
    pub skip: Option<u64>,
    pub sig: TimeSignature,
    pub bpm_unit: BpmUnit,
    pub align: Align,
    /// Forces a workflow. `None` uses the detected one.
    pub workflow: Option<Workflow>,
    /// An explicit varispeed ratio, from a pitch interval.
    pub ratio: Option<Ratio>,
    /// A varispeed target tempo. Ignored when `ratio` is set.
    pub target_bpm: Option<Tempo>,
    pub snap: bool,
    pub snap_window: u32,
    pub depth: BitDepth,
    pub normalize: bool,
    pub dither: DitherPolicy,
    pub dither_seed: u64,
    pub tape: tape::TapeParams,
    /// Micro-fade length in milliseconds. `None` picks by path.
    pub fade_ms: Option<f64>,
    /// Never fade, whichever path is taken.
    pub no_fade: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            bpm: None,
            prefer_name_tempo: false,
            bars: None,
            prefer_name_bars: false,
            skip: None,
            sig: TimeSignature::default(),
            bpm_unit: BpmUnit::default(),
            align: Align::default(),
            workflow: None,
            ratio: None,
            target_bpm: None,
            snap: false,
            snap_window: 15,
            depth: BitDepth::default(),
            normalize: false,
            dither: DitherPolicy::default(),
            dither_seed: dither::DEFAULT_SEED,
            tape: tape::TapeParams::default(),
            fade_ms: None,
            no_fade: false,
        }
    }
}

/// Where the tempo came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TempoSource {
    Given,
    AcidChunk,
    Filename,
}

impl TempoSource {
    pub fn label(self) -> &'static str {
        match self {
            TempoSource::Given => "given",
            TempoSource::AcidChunk => "from acid chunk",
            TempoSource::Filename => "from filename",
        }
    }
}

/// Where the loop length came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BarsSource {
    Given,
    Filename,
    FileLength,
}

impl BarsSource {
    pub fn label(self) -> &'static str {
        match self {
            BarsSource::Given => "",
            BarsSource::Filename => " (from filename)",
            BarsSource::FileLength => " (from file length)",
        }
    }
}

/// Which of the two paths was taken, and what it did.
#[derive(Clone, Debug)]
pub enum Taken {
    /// A straight cut; the tail was discarded.
    StraightCut,
    /// The tail was folded back onto the loop.
    Foldback(foldback::Foldback),
    /// A foldback was asked for but the source was too short to have a tail.
    TooShortToFold,
}

/// Everything the pipeline decided and did.
///
/// Wide on purpose: this is the report as data. A caller renders whichever parts
/// it has room for, and nothing has to be recomputed or scraped back out of text.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub buffer: AudioBuffer,

    pub source_frames: usize,
    pub sample_rate: u32,
    pub source_bits: u16,

    pub tempo: Tempo,
    pub tempo_source: TempoSource,
    pub grid: Grid,

    pub bars: u64,
    pub bars_source: BarsSource,

    pub tail: Tail,
    pub shape: WorkflowGuess,
    /// The shape read off the file.
    pub detected: Workflow,
    /// The shape acted on, which differs when [`Params::workflow`] forced one.
    pub chosen: Workflow,
    pub skip_bars: u64,
    pub region: Region,
    /// Frames the loop needs.
    pub loop_frames: usize,
    /// Frames the file actually offers from the cut-in point.
    pub available: usize,
    /// How far short the source fell. Zero on a source long enough.
    pub short_by: usize,

    pub taken: Taken,
    pub fade: fade::Fade,

    pub ratio: Ratio,
    pub final_tempo: Tempo,

    pub tape: tape::Tape,
    /// The gain applied and the peak it was applied to, when normalising did
    /// anything.
    pub normalized: Option<(f64, f64)>,
    pub dither: Option<(Dither, Applied)>,
    pub peak: gain::Peak,
}

impl Outcome {
    /// Whether the result is a whole number of bars, or drifts.
    pub fn drifts(&self) -> bool {
        self.buffer.frames() < self.loop_frames
    }

    /// Frames missing from a whole loop, and the drift per repeat in seconds.
    pub fn drift(&self) -> Option<(usize, f64)> {
        self.drifts().then(|| {
            let missing = self.loop_frames - self.buffer.frames();
            (missing, missing as f64 / self.sample_rate as f64)
        })
    }

    /// Beats in the finished loop, for the `acid` chunk.
    pub fn beats(&self) -> u64 {
        self.bars * self.sig().num as u64
    }

    pub fn sig(&self) -> TimeSignature {
        self.grid.sig
    }
}

/// Runs the whole pipeline over an already-parsed file.
///
/// `name` is the filename, used only as a tempo and bar-count source — nothing
/// here touches the filesystem.
///
/// # Errors
/// When no tempo can be found, when no loop length can be worked out, or when
/// any stage refuses. A source too short to fill the loop is **not** an error;
/// see [`Outcome::short_by`].
pub fn run(wav: &Wav, name: &str, params: &Params) -> Result<Outcome> {
    let format = wav.format();
    let (tempo, tempo_source) = resolve_tempo(wav, name, params)?;
    let grid = Grid::new(tempo.with_unit(params.bpm_unit), params.sig, format.sample_rate);

    // Loop length: the parameter wins, then the filename if asked for, then the
    // file's own duration. No fixed default — 8 bars is right for the reference
    // render and wrong for four fifths of the archive.
    let from_length = analysis::guess_loop_bars(&grid, wav.frames());
    let from_name = params
        .prefer_name_bars
        .then(|| naming::bars_from_name(name))
        .flatten();
    let (bars, bars_source) = match (params.bars, from_name, from_length) {
        (Some(b), _, _) => (b, BarsSource::Given),
        (None, Some(b), _) => (b, BarsSource::Filename),
        (None, None, Some(g)) => (g.bars, BarsSource::FileLength),
        (None, None, None) => return Err(Error::NoLoopLength),
    };

    let source = wav.decode()?;
    let tail = Tail::measure_default(&source);
    let shape = WorkflowGuess::detect(&grid, source.frames(), tail.audible_end, bars);

    // When the loop length came from the file's own duration, the shape comes
    // from the same reading. Asking `WorkflowGuess::detect` again would let the
    // two disagree — and they do: a file read as one 4-bar loop plus a tail comes
    // back `Unclear` from `detect`, whose skip of one whole loop then puts the
    // region past the end of a file that was only ever one loop long.
    let detected = match (bars_source, from_length) {
        (BarsSource::FileLength, Some(g)) => g.workflow,
        _ => shape.workflow,
    };
    let chosen = params.workflow.unwrap_or(detected);

    let fold = chosen == Workflow::TailFoldback;
    let skip_bars = params.skip.unwrap_or(match chosen {
        Workflow::TailFoldback | Workflow::AlreadyTrimmed => 0,
        // Unclear falls back to a straight cut one loop in: audible and
        // reversible, where a wrong foldback quietly doubles the tails.
        Workflow::WarmupRender | Workflow::Unclear => bars,
    });

    let region = grid.region(skip_bars, bars, params.align);
    let loop_frames = usize::try_from(region.len()).map_err(|_| Error::LoopTooLong(region.len()))?;
    let available = source.frames().saturating_sub(region.start as usize);
    let short_by = loop_frames.saturating_sub(available);

    // The two paths, never combined: a warmup render already contains the settled
    // state, so folding its tail in would add the reverb twice.
    let (mut buffer, taken) = if fold && short_by == 0 {
        // Foldback needs the loop *and* everything after it, in one piece.
        let from_start = source.slice(
            usize::try_from(region.start).unwrap_or(usize::MAX),
            source.frames(),
        );
        let (folded, report) = foldback::foldback(&from_start, loop_frames)?;
        (folded, Taken::Foldback(report))
    } else {
        // A source too short to fill the loop has no tail to fold, so there is
        // nothing for path B to do with it: take what is there and say so.
        let taken = if fold {
            Taken::TooShortToFold
        } else {
            Taken::StraightCut
        };
        (cut::cut(&source, region).buffer, taken)
    };

    // Fades: on for a straight cut, off for a foldback. A foldback loop is
    // circular by construction, and fading both ends to zero would undo exactly
    // the continuity it just computed. A straight cut has no such guarantee — its
    // boundaries fall wherever the bar grid says, mid-waveform or not.
    let fade = if params.no_fade {
        fade::Fade::new(0, fade::FadeShape::default())
    } else {
        match params.fade_ms {
            Some(ms) => fade::Fade::from_millis(ms, format.sample_rate),
            None if fold => fade::Fade::new(0, fade::FadeShape::default()),
            None => fade::Fade::micro(format.sample_rate),
        }
    };
    fade.apply_both(&mut buffer);

    // Varispeed, after the cut: the bar grid is exact in the original domain, so
    // resampling last costs one rounding of the output length instead of
    // compounding with the cut.
    let ratio = resolve_ratio(params, &grid, bars)?;
    let final_tempo = ratio.resulting_tempo(tempo)?;
    if !ratio.is_unity() {
        // The length comes from the grid at the new tempo, never from
        // `old_length / ratio` — that would round a second time.
        let target = grid.resampled_length(bars, ratio) as usize;
        buffer = resample::SincResampler::default()
            .with_edge(resample::Edge::Wrap)
            .resample(&buffer, target)?;
    }

    // Character after the varispeed, because both filters scale their corner
    // frequencies with the speed actually played — and because a filter can only
    // be warmed up over a loop whose length has stopped changing.
    let character = tape::apply(&mut buffer, &params.tape, ratio);

    let mut peak = gain::Peak::measure(&buffer);
    let normalized = if params.normalize {
        gain::normalize(&mut buffer, gain::FULL_SCALE).map(|factor| {
            let before = peak.value;
            peak = gain::Peak::measure(&buffer);
            (factor, before)
        })
    } else {
        None
    };

    // Dither immediately before quantisation, and only when the depth actually
    // drops — noise added at or above the source depth is pure loss.
    let mode = params.dither.resolve(format.bits_per_sample, params.depth);
    let applied = dither::apply(&mut buffer, params.depth, mode, params.dither_seed)
        .map(|applied| (mode, applied));

    Ok(Outcome {
        buffer,
        source_frames: source.frames(),
        sample_rate: format.sample_rate,
        source_bits: format.bits_per_sample,
        tempo,
        tempo_source,
        grid,
        bars,
        bars_source,
        tail,
        shape,
        detected,
        chosen,
        skip_bars,
        region,
        loop_frames,
        available,
        short_by,
        taken,
        fade,
        ratio,
        final_tempo,
        tape: character,
        normalized,
        dither: applied,
        peak,
    })
}

/// The tempo, and where it came from.
///
/// The parameter wins, then whichever of the two file sources
/// [`Params::prefer_name_tempo`] puts first. For this archive the filename is the
/// better source — not one file in it carries an `acid` chunk — but a file that
/// does declare one should be believed over a name that might just hold a date.
fn resolve_tempo(wav: &Wav, name: &str, params: &Params) -> Result<(Tempo, TempoSource)> {
    let from_name = naming::tempo_from_name(name);
    let from_acid = wav.tags().declared_tempo().and_then(Tempo::from_f32);

    let candidates: [(Option<Tempo>, TempoSource); 3] = if params.prefer_name_tempo {
        [
            (params.bpm, TempoSource::Given),
            (from_name, TempoSource::Filename),
            (from_acid, TempoSource::AcidChunk),
        ]
    } else {
        [
            (params.bpm, TempoSource::Given),
            (from_acid, TempoSource::AcidChunk),
            (from_name, TempoSource::Filename),
        ]
    };
    candidates
        .into_iter()
        .find_map(|(t, source)| t.map(|t| (t, source)))
        .ok_or(Error::NoTempo)
}

/// The varispeed ratio the parameters ask for.
///
/// `snap` is the interesting one: it nudges the *resulting* tempo to the nearest
/// at which the loop is a whole number of samples. On its own — no ratio, no
/// target — it makes an otherwise inexact loop exact for the price of a few cents.
pub fn resolve_ratio(params: &Params, grid: &Grid, bars: u64) -> Result<Ratio> {
    let source = grid.tempo;
    let wanted = match (params.ratio, params.target_bpm) {
        (Some(ratio), _) => ratio,
        (None, Some(target)) => Ratio::from_tempi(source, target)?,
        (None, None) => Ratio::UNITY,
    };
    if !params.snap {
        return Ok(wanted);
    }

    // Candidates are searched around the tempo the varispeed lands on, not around
    // the source tempo.
    let landing = wanted.resulting_tempo(source)?;
    let near = Grid { tempo: landing, ..*grid };
    if near.is_sample_exact(bars) {
        return Ok(wanted);
    }
    let Some(best) = near
        .sample_exact_bpms(bars, params.snap_window)
        .into_iter()
        .filter_map(|bpm| Tempo::bpm(bpm).ok())
        // Nearest in BPM, compared as exact fractions — `Rational` is `Ord`, so
        // this needs no float and no scaled integer key.
        .min_by_key(|t| (t.value() - landing.value()).abs())
    else {
        // An explicit `snap` that quietly did nothing would be worse than a
        // refusal: the caller asked for an exact loop and would not get one.
        return Err(Error::NoSampleExactTempo {
            landing: landing.to_string(),
            window: params.snap_window,
            bars,
        });
    };
    Ratio::from_tempi(source, best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav::{write, Metadata, WriteSpec};

    const RATE: u32 = 8_000;
    /// One bar of 4/4 at 200 BPM at [`RATE`]. Exact, deliberately.
    const BAR: usize = 9_600;

    /// A WAVE file of `bars` bars, optionally declaring a tempo in an `acid`
    /// chunk. Round-tripped through the writer and reader so the pipeline sees
    /// exactly what it would see from disk.
    fn file(bars: usize, acid_bpm: Option<f32>) -> Vec<u8> {
        let frames = bars * BAR;
        let data: Vec<f64> = (0..frames).map(|i| 0.25 * (i as f64 * 0.05).sin()).collect();
        let buffer = AudioBuffer::new(vec![data], RATE);
        let metadata = match acid_bpm {
            Some(bpm) => Metadata::for_loop(bpm, bars as u32 * 4, 4, 4, frames as u32),
            None => Metadata::default(),
        };
        write::write(
            &buffer,
            &WriteSpec {
                depth: BitDepth::Int24,
                metadata,
            },
        )
        .unwrap()
    }

    fn params() -> Params {
        Params {
            bpm_unit: BpmUnit::quarter(),
            ..Params::default()
        }
    }

    #[test]
    fn the_tempo_comes_from_the_name_when_the_file_declares_nothing() {
        let bytes = file(4, None);
        let wav = Wav::parse(&bytes).unwrap();
        let out = run(&wav, "200 loop.wav", &params()).unwrap();
        assert_eq!(out.tempo_source, TempoSource::Filename);
        assert_eq!(out.tempo.value().to_f64(), 200.0);
        assert_eq!(out.bars, 4);
        assert_eq!(out.bars_source, BarsSource::FileLength);
    }

    #[test]
    fn a_declared_tempo_outranks_the_name_unless_asked_otherwise() {
        // A name might just contain a date; an `acid` chunk is a statement.
        let bytes = file(4, Some(100.0));
        let wav = Wav::parse(&bytes).unwrap();

        let out = run(&wav, "200 loop.wav", &params()).unwrap();
        assert_eq!(out.tempo_source, TempoSource::AcidChunk);
        assert_eq!(out.tempo.value().to_f64(), 100.0);

        // …and the archive's case, where the name is the better source because
        // not one of its files carries a chunk at all.
        let flipped = Params { prefer_name_tempo: true, ..params() };
        let out = run(&wav, "200 loop.wav", &flipped).unwrap();
        assert_eq!(out.tempo_source, TempoSource::Filename);
        assert_eq!(out.tempo.value().to_f64(), 200.0);
    }

    #[test]
    fn a_given_tempo_outranks_both() {
        let bytes = file(4, Some(100.0));
        let wav = Wav::parse(&bytes).unwrap();
        let given = Params { bpm: Some(Tempo::bpm(150).unwrap()), ..params() };
        let out = run(&wav, "200 loop.wav", &given).unwrap();
        assert_eq!(out.tempo_source, TempoSource::Given);
        assert_eq!(out.tempo.value().to_f64(), 150.0);
    }

    #[test]
    fn a_fractional_declared_tempo_survives_as_a_fraction() {
        // 103.5 has to become 207/2, not the nearest binary approximation of it,
        // because the fraction is what makes the loop length exact.
        let bytes = file(4, Some(103.5));
        let wav = Wav::parse(&bytes).unwrap();
        let out = run(&wav, "loop.wav", &params()).unwrap();
        assert_eq!(out.tempo.value(), crate::Rational::new(207, 2));
    }

    #[test]
    fn no_tempo_anywhere_is_an_error_naming_what_is_missing() {
        let bytes = file(4, None);
        let wav = Wav::parse(&bytes).unwrap();
        assert_eq!(run(&wav, "nameless.wav", &params()).unwrap_err(), Error::NoTempo);
    }

    #[test]
    fn a_short_source_is_recorded_rather_than_refused() {
        // The pipeline reports, the caller decides. A UI wants to grey out a
        // button here, and the CLI wants to refuse on a real run but report on a
        // dry one — deciding here would make one of those impossible.
        let bytes = file(4, None);
        let wav = Wav::parse(&bytes).unwrap();
        let asking_for_more = Params { bars: Some(8), ..params() };
        let out = run(&wav, "200 loop.wav", &asking_for_more).unwrap();

        assert_eq!(out.bars, 8);
        assert_eq!(out.bars_source, BarsSource::Given);
        assert!(out.short_by > 0, "a 4-bar file filled an 8-bar loop");
        assert_eq!(out.loop_frames, out.available + out.short_by);
        assert!(out.drifts());
        let (missing, seconds) = out.drift().unwrap();
        assert_eq!(missing, out.short_by);
        assert!((seconds - missing as f64 / RATE as f64).abs() < 1e-12);
    }

    #[test]
    fn a_long_enough_source_drifts_by_nothing() {
        let bytes = file(8, None);
        let wav = Wav::parse(&bytes).unwrap();
        let out = run(&wav, "200 loop.wav", &params()).unwrap();
        assert_eq!(out.short_by, 0);
        assert!(!out.drifts());
        assert_eq!(out.drift(), None);
        assert_eq!(out.buffer.frames(), out.loop_frames);
    }

    #[test]
    fn forcing_a_workflow_is_recorded_next_to_the_detected_one() {
        // Both are kept, because the report has to be able to say that the two
        // disagree — a forced path that silently replaced the detection would
        // hide the one fact worth seeing.
        let bytes = file(8, None);
        let wav = Wav::parse(&bytes).unwrap();
        let forced = Params {
            workflow: Some(Workflow::TailFoldback),
            ..params()
        };
        let out = run(&wav, "200 loop.wav", &forced).unwrap();
        assert_eq!(out.chosen, Workflow::TailFoldback);
        assert_ne!(out.detected, Workflow::TailFoldback);
        // Path B starts at the beginning; path A would have skipped a loop.
        assert_eq!(out.skip_bars, 0);
        assert!(matches!(out.taken, Taken::Foldback(_)));
    }

    #[test]
    fn a_foldback_is_not_faded_and_a_straight_cut_is() {
        // A folded loop is circular by construction; fading both ends would undo
        // exactly the continuity the foldback just computed.
        let bytes = file(8, None);
        let wav = Wav::parse(&bytes).unwrap();

        let folded = run(
            &wav,
            "200 loop.wav",
            &Params { workflow: Some(Workflow::TailFoldback), ..params() },
        )
        .unwrap();
        assert!(folded.fade.is_none());

        let straight = run(
            &wav,
            "200 loop.wav",
            &Params { workflow: Some(Workflow::WarmupRender), ..params() },
        )
        .unwrap();
        assert!(!straight.fade.is_none());
        assert!(matches!(straight.taken, Taken::StraightCut));
    }

    #[test]
    fn varispeed_takes_its_length_from_the_grid_at_the_new_tempo() {
        let bytes = file(8, None);
        let wav = Wav::parse(&bytes).unwrap();
        let slower = Params {
            target_bpm: Some(Tempo::bpm(100).unwrap()),
            ..params()
        };
        let out = run(&wav, "200 loop.wav", &slower).unwrap();

        assert_eq!(out.final_tempo.value().to_f64(), 100.0);
        assert!(out.ratio.is_exact(), "200 → 100 should be the fraction 1/2");
        // The file is 8 bars and reads as already trimmed, so the loop is 8
        // bars: 8 · 4 · 60/100 · 8000 at the new tempo.
        assert_eq!(out.bars, 8);
        assert_eq!(out.buffer.frames(), 8 * 4 * 60 * 8_000 / 100);
        assert_eq!(out.buffer.frames(), out.loop_frames * 2);
    }

    #[test]
    fn the_bypassed_character_leaves_the_buffer_alone() {
        let bytes = file(8, None);
        let wav = Wav::parse(&bytes).unwrap();
        let clean = run(&wav, "200 loop.wav", &params()).unwrap();
        let with_tape = run(
            &wav,
            "200 loop.wav",
            &Params { tape: tape::TapeParams::on(), ..params() },
        )
        .unwrap();

        assert!(clean.tape.is_noop());
        assert!(!with_tape.tape.is_noop());
        assert_eq!(with_tape.buffer.frames(), clean.buffer.frames());
        assert_ne!(with_tape.buffer, clean.buffer);
    }

    #[test]
    fn dither_policy_auto_only_fires_when_the_depth_drops() {
        assert_eq!(DitherPolicy::Auto.resolve(24, BitDepth::Int16), Dither::Tpdf);
        assert_eq!(DitherPolicy::Auto.resolve(16, BitDepth::Int24), Dither::None);
        assert_eq!(DitherPolicy::Auto.resolve(24, BitDepth::Float32), Dither::None);
        // The explicit policies do not consult the source at all.
        assert_eq!(DitherPolicy::Flat.resolve(16, BitDepth::Int24), Dither::Tpdf);
        assert_eq!(DitherPolicy::Shaped.resolve(16, BitDepth::Int24), Dither::Shaped);
        assert_eq!(DitherPolicy::Off.resolve(24, BitDepth::Int16), Dither::None);
    }

    #[test]
    fn the_same_input_and_parameters_give_the_same_buffer() {
        // Invariant 4, at the level a front end sees it.
        let bytes = file(8, None);
        let wav = Wav::parse(&bytes).unwrap();
        let p = Params { tape: tape::TapeParams::on(), normalize: true, ..params() };
        assert_eq!(
            run(&wav, "200 loop.wav", &p).unwrap().buffer,
            run(&wav, "200 loop.wav", &p).unwrap().buffer
        );
    }
}
