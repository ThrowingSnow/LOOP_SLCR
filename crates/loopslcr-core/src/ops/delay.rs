//! A delay line, owned up front.
//!
//! # Why the length is decided once
//!
//! The audio thread may not allocate, so every sample this can ever hold is
//! taken at build time: eight seconds per channel, which is a bar at thirty beats
//! a minute and longer than anything a loop this tool cuts will ask for. A delay
//! that grew its line when you turned the time up would allocate inside the
//! callback the operating system is waiting on, which is a dropout with a good
//! excuse.
//!
//! # Why the time arrives in samples
//!
//! Because two different questions produce it and neither belongs here. "An
//! eighth of a bar" needs the loop's length and the speed it is being played at;
//! "three hundred milliseconds" needs neither. The caller knows both, works out
//! the number of output samples, and this module counts them — so a delay locked
//! to the grid follows the varispeed without this file knowing what a varispeed
//! is.
//!
//! # Why the writes are collected first
//!
//! Ping-pong crosses the feedback: what channel one heard is what channel two
//! repeats. Written straight into the lines, the order the channels happen to be
//! processed in would decide the result — one channel would overwrite the slot
//! the other had just crossed into. The per-frame writes are accumulated and
//! flushed in [`advance`](Delay::advance), so the answer does not depend on
//! which channel went first.

// The delay is the sample domain.
#![allow(clippy::float_arithmetic)]

/// The longest delay that can ever be asked for.
const MAX_SECONDS: f64 = 8.0;

/// What the delay is set to.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct DelaySettings {
    /// How much of the repeat is added to the dry signal. Zero is off, and off
    /// exactly — the line is not even read.
    pub mix: f64,
    /// The time, in output samples. See the module docs for why it is not a
    /// tempo and not a number of milliseconds.
    pub samples: f64,
    /// How much of each repeat comes back round, `0.0` to `0.95`.
    pub feedback: f64,
    /// A lowpass in the feedback path, `0.0` to `1.0`. Zero is no filter at all,
    /// so the repeats stay as bright as the source; turned up, each pass loses a
    /// little more top, which is what makes a long tail sit behind a loop
    /// instead of fighting it.
    pub damping: f64,
    /// Whether the feedback crosses to the next channel.
    pub ping_pong: bool,
    /// Holds what is in the line and stops letting anything new in.
    pub freeze: bool,
}

impl DelaySettings {
    /// Whether this is indistinguishable from a wire.
    ///
    /// Freeze counts as on even at no mix: a held line you cannot hear is still
    /// a held line, and letting it decay because the mix was down would lose the
    /// thing the moment you reached for the knob.
    pub fn is_wire(&self) -> bool {
        self.mix <= 0.0 && !self.freeze
    }
}

pub struct Delay {
    /// One line per channel, `f32` because a delay line is storage rather than
    /// arithmetic — every sum still happens in `f64`, and halving this halves
    /// the only large allocation the effect makes.
    lines: Vec<Vec<f32>>,
    /// Where the next frame is written.
    write: usize,
    /// What this frame wants written, per channel, before anything is.
    pending: Vec<f64>,
    /// The feedback lowpass, one per channel.
    damp: Vec<f64>,
    settings: DelaySettings,
    /// The time in whole samples, already brought inside the line.
    taps: usize,
    /// The one-pole coefficient the damping works out to.
    damp_coefficient: f64,
}

