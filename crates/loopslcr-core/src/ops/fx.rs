//! The insert: a filter and an overdrive, in either order.
//!
//! # Why this is not [`filter`](super::filter)
//!
//! That module filters a *loop*, and has to go to some trouble about it: a
//! filter started from silence puts a transient at frame 0, which is exactly the
//! seam this tool exists to clean, so it runs the material twice and keeps the
//! second pass.
//!
//! This one filters a *play head*, and the play head does not stop at the seam.
//! It runs from the moment playback starts until it stops, across every loop
//! boundary, every jump and every swap, carrying its state the whole way — the
//! same as the wire it is standing in for. There is nothing to warm up, because
//! nothing here ever restarts. That difference is the whole reason these are two
//! modules and not one with a flag.
//!
//! # Why the order is a control
//!
//! A lowpass in front of an overdrive takes the highs away before they can be
//! distorted: the drive only ever hears what got through, and the result stays
//! as dark as the filter is. A lowpass behind an overdrive takes away the highs
//! the drive itself made: the distortion happens on the whole signal and the
//! filter then decides how much of the result you hear, which is a sweep with
//! something to sweep *through*.
//!
//! Neither is the correct one, so neither is hard-wired. They are two sounds and
//! the switch picks between them.
//!
//! # Why the delay and the reverb come after both, in that order
//!
//! Because the alternative is a room recorded through a distortion pedal. The
//! filter and the drive are what the sound *is*; the delay and the reverb are
//! where it *is*. Putting the space first would mean the drive flattening the
//! tail as well as the source, which is the sound of a broken send rather than a
//! choice anyone makes on purpose. Delay before reverb for the same reason a
//! desk puts it there: the echoes are events in the room, so the room should
//! hear them.

// The insert is the sample domain.
#![allow(clippy::float_arithmetic)]

/// Which comes first.
use crate::ops::delay::{Delay, DelaySettings};
use crate::ops::reverb::{Reverb, ReverbSettings};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Route {
    /// Filter, then drive. The drive hears only what got through.
    #[default]
    FilterFirst,
    /// Drive, then filter. The filter decides how much of the dirt you hear.
    DriveFirst,
}

/// What the filter does, if anything.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// A wire. Not a filter at a harmless setting — no filter at all.
    #[default]
    Off,
    LowPass,
    HighPass,
    BandPass,
}

/// Everything the insert can be asked for.
///
/// One struct rather than a setter each, because these are read on the audio
/// thread and a half-applied change is a sound nobody asked for. The caller
/// hands over a whole position of the panel and [`Fx::set`] does the arithmetic
/// once, off the per-sample path.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FxSettings {
    pub mode: Mode,
    /// The corner, in hertz.
    pub cutoff_hz: f64,
    /// How much the corner is lifted, `0.0` to `1.0`.
    pub resonance: f64,
    /// How hard the signal is pushed into the curve, `0.0` to `1.0`. Zero is a
    /// wire, and exactly a wire — see [`drive`](Fx::drive).
    pub drive: f64,
    /// Trim after both, linear. A filter and a drive both change the level, and
    /// a knob that changes the level is unusable for judging a sound.
    pub output: f64,
    pub route: Route,
    /// The echoes, after both. See [`DelaySettings`].
    pub delay: DelaySettings,
    /// The room, after the echoes. See [`ReverbSettings`].
    pub reverb: ReverbSettings,
}

impl Default for FxSettings {
    fn default() -> Self {
        FxSettings {
            mode: Mode::Off,
            cutoff_hz: 1_000.0,
            resonance: 0.0,
            drive: 0.0,
            output: 1.0,
            route: Route::FilterFirst,
            delay: DelaySettings::default(),
            reverb: ReverbSettings::default(),
        }
    }
}

