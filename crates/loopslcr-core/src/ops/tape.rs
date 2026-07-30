//! Tape character: wow, flutter, head-gap loss, head bump.
//!
//! Everything here is off unless [`TapeParams::enabled`] is set, and that single
//! bypass is deliberate. The clean varispeed path has to stay byte-identical
//! forever (the reproducibility invariant), so character is one branch that is
//! either taken whole or not taken at all — never a set of small effects that
//! each leave a trace when nominally at zero.
//!
//! # The problem wow and flutter normally cause
//!
//! Modulating playback speed modulates the *length*. A loop read at a wobbling
//! rate comes out some unpredictable number of samples long, the seam no longer
//! lands on the bar line, and the sample-exactness this whole program is built
//! for is gone.
//!
//! # The fix: modulate position, not rate
//!
//! The wobble is defined as a **displacement** of the read position, `p(j) = j +
//! D(j)`, where `D` is a sum of sinusoids whose periods divide the loop exactly.
//! Two consequences fall out of that and neither is approximate:
//!
//! - **The length is untouched.** The output has as many frames as the input,
//!   because the output index still advances by one per frame. Nothing to round.
//! - **The seam stays continuous.** `D` is loop-periodic, so the position the
//!   next repeat starts from is the position this one would have continued to.
//!
//! The rate deviation is then `D'(j)`, which is loop-periodic and — being the
//! derivative of a periodic function — has an integral of exactly zero over the
//! loop. That is the zero-mean guarantee, and it is a theorem here rather than a
//! calibration.
//!
//! `D(0)` is *not* forced to zero, and must not be. Pinning the wobble to a node
//! at the seam would put a fixed point in the modulation once per repeat, which
//! is audible as a tick — the very artefact being avoided. A non-zero `D(0)` only
//! shifts the whole loop by a fraction of a sample.
//!
//! # Rate quantisation
//!
//! Requested rates are rounded to the nearest whole number of cycles per loop,
//! `k / loopDuration`. The grid is far finer than the ear: a 21-second loop puts
//! it at 0.047 Hz, so wow (0.5–6 Hz) has around a hundred choices and flutter
//! (6–100 Hz) around two thousand. What comes back in [`Tape::wow`] and
//! [`Tape::flutter`] is the rate actually used, not the one asked for.
//!
//! # No randomness
//!
//! Real wow drifts randomly. This does not, because a random component would
//! either break byte-identical reproducibility or need a seed, and a seeded
//! pseudo-random drift on a two-second loop is a fixed pattern anyway. Two
//! sinusoids at unrelated rates already sound irregular over a loop.

// Modulation and filtering are the sample domain.
#![allow(clippy::float_arithmetic)]

use std::f64::consts::PI;

use crate::buffer::AudioBuffer;
use crate::ops::filter::{run_periodic, Biquad, OnePole};
use crate::ops::resample::{Edge, SincResampler};
use crate::timing::Ratio;

/// Peak speed deviation of the wow, in percent. Slow drift, 0.1–0.5 % on a
/// machine worth using.
pub const DEFAULT_WOW_PERCENT: f64 = 0.3;
/// Peak speed deviation of the flutter, in percent. Faster and always smaller
/// than the wow — it comes from the capstan and the tape guides, not the reel.
pub const DEFAULT_FLUTTER_PERCENT: f64 = 0.15;
/// Head-gap loss corner, in Hz at nominal speed.
pub const DEFAULT_HF_ROLLOFF_HZ: f64 = 12_000.0;
/// Head bump height, in dB.
pub const DEFAULT_HEAD_BUMP_DB: f64 = 2.0;
/// Head bump centre, in Hz at nominal speed.
pub const DEFAULT_HEAD_BUMP_HZ: f64 = 60.0;
/// Head bump width. Broad enough to read as weight rather than as a resonance.
pub const HEAD_BUMP_Q: f64 = 0.7;

/// One sinusoidal component of the wobble.
struct Component {
    /// Nominal rate in Hz, before quantisation to the loop.
    hz: f64,
    /// Share of the requested depth. The shares of a group sum to 1.
    share: f64,
    /// Starting phase in radians. Fixed, so the result is reproducible, and
    /// offset between components so their peaks do not stack on frame 0.
    phase: f64,
}