impl Delay {
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        let length = ((f64::from(sample_rate.max(1)) * MAX_SECONDS) as usize).max(2);
        Delay {
            lines: vec![vec![0.0; length]; channels],
            write: 0,
            pending: vec![0.0; channels],
            damp: vec![0.0; channels],
            settings: DelaySettings::default(),
            taps: 1,
            damp_coefficient: 1.0,
        }
    }

    /// Takes a whole panel position. Clamped here, for the same reason the
    /// filter's is: this comes off a control surface.
    ///
    /// Feedback stops at 0.95 rather than 1.0. A delay at unity feedback never
    /// decays, and one a hair past it doubles every pass — which is not a long
    /// tail, it is a siren. [`freeze`](DelaySettings::freeze) is the honest way
    /// to ask for forever, and it says so on the switch.
    pub fn set(&mut self, settings: DelaySettings) {
        let longest = self.lines.first().map_or(1, |line| line.len()) - 1;
        let samples = if settings.samples.is_finite() {
            settings.samples
        } else {
            0.0
        };
        self.taps = (samples as usize).clamp(1, longest);
        let damping = sane(settings.damping).clamp(0.0, 1.0);
        // Zero damping is no filter at all rather than a filter at the top of
        // its range: the repeats have to be able to come back exactly as bright
        // as they went in.
        self.damp_coefficient = 1.0 - damping * 0.97;
        self.settings = DelaySettings {
            mix: sane(settings.mix).clamp(0.0, 1.0),
            samples,
            feedback: sane(settings.feedback).clamp(0.0, 0.95),
            damping,
            ping_pong: settings.ping_pong,
            freeze: settings.freeze,
        };
    }

    /// What it actually took, after the clamps.
    pub fn settings(&self) -> DelaySettings {
        self.settings
    }

    pub fn is_wire(&self) -> bool {
        self.settings.is_wire()
    }

    /// Empties the line. Only for a preview that has stopped.
    pub fn reset(&mut self) {
        for line in self.lines.iter_mut() {
            line.fill(0.0);
        }
        for (state, pending) in self.damp.iter_mut().zip(self.pending.iter_mut()) {
            *state = 0.0;
            *pending = 0.0;
        }
    }

    /// One sample of one channel. Returns the dry signal with the repeat on it.
    ///
    /// The repeat is *added* rather than crossfaded with the dry, because a
    /// delay is a send and not an insert: turning it up should put echoes behind
    /// what you already had, not take away the thing they are echoes of.
    pub fn process(&mut self, channel: usize, input: f64) -> f64 {
        if channel >= self.lines.len() || self.is_wire() {
            return input;
        }
        let length = self.lines[channel].len();
        let read = (self.write + length - self.taps) % length;
        let delayed = f64::from(self.lines[channel][read]);

        // Freeze skips the damping. A hold that got darker every pass would be
        // a long fade with a switch on it, and the switch says freeze.
        let returned = if self.settings.freeze {
            delayed
        } else {
            self.damp[channel] += self.damp_coefficient * (delayed - self.damp[channel]);
            self.damp[channel]
        };

        // Where the feedback goes. Crossed, what one channel heard is what the
        // next one repeats; the dry always goes into the channel it came from,
        // so the source stays where it was and only the echoes travel.
        let target = if self.settings.ping_pong {
            (channel + 1) % self.lines.len()
        } else {
            channel
        };
        if self.settings.freeze {
            self.pending[target] += returned;
        } else {
            self.pending[channel] += input;
            self.pending[target] += self.settings.feedback * returned;
        }

        input + self.settings.mix * delayed
    }

    /// Closes the frame: writes what the channels asked for and moves on.
    ///
    /// Must be called once per frame, after every channel. See the module docs
    /// for why the writes wait until here.
    pub fn advance(&mut self) {
        if self.is_wire() {
            return;
        }
        let write = self.write;
        for (line, pending) in self.lines.iter_mut().zip(self.pending.iter_mut()) {
            line[write] = *pending as f32;
            *pending = 0.0;
        }
        self.write = (write + 1) % self.lines[0].len();
    }
}