impl FxSettings {
    /// Whether this position of the panel is indistinguishable from a wire.
    ///
    /// Not an optimisation dressed up as a predicate: the preview checks it and
    /// takes the old path when it holds, so a session that never touches the
    /// insert produces the same samples it produced before the insert existed.
    /// "Off" that still ran a filter at unity would be off to two decimal
    /// places, which is not the same claim.
    pub fn is_wire(&self) -> bool {
        self.mode == Mode::Off
            && self.drive <= 0.0
            && self.output == 1.0
            && self.delay.is_wire()
            && self.reverb.is_wire()
    }
}

/// The insert, with a filter's worth of state per channel.
///
/// The state is one small array per channel, allocated when the preview is
/// built. Stereo filtered through one shared state would collapse the image the
/// moment the resonance came up, because the two channels would be sharing a
/// resonator.
pub struct Fx {
    settings: FxSettings,
    sample_rate: u32,
    /// Coefficients of the topology-preserving state variable filter, computed
    /// in [`set`](Fx::set) rather than per sample.
    a1: f64,
    a2: f64,
    a3: f64,
    /// The damping, `2/Q`. Also the highpass's own term, so it is kept.
    k: f64,
    /// Gain into the curve, what a full-scale sample comes out of it at, and
    /// how much of the curved signal is used.
    push: f64,
    ceiling: f64,
    blend: f64,
    /// `[ic1eq, ic2eq]` per channel.
    state: Vec<[f64; 2]>,
    delay: Delay,
    reverb: Reverb,
}