/// Reel and tape-pack irregularity: slow, and the larger of the two.
const WOW: &[Component] = &[
    Component { hz: 0.7, share: 0.6, phase: 0.0 },
    Component { hz: 1.9, share: 0.4, phase: 1.7 },
];

/// Capstan and guide chatter: fast, small, and audible as texture rather than
/// as pitch movement.
const FLUTTER: &[Component] = &[
    Component { hz: 11.0, share: 0.6, phase: 0.6 },
    Component { hz: 27.0, share: 0.4, phase: 2.9 },
];

/// What character to impose. Amounts of zero switch an element off individually;
/// [`Self::enabled`] switches the lot off and restores the clean path exactly.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TapeParams {
    /// The single bypass. False means [`apply`] does not touch the buffer.
    pub enabled: bool,
    /// Wow depth as peak speed deviation in percent.
    pub wow_percent: f64,
    /// Flutter depth as peak speed deviation in percent.
    pub flutter_percent: f64,
    /// Head-gap loss corner in Hz *at nominal speed*, scaled by the varispeed
    /// ratio when applied. Zero switches the rolloff off.
    pub hf_rolloff_hz: f64,
    /// Head bump height in dB. Zero switches the bump off.
    pub head_bump_db: f64,
    /// Head bump centre in Hz at nominal speed, likewise scaled.
    pub head_bump_hz: f64,
}

impl Default for TapeParams {
    /// Off, but with every amount already at a usable value, so `--tape` alone
    /// is a complete setting and each flag is an override rather than a
    /// requirement.
    fn default() -> Self {
        TapeParams {
            enabled: false,
            wow_percent: DEFAULT_WOW_PERCENT,
            flutter_percent: DEFAULT_FLUTTER_PERCENT,
            hf_rolloff_hz: DEFAULT_HF_ROLLOFF_HZ,
            head_bump_db: DEFAULT_HEAD_BUMP_DB,
            head_bump_hz: DEFAULT_HEAD_BUMP_HZ,
        }
    }
}

impl TapeParams {
    /// The defaults, switched on.
    pub fn on() -> Self {
        TapeParams { enabled: true, ..Self::default() }
    }

    /// True when nothing would change even with the bypass open.
    pub fn is_silent(&self) -> bool {
        self.wow_percent <= 0.0
            && self.flutter_percent <= 0.0
            && self.hf_rolloff_hz <= 0.0
            && self.head_bump_db == 0.0
    }
}

/// One modulation group as it was actually realised.
#[derive(Clone, Debug, PartialEq)]
pub struct Wobble {
    /// Quantised rates in Hz, in the order of the components.
    pub rates_hz: Vec<f64>,
    /// Cycles per loop for each — the integers the rates were rounded to.
    pub cycles: Vec<u64>,
    /// Worst-case read displacement, in frames.
    pub peak_frames: f64,
    /// Peak speed deviation actually imposed, in percent.
    pub depth_percent: f64,
}

/// What [`apply`] did.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Tape {
    pub wow: Option<Wobble>,
    pub flutter: Option<Wobble>,
    /// Rolloff corner after scaling by the varispeed ratio, in Hz.
    pub hf_rolloff_hz: Option<f64>,
    /// Head bump centre after scaling, in Hz, with its height in dB.
    pub head_bump: Option<(f64, f64)>,
}

impl Tape {
    /// True when the buffer was left exactly as it came in.
    pub fn is_noop(&self) -> bool {
        *self == Tape::default()
    }
}