fn sane(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Feeds one sample of 1.0 and then silence, and returns what came out of
    /// `channel` over `frames`.
    fn ping(delay: &mut Delay, channels: usize, frames: usize, channel: usize) -> Vec<f64> {
        let mut out = Vec::with_capacity(frames);
        for frame in 0..frames {
            let mut heard = 0.0;
            for c in 0..channels {
                let input = if frame == 0 && c == 0 { 1.0 } else { 0.0 };
                let value = delay.process(c, input);
                if c == channel {
                    heard = value;
                }
            }
            delay.advance();
            out.push(heard);
        }
        out
    }

    fn at(out: &[f64], index: usize) -> f64 {
        out.get(index).copied().unwrap_or(0.0)
    }

    #[test]
    fn a_delay_that_is_off_is_a_wire() {
        let mut delay = Delay::new(2, RATE);
        assert!(delay.is_wire());
        let out = ping(&mut delay, 2, 64, 0);
        assert_eq!(out[0], 1.0);
        assert!(out[1..].iter().all(|&s| s == 0.0), "an idle delay made sound");
    }

    #[test]
    fn the_repeat_comes_back_where_it_was_asked_for() {
        let mut delay = Delay::new(1, RATE);
        delay.set(DelaySettings {
            mix: 1.0,
            samples: 10.0,
            feedback: 0.0,
            ..DelaySettings::default()
        });
        let out = ping(&mut delay, 1, 40, 0);
        assert_eq!(at(&out, 0), 1.0, "the dry sample did not come straight out");
        assert!((at(&out, 10) - 1.0).abs() < 1e-9, "the repeat is at {}", at(&out, 10));
        assert_eq!(at(&out, 9), 0.0);
        assert_eq!(at(&out, 11), 0.0);
        assert_eq!(at(&out, 20), 0.0, "there was a second repeat at no feedback");
    }

    #[test]
    fn feedback_repeats_and_always_gets_quieter() {
        let mut delay = Delay::new(1, RATE);
        delay.set(DelaySettings {
            mix: 1.0,
            samples: 8.0,
            feedback: 0.5,
            ..DelaySettings::default()
        });
        let out = ping(&mut delay, 1, 64, 0);
        let repeats: Vec<f64> = (1..=4).map(|n| at(&out, 8 * n)).collect();
        assert!((repeats[0] - 1.0).abs() < 1e-9, "{repeats:?}");
        for pair in repeats.windows(2) {
            assert!(pair[1] < pair[0], "the tail did not decay: {repeats:?}");
            assert!(pair[1] > 0.0, "the tail stopped early: {repeats:?}");
        }

        // And the knob cannot be pushed to the place where it does not decay.
        delay.set(DelaySettings {
            mix: 1.0,
            samples: 8.0,
            feedback: 40.0,
            ..DelaySettings::default()
        });
        let out = ping(&mut delay, 1, 200, 0);
        assert!(at(&out, 160) < at(&out, 8), "runaway feedback");
    }

    #[test]
    fn ping_pong_sends_the_repeats_to_the_other_side() {
        // The dry stays where it came in and only the echoes travel: a delay
        // that moved the source would be a pan control with a strange name.
        let mut delay = Delay::new(2, RATE);
        delay.set(DelaySettings {
            mix: 1.0,
            samples: 6.0,
            feedback: 0.7,
            ping_pong: true,
            ..DelaySettings::default()
        });
        let left = {
            let mut fresh = Delay::new(2, RATE);
            fresh.set(DelaySettings {
                mix: 1.0,
                samples: 6.0,
                feedback: 0.7,
                ping_pong: true,
                ..DelaySettings::default()
            });
            ping(&mut fresh, 2, 40, 0)
        };
        let right = ping(&mut delay, 2, 40, 1);

        assert_eq!(at(&left, 0), 1.0, "the dry did not stay on the left");
        assert_eq!(at(&right, 0), 0.0, "the dry crossed over");
        // First repeat on the left, second on the right, and so on down.
        assert!(at(&left, 6) > 0.5, "the first repeat: {}", at(&left, 6));
        assert!(at(&right, 6) < 1e-9, "the first repeat crossed too early");
        assert!(at(&right, 12) > 0.3, "the second repeat: {}", at(&right, 12));
        assert!(at(&left, 12) < 1e-9, "the second repeat stayed put");
    }

    #[test]
    fn damping_takes_the_top_off_each_pass_and_zero_takes_nothing() {
        let settings = |damping| DelaySettings {
            mix: 1.0,
            samples: 4.0,
            feedback: 0.9,
            damping,
            ..DelaySettings::default()
        };

        // The filter is in the *feedback* path, so the first repeat is the
        // sample itself either way — it has not been round yet. That is the
        // whole point: a delay you can hear at all should not dull the first
        // thing it gives back.
        let mut plain = Delay::new(1, RATE);
        plain.set(settings(0.0));
        let dry = ping(&mut plain, 1, 40, 0);
        let mut damped = Delay::new(1, RATE);
        damped.set(settings(1.0));
        let dark = ping(&mut damped, 1, 40, 0);

        assert!((at(&dry, 4) - 1.0).abs() < 1e-9, "{}", at(&dry, 4));
        assert!((at(&dark, 4) - 1.0).abs() < 1e-9, "{}", at(&dark, 4));

        // By the second pass the damped one has lost most of it, and the plain
        // one has lost only what the feedback took.
        // A hair off 0.9 rather than 0.9: the line stores `f32`, and this is
        // the whole size of that decision.
        assert!((at(&dry, 8) - 0.9).abs() < 1e-6, "{}", at(&dry, 8));
        assert!(at(&dark, 8) < 0.2, "damping did nothing: {}", at(&dark, 8));
        assert!(at(&dark, 9) > 0.0, "the energy vanished instead of spreading");
    }

    #[test]
    fn freeze_holds_what_is_in_the_line_and_stops_listening() {
        let mut delay = Delay::new(1, RATE);
        delay.set(DelaySettings {
            mix: 1.0,
            samples: 10.0,
            feedback: 0.5,
            ..DelaySettings::default()
        });
        // Put something in.
        for frame in 0..10 {
            delay.process(0, if frame == 0 { 1.0 } else { 0.0 });
            delay.advance();
        }

        delay.set(DelaySettings {
            mix: 1.0,
            samples: 10.0,
            feedback: 0.5,
            freeze: true,
            ..DelaySettings::default()
        });
        // Now shout at it. Nothing new gets in, and what is in there stays.
        let mut heard = Vec::new();
        for frame in 0..100 {
            let value = delay.process(0, 0.9);
            if frame % 10 == 0 {
                heard.push(value - 0.9);
            }
            delay.advance();
        }
        let loudest = heard.iter().fold(0.0f64, |m, &s| m.max(s.abs()));
        assert!((loudest - 1.0).abs() < 1e-6, "the held sample became {loudest}");
        assert!(
            heard.iter().filter(|&&s| s.abs() > 0.5).count() >= 9,
            "the hold decayed: {heard:?}",
        );
    }

    #[test]
    fn a_time_longer_than_the_line_is_brought_inside_it() {
        let mut delay = Delay::new(1, RATE);
        delay.set(DelaySettings {
            mix: 1.0,
            samples: 1e12,
            feedback: 0.0,
            ..DelaySettings::default()
        });
        // Still a delay, still finite, and still inside the memory it owns.
        let out = ping(&mut delay, 1, 32, 0);
        assert!(out.iter().all(|s| s.is_finite()));

        delay.set(DelaySettings {
            mix: f64::NAN,
            samples: f64::NAN,
            feedback: f64::NAN,
            damping: f64::NAN,
            ..DelaySettings::default()
        });
        let out = ping(&mut delay, 1, 32, 0);
        assert!(out.iter().all(|s| s.is_finite()), "a NaN got into the line");
    }
}