impl Fx {
    /// An insert for `channels` channels at `sample_rate`, set to a wire.
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        let mut fx = Fx {
            settings: FxSettings::default(),
            sample_rate: sample_rate.max(1),
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 2.0,
            push: 1.0,
            ceiling: 1.0,
            blend: 0.0,
            state: vec![[0.0; 2]; channels],
            delay: Delay::new(channels, sample_rate.max(1)),
            reverb: Reverb::new(channels, sample_rate.max(1)),
        };
        fx.set(FxSettings::default());
        fx
    }

    pub fn settings(&self) -> FxSettings {
        self.settings
    }

    /// Takes a whole panel position and works out the coefficients.
    ///
    /// Every value is brought into range here rather than trusted: this is fed
    /// from a control surface, across a JNI boundary, and a cutoff above Nyquist
    /// or a NaN resonance does not sound like a mistake, it sounds like an
    /// explosion. Cheap enough to call every block, which is what the preview
    /// does — there is nowhere to store "has this changed" that would be
    /// cheaper than recomputing.
    pub fn set(&mut self, settings: FxSettings) {
        let rate = f64::from(self.sample_rate);
        let cutoff = sane(settings.cutoff_hz, 1_000.0).clamp(20.0, rate * 0.45);
        let resonance = sane(settings.resonance, 0.0).clamp(0.0, 1.0);
        let drive = sane(settings.drive, 0.0).clamp(0.0, 1.0);
        let output = sane(settings.output, 1.0).clamp(0.0, 4.0);

        self.settings = FxSettings {
            mode: settings.mode,
            cutoff_hz: cutoff,
            resonance,
            drive,
            output,
            route: settings.route,
            delay: settings.delay,
            reverb: settings.reverb,
        };

        // Zavalishin's TPT state variable filter. Chosen over a biquad because
        // the cutoff here is a knob under a hand: this topology stays stable
        // while its coefficients move, where a direct-form biquad swept quickly
        // will bang.
        let g = (std::f64::consts::PI * cutoff / rate).tan();
        // Q from 0.5 (no lift at all) to 10 at the top of the knob. Stopping
        // short of self-oscillation on purpose: a filter that howls on its own
        // is a synth voice, and this one is standing in a signal path.
        self.k = 2.0 - 1.9 * resonance;
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;

        // Up to +24 dB into the curve. The blend is the same knob, so drive at
        // zero is the input unchanged rather than `tanh` of it — `tanh(x)` is
        // not `x`, and an effect whose "off" is already audible has no off.
        self.push = 1.0 + 15.0 * drive;
        self.ceiling = self.push.tanh();
        self.blend = drive;

        // Each of these clamps its own knobs, so the panel this reports back is
        // what the boxes actually took rather than what arrived.
        self.delay.set(settings.delay);
        self.reverb.set(settings.reverb, self.sample_rate);
        self.settings.delay = self.delay.settings();
        self.settings.reverb = self.reverb.settings();
    }

    /// Forgets the filter's state.
    ///
    /// Only for a preview that has stopped. Calling it while playing would put
    /// the transient back that the whole design avoids.
    pub fn reset(&mut self) {
        for channel in self.state.iter_mut() {
            *channel = [0.0; 2];
        }
        self.delay.reset();
        self.reverb.reset();
    }

    /// Whether the insert is indistinguishable from a wire right now.
    pub fn is_wire(&self) -> bool {
        self.settings.is_wire()
    }

    /// One sample of one channel, in the order the panel asks for.
    ///
    /// An unknown channel is passed through rather than panicking: this runs on
    /// the audio thread, where the cost of being wrong is a crash inside a
    /// callback the operating system is waiting on.
    pub fn process(&mut self, channel: usize, input: f64) -> f64 {
        if channel >= self.state.len() {
            return input;
        }
        let value = match self.settings.route {
            Route::FilterFirst => {
                let filtered = self.filter(channel, input);
                self.drive(filtered)
            }
            Route::DriveFirst => {
                let driven = self.drive(input);
                self.filter(channel, driven)
            }
        };
        // The trim before the space, not after: it is the level of the *sound*,
        // and moving it should change how hard the room is hit rather than how
        // loud a room you already filled comes out.
        let trimmed = value * self.settings.output;
        let echoed = self.delay.process(channel, trimmed);
        self.reverb.process(channel, echoed)
    }

    /// Closes the frame. Must be called once per frame, after every channel —
    /// the delay collects its writes until here so that ping-pong does not
    /// depend on which channel was processed first.
    pub fn advance(&mut self) {
        self.delay.advance();
    }

    /// The filter alone. `Mode::Off` does not touch the sample.
    fn filter(&mut self, channel: usize, input: f64) -> f64 {
        if self.settings.mode == Mode::Off {
            return input;
        }
        let [ic1, ic2] = self.state[channel];
        let v3 = input - ic2;
        let v1 = self.a1 * ic1 + self.a2 * v3;
        let v2 = ic2 + self.a2 * ic1 + self.a3 * v3;
        self.state[channel] = [2.0 * v1 - ic1, 2.0 * v2 - ic2];
        match self.settings.mode {
            Mode::LowPass => v2,
            Mode::BandPass => v1,
            // The highpass falls out of the other two: what is left of the input
            // once the band and the low end have been taken off it.
            Mode::HighPass => input - self.k * v1 - v2,
            Mode::Off => input,
        }
    }

    /// The overdrive alone.
    ///
    /// The signal is pushed into `tanh` and the result normalised by what a
    /// full-scale sample would have come out at, so the loud parts stay where
    /// they were and everything underneath them comes up. That is what turning
    /// up an overdrive does: it does not add level, it takes the distance
    /// between quiet and loud away.
    ///
    /// It also means the drive cannot make an overload. A sample inside full
    /// scale leaves inside full scale at any setting of the knob — worth having
    /// in a box that sits in front of the master fader, where the alternative is
    /// a meter that goes red because an effect was turned up.
    fn drive(&self, input: f64) -> f64 {
        if self.blend <= 0.0 {
            return input;
        }
        let curved = (self.push * input).tanh() / self.ceiling;
        input + self.blend * (curved - input)
    }
}