/// Imposes tape character on `buffer`, which must be exactly one loop period.
///
/// `speed` is the varispeed ratio already applied, and only scales the two
/// filters. On a real machine the head losses happen at a fixed frequency *on
/// the tape*; played back faster, they appear proportionally higher — which is
/// why a tape sped up sounds brighter and not merely higher.
///
/// The order inside is wobble, then rolloff, then bump. The filters come last on
/// purpose: [`run_periodic`] can only cancel a startup transient for a filter
/// whose coefficients hold still, and running a fixed filter over already-
/// wobbled audio satisfies that where wobbling a filter's own output would not.
pub fn apply(buffer: &mut AudioBuffer, params: &TapeParams, speed: Ratio) -> Tape {
    if !params.enabled || buffer.is_empty() {
        return Tape::default();
    }

    let frames = buffer.frames();
    let sample_rate = buffer.sample_rate();
    let mut report = Tape::default();

    // Wow and flutter are one displacement curve, applied in a single read.
    // Two reads would mean two interpolations and so twice the passband loss for
    // no gain — the curves simply add.
    let wow = plan(WOW, params.wow_percent, frames, sample_rate);
    let flutter = plan(FLUTTER, params.flutter_percent, frames, sample_rate);
    if wow.is_some() || flutter.is_some() {
        let mut terms: Vec<Term> = Vec::new();
        terms.extend(wow.as_ref().map(|w| w.terms.clone()).unwrap_or_default());
        terms.extend(flutter.as_ref().map(|f| f.terms.clone()).unwrap_or_default());
        wobble(buffer, &terms);
        report.wow = wow.map(|w| w.report);
        report.flutter = flutter.map(|f| f.report);
    }

    let speed = speed.to_f64();

    if params.hf_rolloff_hz > 0.0 {
        let corner = params.hf_rolloff_hz * speed;
        for channel in buffer.channels_mut() {
            run_periodic(&mut OnePole::lowpass(corner, sample_rate), channel);
        }
        report.hf_rolloff_hz = Some(corner);
    }

    if params.head_bump_db != 0.0 && params.head_bump_hz > 0.0 {
        let centre = params.head_bump_hz * speed;
        let bump = Biquad::peaking(centre, params.head_bump_db, HEAD_BUMP_Q, sample_rate);
        for channel in buffer.channels_mut() {
            run_periodic(&mut bump.clone(), channel);
        }
        report.head_bump = Some((centre, params.head_bump_db));
    }

    report
}

/// One realised sinusoid of the displacement curve.
#[derive(Copy, Clone, Debug)]
struct Term {
    /// Displacement amplitude in frames.
    amplitude: f64,
    /// Whole cycles per loop.
    cycles: f64,
    phase: f64,
}

struct Plan {
    terms: Vec<Term>,
    report: Wobble,
}

/// Turns a requested depth into whole-cycle-per-loop terms.
///
/// The depth is given as a *speed* deviation, because that is how a tape machine
/// is specified and how it is heard. The displacement amplitude follows from it:
/// for `D(j) = A sin(2π k j / N)` the rate deviation peaks at `A · 2π k / N`, so
/// `A = depth · N / (2π k)`. A slow wobble therefore displaces far more than a
/// fast one at the same depth — 0.3 % wow on a 21-second loop is some 27 frames,
/// the same 0.3 % of flutter a fraction of one. That asymmetry is physical: it is
/// why wow is heard as pitch movement and flutter as roughness.
fn plan(components: &[Component], depth_percent: f64, frames: usize, sample_rate: u32) -> Option<Plan> {
    if depth_percent <= 0.0 || frames == 0 {
        return None;
    }
    let depth = depth_percent / 100.0;
    let frames_f = frames as f64;

    let mut terms = Vec::with_capacity(components.len());
    let mut rates = Vec::with_capacity(components.len());
    let mut cycles = Vec::with_capacity(components.len());
    let mut peak = 0.0;
    let mut realised_depth = 0.0;

    for component in components {
        // Cycles per loop, rounded. At least one: a component slower than the
        // loop itself cannot be periodic in it, and rounding it to zero would
        // leave a constant offset — a detuned loop, not a wobbling one.
        let k = (component.hz * frames_f / sample_rate as f64).round().max(1.0);
        let share = depth * component.share;
        let amplitude = share * frames_f / (2.0 * PI * k);

        terms.push(Term { amplitude, cycles: k, phase: component.phase });
        rates.push(k * sample_rate as f64 / frames_f);
        cycles.push(k as u64);
        peak += amplitude;
        realised_depth += share;
    }

    Some(Plan {
        terms,
        report: Wobble {
            rates_hz: rates,
            cycles,
            // Worst case, not typical: the components peak at different times,
            // so this is a bound rather than a measurement.
            peak_frames: peak,
            depth_percent: realised_depth * 100.0,
        },
    })
}