/// A NaN has no place on a knob, so it becomes the default. An infinity does
/// have one — it is the top of the knob pressed harder — so it is left for the
/// clamp to bring in.
fn sane(value: f64, fallback: f64) -> f64 {
    if value.is_nan() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const RATE: u32 = 48_000;

    fn tone(frames: usize, hz: f64, amplitude: f64) -> Vec<f64> {
        (0..frames)
            .map(|i| amplitude * (2.0 * PI * hz * i as f64 / f64::from(RATE)).sin())
            .collect()
    }

    /// Amplitude at `hz`, by correlation, over the settled second half.
    fn magnitude(samples: &[f64], hz: f64) -> f64 {
        let start = samples.len() / 2;
        let window = &samples[start..];
        let n = window.len() as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in window.iter().enumerate() {
            let phase = 2.0 * PI * hz * (start + i) as f64 / f64::from(RATE);
            re += s * phase.cos();
            im += s * phase.sin();
        }
        2.0 * (re * re + im * im).sqrt() / n
    }

    fn run(fx: &mut Fx, input: &[f64]) -> Vec<f64> {
        input.iter().map(|&s| fx.process(0, s)).collect()
    }

    #[test]
    fn an_insert_at_rest_is_a_wire_sample_for_sample() {
        let mut fx = Fx::new(2, RATE);
        assert!(fx.is_wire());
        let input = tone(2_000, 440.0, 0.7);
        let out = run(&mut fx, &input);
        assert_eq!(out, input, "an untouched insert changed the samples");
    }

    #[test]
    fn a_lowpass_keeps_the_low_tone_and_loses_the_high_one() {
        let low = 200.0;
        let high = 8_000.0;
        let settings = FxSettings {
            mode: Mode::LowPass,
            cutoff_hz: 1_000.0,
            ..FxSettings::default()
        };

        let mut fx = Fx::new(1, RATE);
        fx.set(settings);
        let kept = magnitude(&run(&mut fx, &tone(20_000, low, 0.5)), low);

        let mut fx = Fx::new(1, RATE);
        fx.set(settings);
        let lost = magnitude(&run(&mut fx, &tone(20_000, high, 0.5)), high);

        assert!(kept > 0.45, "the low tone came out at {kept}");
        assert!(lost < 0.02, "the high tone came out at {lost}");
    }

    #[test]
    fn a_highpass_does_it_the_other_way_round() {
        let low = 200.0;
        let high = 8_000.0;
        let settings = FxSettings {
            mode: Mode::HighPass,
            cutoff_hz: 1_000.0,
            ..FxSettings::default()
        };

        let mut fx = Fx::new(1, RATE);
        fx.set(settings);
        let lost = magnitude(&run(&mut fx, &tone(20_000, low, 0.5)), low);

        let mut fx = Fx::new(1, RATE);
        fx.set(settings);
        let kept = magnitude(&run(&mut fx, &tone(20_000, high, 0.5)), high);

        assert!(kept > 0.45, "the high tone came out at {kept}");
        assert!(lost < 0.05, "the low tone came out at {lost}");
    }

    #[test]
    fn resonance_lifts_what_sits_on_the_corner() {
        let corner = 1_000.0;
        let plain = {
            let mut fx = Fx::new(1, RATE);
            fx.set(FxSettings {
                mode: Mode::LowPass,
                cutoff_hz: corner,
                resonance: 0.0,
                ..FxSettings::default()
            });
            magnitude(&run(&mut fx, &tone(20_000, corner, 0.2)), corner)
        };
        let lifted = {
            let mut fx = Fx::new(1, RATE);
            fx.set(FxSettings {
                mode: Mode::LowPass,
                cutoff_hz: corner,
                resonance: 1.0,
                ..FxSettings::default()
            });
            magnitude(&run(&mut fx, &tone(20_000, corner, 0.2)), corner)
        };
        assert!(
            lifted > plain * 3.0,
            "resonance moved the corner from {plain} to {lifted}"
        );
    }

    #[test]
    fn drive_holds_the_top_still_and_brings_everything_under_it_up() {
        let mut fx = Fx::new(1, RATE);
        fx.set(FxSettings {
            drive: 1.0,
            ..FxSettings::default()
        });

        // The loud end stays where it was.
        let loud = fx.process(0, 1.0);
        assert!((loud - 1.0).abs() < 0.01, "a full-scale sample became {loud}");

        // What was quiet is not quiet any more. That is the effect.
        let quiet = fx.process(0, 0.05);
        assert!(quiet > 0.5, "a sample at 0.05 came out at {quiet}");

        // And at no setting of the knob can it make an overload out of a signal
        // that did not have one.
        for step in 0..=10 {
            fx.set(FxSettings {
                drive: f64::from(step) / 10.0,
                ..FxSettings::default()
            });
            for input in [-1.0, -0.7, 0.0, 0.3, 1.0] {
                let out = fx.process(0, input);
                assert!(out.abs() <= 1.0001, "{input} became {out} at step {step}");
            }
        }
    }

    #[test]
    fn the_route_decides_which_one_hears_the_other() {
        // A low tone, a lowpass a little above it, and as much drive as there
        // is. Filter first, the tone gets through nearly whole and the drive
        // makes harmonics all the way up that nothing afterwards removes. Drive
        // first, the same harmonics are made and the filter is standing after
        // them. Listen well above the corner — at the ninth harmonic — and the
        // two orders are not close.
        let hz = 100.0;
        let up_there = hz * 9.0;
        let settings = FxSettings {
            mode: Mode::LowPass,
            cutoff_hz: 300.0,
            drive: 1.0,
            ..FxSettings::default()
        };
        let input = tone(20_000, hz, 0.9);

        let mut fx = Fx::new(1, RATE);
        fx.set(FxSettings {
            route: Route::FilterFirst,
            ..settings
        });
        let dirty = magnitude(&run(&mut fx, &input), up_there);

        let mut fx = Fx::new(1, RATE);
        fx.set(FxSettings {
            route: Route::DriveFirst,
            ..settings
        });
        let cleaned = magnitude(&run(&mut fx, &input), up_there);

        assert!(
            dirty > cleaned * 5.0,
            "the ninth harmonic: filter first {dirty}, drive first {cleaned}"
        );
    }

    #[test]
    fn each_channel_filters_on_its_own() {
        let mut fx = Fx::new(2, RATE);
        fx.set(FxSettings {
            mode: Mode::LowPass,
            cutoff_hz: 500.0,
            resonance: 0.9,
            ..FxSettings::default()
        });

        // Only the left channel is fed. A shared resonator would leak into the
        // right one, which is how a stereo image collapses.
        let mut right = 0.0f64;
        for _ in 0..2_000 {
            fx.process(0, 1.0);
            right = right.max(fx.process(1, 0.0).abs());
        }
        assert_eq!(right, 0.0, "the silent channel came out at {right}");
    }

    #[test]
    fn nothing_a_control_surface_can_send_makes_it_misbehave() {
        let mut fx = Fx::new(1, RATE);
        fx.set(FxSettings {
            mode: Mode::LowPass,
            cutoff_hz: f64::NAN,
            resonance: f64::INFINITY,
            drive: -3.0,
            output: f64::NAN,
            route: Route::FilterFirst,
            ..FxSettings::default()
        });
        let settings = fx.settings();
        assert!(settings.cutoff_hz.is_finite() && settings.cutoff_hz >= 20.0);
        assert_eq!(settings.resonance, 1.0, "an infinite resonance is the top of the knob");
        assert_eq!(settings.drive, 0.0);
        assert_eq!(settings.output, 1.0);

        for &s in tone(1_000, 440.0, 0.9).iter() {
            let out = fx.process(0, s);
            assert!(out.is_finite(), "{s} became {out}");
        }

        // A cutoff above Nyquist is brought back rather than turning the filter
        // into an oscillator.
        fx.set(FxSettings {
            mode: Mode::LowPass,
            cutoff_hz: 1e9,
            ..FxSettings::default()
        });
        for &s in tone(1_000, 440.0, 0.9).iter() {
            assert!(fx.process(0, s).is_finite());
        }
    }

    #[test]
    fn an_output_trim_moves_the_whole_thing_and_only_that() {
        let mut fx = Fx::new(1, RATE);
        fx.set(FxSettings {
            output: 0.5,
            ..FxSettings::default()
        });
        assert!(!fx.is_wire(), "a trim of a half is not a wire");
        let out = fx.process(0, 0.8);
        assert!((out - 0.4).abs() < 1e-12, "0.8 became {out}");
    }
}