/// Re-reads `buffer` at a displaced position, in place.
fn wobble(buffer: &mut AudioBuffer, terms: &[Term]) {
    let frames = buffer.frames();
    // The position wobbles around unity, so the local rate is 1 to within a
    // fraction of a percent and no extra anti-aliasing is called for: this is a
    // fractional-delay read, not a rate change. Wrapping, because the buffer is
    // one period of a loop and the displacement routinely reaches past both ends.
    let reader = SincResampler::default().with_edge(Edge::Wrap);
    let displacement: Vec<f64> = (0..frames)
        .map(|j| {
            terms
                .iter()
                .map(|t| {
                    t.amplitude * (2.0 * PI * t.cycles * j as f64 / frames as f64 + t.phase).sin()
                })
                .sum()
        })
        .collect();

    for channel in buffer.channels_mut() {
        let source = channel.clone();
        for (j, sample) in channel.iter_mut().enumerate() {
            *sample = reader.read(&source, j as f64 + displacement[j], 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rational::Rational;

    /// A playback speed, as a percentage *deviation* from nominal — the tape
    /// convention `Ratio::from_percent` takes: 0 is unity, +100 double, −50 half.
    fn speed(deviation_percent: i128) -> Ratio {
        Ratio::from_percent(Rational::from(deviation_percent)).unwrap()
    }

    fn loop_buffer(frames: usize, cycles: usize, amplitude: f64) -> AudioBuffer {
        let data: Vec<f64> = (0..frames)
            .map(|i| amplitude * (2.0 * PI * cycles as f64 * i as f64 / frames as f64).sin())
            .collect();
        AudioBuffer::new(vec![data.clone(), data], 44_100)
    }

    /// The displacement curve for a set of params, as `apply` would build it.
    fn curve(params: &TapeParams, frames: usize, sample_rate: u32) -> Vec<f64> {
        let mut terms = Vec::new();
        for plan in [
            plan(WOW, params.wow_percent, frames, sample_rate),
            plan(FLUTTER, params.flutter_percent, frames, sample_rate),
        ]
        .into_iter()
        .flatten()
        {
            terms.extend(plan.terms);
        }
        (0..frames)
            .map(|j| {
                terms
                    .iter()
                    .map(|t| {
                        t.amplitude
                            * (2.0 * PI * t.cycles * j as f64 / frames as f64 + t.phase).sin()
                    })
                    .sum()
            })
            .collect()
    }

    #[test]
    fn bypassed_is_bit_identical() {
        // The reproducibility invariant. Not "close" — the same bits, so that a
        // clean run today and a clean run after any amount of work on this
        // module produce the same file.
        let source = loop_buffer(9000, 30, 0.5);
        let mut out = source.clone();
        let report = apply(&mut out, &TapeParams::default(), speed(0));
        assert_eq!(out, source, "the bypassed path altered the buffer");
        assert!(report.is_noop());
    }

    #[test]
    fn every_element_can_be_switched_off_individually() {
        let source = loop_buffer(9000, 30, 0.5);
        let params = TapeParams {
            enabled: true,
            wow_percent: 0.0,
            flutter_percent: 0.0,
            hf_rolloff_hz: 0.0,
            head_bump_db: 0.0,
            ..TapeParams::default()
        };
        assert!(params.is_silent());

        let mut out = source.clone();
        let report = apply(&mut out, &params, speed(0));
        assert_eq!(out, source, "an all-zero character still touched the audio");
        assert!(report.is_noop());
    }

    #[test]
    fn the_length_is_untouched() {
        // The whole point of displacing the position rather than the rate.
        for frames in [4410usize, 9000, 44_100, 100_000] {
            let mut out = loop_buffer(frames, 17, 0.5);
            apply(&mut out, &TapeParams::on(), speed(0));
            assert_eq!(out.frames(), frames, "length changed at {frames} frames");
            assert_eq!(out.channel_count(), 2);
        }
    }

    #[test]
    fn the_modulation_is_zero_mean_over_the_loop() {
        // A non-zero mean would be a net speed offset: the loop would come out
        // at the wrong tempo however carefully the length was computed.
        let frames = 44_100;
        let d = curve(&TapeParams::on(), frames, 44_100);
        let mean = d.iter().sum::<f64>() / frames as f64;
        let peak = d.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        assert!(peak > 1.0, "the test curve is too small to mean anything: {peak}");
        assert!(
            mean.abs() < peak * 1e-12,
            "mean {mean} against a peak of {peak}"
        );
    }

    #[test]
    fn the_modulation_is_loop_periodic() {
        // The seam test in its purest form: the displacement one frame past the
        // end has to equal the displacement at the start, or the read position
        // jumps once per repeat.
        let frames = 30_000;
        let params = TapeParams::on();
        let d = curve(&params, frames, 44_100);

        let mut terms = Vec::new();
        for p in [
            plan(WOW, params.wow_percent, frames, 44_100),
            plan(FLUTTER, params.flutter_percent, frames, 44_100),
        ]
        .into_iter()
        .flatten()
        {
            terms.extend(p.terms);
        }
        let at = |j: f64| -> f64 {
            terms
                .iter()
                .map(|t| t.amplitude * (2.0 * PI * t.cycles * j / frames as f64 + t.phase).sin())
                .sum()
        };

        assert!((at(frames as f64) - d[0]).abs() < 1e-9);
        assert!((at(frames as f64 + 0.5) - at(0.5)).abs() < 1e-9);
    }

    #[test]
    fn rates_are_quantised_to_whole_cycles_per_loop() {
        // 44100 frames is one second, so the grid is 1 Hz and the requested
        // rates land on their own integers.
        let p = plan(WOW, 0.3, 44_100, 44_100).unwrap();
        assert_eq!(p.report.cycles, vec![1, 2], "0.7 and 1.9 Hz on a 1 Hz grid");
        assert_eq!(p.report.rates_hz, vec![1.0, 2.0]);

        // A 21.3-second loop: a much finer grid, so the rates land close to what
        // was asked for.
        let frames = 940_800;
        let p = plan(WOW, 0.3, frames, 44_100).unwrap();
        for (asked, got) in WOW.iter().map(|c| c.hz).zip(p.report.rates_hz) {
            assert!((got - asked).abs() < 0.05, "asked {asked} Hz, got {got} Hz");
        }
    }

    #[test]
    fn a_component_slower_than_the_loop_still_gets_a_whole_cycle() {
        // 4410 frames is 0.1 s, where 0.7 Hz rounds to zero cycles. Zero would
        // be a constant displacement — a detuned loop rather than a wobbling
        // one — so it is floored at one.
        let p = plan(WOW, 0.3, 4410, 44_100).unwrap();
        assert!(p.report.cycles.iter().all(|&k| k >= 1), "{:?}", p.report.cycles);
        assert!(p.report.rates_hz.iter().all(|&hz| hz >= 10.0));
    }

    #[test]
    fn depth_is_a_speed_deviation_so_slow_wobbles_displace_further() {
        // The physical asymmetry: at equal depth the displacement goes as 1/rate.
        let frames = 940_800;
        let wow = plan(WOW, 0.3, frames, 44_100).unwrap();
        let flutter = plan(FLUTTER, 0.3, frames, 44_100).unwrap();
        assert!(
            wow.report.peak_frames > flutter.report.peak_frames * 10.0,
            "wow {} frames, flutter {} frames",
            wow.report.peak_frames,
            flutter.report.peak_frames
        );

        // And the realised rate deviation is the depth that was asked for.
        for term in &wow.terms {
            let deviation = term.amplitude * 2.0 * PI * term.cycles / frames as f64;
            let share = WOW
                .iter()
                .find(|c| (c.phase - term.phase).abs() < 1e-12)
                .unwrap()
                .share;
            assert!((deviation - 0.003 * share).abs() < 1e-12, "{deviation}");
        }
    }

    #[test]
    fn the_wobble_moves_the_audio_without_destroying_it() {
        let frames = 44_100;
        let source = loop_buffer(frames, 100, 0.5);
        let mut out = source.clone();
        let params = TapeParams {
            hf_rolloff_hz: 0.0,
            head_bump_db: 0.0,
            ..TapeParams::on()
        };
        let report = apply(&mut out, &params, speed(0));

        assert!(report.wow.is_some() && report.flutter.is_some());
        assert!(report.hf_rolloff_hz.is_none() && report.head_bump.is_none());

        // Something changed …
        assert_ne!(out, source);
        // … but it is still the same tone at the same level, only smeared in
        // time. A broken interpolation would show up as lost amplitude.
        let level = |b: &AudioBuffer| {
            (b.channel(0).iter().map(|s| s * s).sum::<f64>() / frames as f64).sqrt()
        };
        assert!(
            (level(&out) / level(&source) - 1.0).abs() < 0.01,
            "level moved from {} to {}",
            level(&source),
            level(&out)
        );
        // Both channels get the same displacement — a stereo image that wandered
        // would be a bug, not character.
        assert_eq!(out.channel(0), out.channel(1));
    }

    #[test]
    fn the_filters_scale_their_frequencies_with_playback_speed() {
        // A tape sped up sounds brighter as well as higher, because the head
        // losses sit at a fixed frequency on the tape, not in the output.
        let params = TapeParams {
            wow_percent: 0.0,
            flutter_percent: 0.0,
            ..TapeParams::on()
        };

        let mut half = loop_buffer(9000, 30, 0.5);
        let report = apply(&mut half, &params, speed(-50));
        assert_eq!(report.hf_rolloff_hz, Some(DEFAULT_HF_ROLLOFF_HZ * 0.5));
        assert_eq!(report.head_bump, Some((DEFAULT_HEAD_BUMP_HZ * 0.5, DEFAULT_HEAD_BUMP_DB)));

        let mut double = loop_buffer(9000, 30, 0.5);
        let report = apply(&mut double, &params, speed(100));
        assert_eq!(report.hf_rolloff_hz, Some(DEFAULT_HF_ROLLOFF_HZ * 2.0));
        assert_eq!(report.head_bump, Some((DEFAULT_HEAD_BUMP_HZ * 2.0, DEFAULT_HEAD_BUMP_DB)));
    }

    /// The claim the module is built on, measured on the audio rather than on
    /// the modulation curve: after the whole chain the join from the last frame
    /// back to the first is no rougher than any other join in the loop.
    ///
    /// A rate-modulated wobble would fail this even with a perfectly zero-mean
    /// LFO, because its read position would not return to where the next repeat
    /// starts. This is the test that tells the two designs apart.
    #[test]
    fn the_seam_survives_the_whole_chain() {
        let frames = 44_100;
        // A cosine, so the seam sits at full scale rather than on a zero
        // crossing — the same worst case the resampler's edge test uses.
        let data: Vec<f64> = (0..frames)
            .map(|i| 0.5 * (2.0 * PI * 97.0 * i as f64 / frames as f64).cos())
            .collect();
        let mut out = AudioBuffer::new(vec![data], 44_100);
        apply(&mut out, &TapeParams::on(), speed(0));

        let c = out.channel(0);
        let seam = (c[0] - c[frames - 1]).abs();
        let worst_interior = c
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f64, f64::max);
        assert!(
            seam <= worst_interior,
            "the seam steps by {seam}, more than the worst interior step {worst_interior}"
        );
    }

    #[test]
    fn the_character_is_deterministic() {
        // No RNG anywhere in here, so two runs agree bit for bit. Without this
        // the reproducibility invariant would hold only for the clean path.
        let source = loop_buffer(20_000, 40, 0.5);
        let ratio = speed(0);
        let mut a = source.clone();
        let mut b = source.clone();
        assert_eq!(apply(&mut a, &TapeParams::on(), ratio), apply(&mut b, &TapeParams::on(), ratio));
        assert_eq!(a, b);
    }

    #[test]
    fn silence_survives_the_whole_chain() {
        let mut out = AudioBuffer::silence(2, 10_000, 44_100);
        apply(&mut out, &TapeParams::on(), speed(0));
        assert!(out.channels().iter().all(|c| c.iter().all(|&s| s == 0.0)));
    }

    #[test]
    fn an_empty_buffer_is_left_alone() {
        let mut out = AudioBuffer::silence(1, 0, 44_100);
        let report = apply(&mut out, &TapeParams::on(), speed(0));
        assert!(report.is_noop());
        assert_eq!(out.frames(), 0);
    }
}
