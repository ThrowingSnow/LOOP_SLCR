//! A reverb, written out rather than depended on.
//!
//! # Why it is written here
//!
//! Because the core takes no dependencies, and a reverb is one of the few
//! effects where that is a real decision rather than a slogan. What is here is
//! Schroeder's arrangement in the shape Jezar's Freeverb made familiar: eight
//! comb filters in parallel, whose job is density, feeding four allpasses in
//! series, whose job is to smear what the combs left sounding like eight
//! separate echoes. Both parts are twenty lines. The tuning is the part worth
//! having, and it is public knowledge.
//!
//! # Why the buffer lengths are prime-ish and scaled
//!
//! The comb lengths are mutually indivisible on purpose: lengths that shared a
//! factor would put their echoes on top of each other and the tail would ring at
//! that period instead of filling in. They are given at 44.1 kHz and scaled to
//! whatever the file actually is, so a loop at 48 kHz gets the same *room* rather
//! than the same *sample counts* — the tuning is a set of times, not a set of
//! integers.
//!
//! # Why the two channels are not the same room
//!
//! Every buffer on the second channel is a couple of dozen samples longer. Two
//! identical mono reverbs fed the same signal produce the same tail twice, which
//! collapses to the middle and sounds like a mono reverb, because it is one.
//!
//! # Why there is a pre-delay
//!
//! A room does not answer instantly, and more usefully: a tail that starts on
//! the transient buries it. Holding the reverb off for a few tens of
//! milliseconds leaves the drum hit in the clear and puts the room behind it,
//! which is the difference between a loop in a space and a loop in a fog.

// The reverb is the sample domain.
#![allow(clippy::float_arithmetic)]

/// The tuning, in samples at 44.1 kHz. Eight combs, four allpasses.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
/// How much longer every buffer is on each channel after the first.
const SPREAD: usize = 23;
/// The reference rate the tuning above is given at.
const TUNED_AT: f64 = 44_100.0;
/// The longest pre-delay that can be asked for.
const MAX_PREDELAY_MS: f64 = 250.0;

/// What the reverb is set to.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct ReverbSettings {
    /// How much of the tail is added to the dry signal. Zero is off, exactly.
    pub mix: f64,
    /// How long the tail runs, `0.0` to `1.0`.
    pub size: f64,
    /// How much top each pass loses, `0.0` to `1.0`. A bright room and a dead
    /// one are the same shape with different damping.
    pub damping: f64,
    /// How long the room waits before answering, in milliseconds.
    pub predelay_ms: f64,
}

impl ReverbSettings {
    pub fn is_wire(&self) -> bool {
        self.mix <= 0.0
    }
}

/// One comb filter with a lowpass in its own feedback.
struct Comb {
    buffer: Vec<f32>,
    index: usize,
    filtered: f64,
}

impl Comb {
    fn new(length: usize) -> Self {
        Comb {
            buffer: vec![0.0; length.max(1)],
            index: 0,
            filtered: 0.0,
        }
    }

    fn process(&mut self, input: f64, feedback: f64, damp: f64) -> f64 {
        let out = f64::from(self.buffer[self.index]);
        // The damping is inside the loop, so every pass round the comb loses a
        // little more top. Outside it, one filter on the output would darken the
        // whole tail evenly and a room does not do that.
        self.filtered = out * (1.0 - damp) + self.filtered * damp;
        self.buffer[self.index] = (input + self.filtered * feedback) as f32;
        self.index = (self.index + 1) % self.buffer.len();
        out
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.filtered = 0.0;
    }
}

/// One allpass: passes everything, moves it about in time.
struct Allpass {
    buffer: Vec<f32>,
    index: usize,
}

impl Allpass {
    fn new(length: usize) -> Self {
        Allpass {
            buffer: vec![0.0; length.max(1)],
            index: 0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let stored = f64::from(self.buffer[self.index]);
        self.buffer[self.index] = (input + stored * 0.5) as f32;
        self.index = (self.index + 1) % self.buffer.len();
        stored - input
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
    }
}

/// One channel's room.
struct Room {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
    /// The pre-delay line, long enough for the most that can be asked for.
    early: Vec<f32>,
    early_index: usize,
}

impl Room {
    fn new(channel: usize, sample_rate: u32) -> Self {
        let scale = f64::from(sample_rate.max(1)) / TUNED_AT;
        let length = |tuning: usize| ((tuning as f64 * scale) as usize) + channel * SPREAD;
        let early = ((f64::from(sample_rate.max(1)) * MAX_PREDELAY_MS / 1000.0) as usize).max(2);
        Room {
            combs: COMBS.iter().map(|&n| Comb::new(length(n))).collect(),
            allpasses: ALLPASSES.iter().map(|&n| Allpass::new(length(n))).collect(),
            early: vec![0.0; early],
            early_index: 0,
        }
    }

    fn process(&mut self, input: f64, feedback: f64, damp: f64, predelay: usize) -> f64 {
        // The pre-delay first, so what the room hears is already late.
        let read = (self.early_index + self.early.len() - predelay) % self.early.len();
        let heard = f64::from(self.early[read]);
        self.early[self.early_index] = input as f32;
        self.early_index = (self.early_index + 1) % self.early.len();

        // Combs in parallel: density.
        let mut sum = 0.0;
        for comb in self.combs.iter_mut() {
            sum += comb.process(heard, feedback, damp);
        }
        // Allpasses in series: the smear that stops eight combs sounding like
        // eight echoes.
        let mut value = sum;
        for allpass in self.allpasses.iter_mut() {
            value = allpass.process(value);
        }
        value
    }

    fn reset(&mut self) {
        for comb in self.combs.iter_mut() {
            comb.reset();
        }
        for allpass in self.allpasses.iter_mut() {
            allpass.reset();
        }
        self.early.fill(0.0);
        self.early_index = 0;
    }
}

pub struct Reverb {
    rooms: Vec<Room>,
    settings: ReverbSettings,
    feedback: f64,
    damp: f64,
    predelay: usize,
    /// What the input is scaled by before the combs. Eight of them in parallel
    /// is eight times the signal, and the tail has to sit behind the loop rather
    /// than on top of it.
    input_gain: f64,
}

impl Reverb {
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        let mut reverb = Reverb {
            rooms: (0..channels).map(|c| Room::new(c, sample_rate)).collect(),
            settings: ReverbSettings::default(),
            feedback: 0.84,
            damp: 0.2,
            predelay: 1,
            input_gain: 0.015,
        };
        reverb.set(ReverbSettings::default(), sample_rate);
        reverb
    }

    /// Takes a whole panel position. Clamped, like every other set on this path.
    ///
    /// The feedback tops out below one. A comb at unity feedback never decays,
    /// and eight of them slightly past it is not a big room, it is an
    /// oscillator with a reverb's name on it.
    pub fn set(&mut self, settings: ReverbSettings, sample_rate: u32) {
        let mix = sane(settings.mix).clamp(0.0, 1.0);
        let size = sane(settings.size).clamp(0.0, 1.0);
        let damping = sane(settings.damping).clamp(0.0, 1.0);
        let predelay_ms = sane(settings.predelay_ms).clamp(0.0, MAX_PREDELAY_MS);

        self.feedback = 0.7 + size * 0.28;
        self.damp = damping * 0.4;
        let longest = self.rooms.first().map_or(2, |room| room.early.len()) - 1;
        self.predelay = ((predelay_ms * f64::from(sample_rate.max(1)) / 1000.0) as usize)
            .clamp(1, longest);
        self.settings = ReverbSettings {
            mix,
            size,
            damping,
            predelay_ms,
        };
    }

    /// What it actually took, after the clamps.
    pub fn settings(&self) -> ReverbSettings {
        self.settings
    }

    pub fn is_wire(&self) -> bool {
        self.settings.is_wire()
    }

    pub fn reset(&mut self) {
        for room in self.rooms.iter_mut() {
            room.reset();
        }
    }

    /// One sample of one channel: the dry signal with the room behind it.
    pub fn process(&mut self, channel: usize, input: f64) -> f64 {
        if channel >= self.rooms.len() || self.is_wire() {
            return input;
        }
        let tail = self.rooms[channel].process(
            input * self.input_gain,
            self.feedback,
            self.damp,
            self.predelay,
        );
        input + self.settings.mix * tail
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

    /// Hits it once and returns what came out of each channel.
    fn hit(reverb: &mut Reverb, channels: usize, frames: usize) -> Vec<Vec<f64>> {
        let mut out: Vec<Vec<f64>> = (0..channels).map(|_| Vec::with_capacity(frames)).collect();
        for frame in 0..frames {
            let input = if frame == 0 { 1.0 } else { 0.0 };
            for (c, channel) in out.iter_mut().enumerate() {
                channel.push(reverb.process(c, input));
            }
        }
        out
    }

    fn energy(samples: &[f64]) -> f64 {
        samples.iter().map(|s| s * s).sum()
    }

    #[test]
    fn a_reverb_at_no_mix_is_a_wire() {
        let mut reverb = Reverb::new(2, RATE);
        assert!(reverb.is_wire());
        let out = hit(&mut reverb, 2, 4_000);
        assert_eq!(out[0][0], 1.0);
        assert!(
            out[0][1..].iter().all(|&s| s == 0.0),
            "an idle reverb made a tail",
        );
    }

    #[test]
    fn one_hit_becomes_a_tail_that_arrives_late_and_dies_away() {
        let mut reverb = Reverb::new(2, RATE);
        reverb.set(
            ReverbSettings {
                mix: 1.0,
                size: 0.7,
                damping: 0.3,
                predelay_ms: 20.0,
            },
            RATE,
        );
        let out = hit(&mut reverb, 2, 96_000);
        let left = &out[0];

        // Nothing until the pre-delay is up. Measured against the same room
        // without one, because the shortest comb is already about 1200 samples
        // long — silence early on proves nothing on its own, and a test that
        // passes with the pre-delay removed is not testing the pre-delay.
        let mut immediate = Reverb::new(2, RATE);
        immediate.set(
            ReverbSettings {
                mix: 1.0,
                size: 0.7,
                damping: 0.3,
                predelay_ms: 0.0,
            },
            RATE,
        );
        let straight = hit(&mut immediate, 2, 96_000);
        // 20 ms is 960 samples, so the window runs past every comb the room has
        // and stops short of the wait.
        let waited = energy(&left[1..1_900]);
        let unwaited = energy(&straight[0][1..1_900]);
        assert!(waited < 1e-12, "the room answered before it was asked: {waited}");
        assert!(unwaited > 1e-9, "the window was too early to prove anything");

        // Then a tail.
        let started = energy(&left[1_000..10_000]);
        assert!(started > 1e-9, "there was no tail: {started}");

        // Which dies away rather than sitting there.
        let later = energy(&left[80_000..96_000]);
        assert!(later < started, "the tail did not decay: {started} then {later}");
        assert!(left.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn a_bigger_room_rings_for_longer() {
        let tail = |size| {
            let mut reverb = Reverb::new(1, RATE);
            reverb.set(
                ReverbSettings {
                    mix: 1.0,
                    size,
                    damping: 0.0,
                    predelay_ms: 0.0,
                },
                RATE,
            );
            let out = hit(&mut reverb, 1, 96_000);
            energy(&out[0][48_000..96_000])
        };
        let small = tail(0.0);
        let large = tail(1.0);
        assert!(large > small * 10.0, "small {small}, large {large}");
    }

    #[test]
    fn damping_makes_the_same_room_a_deader_one() {
        let tail = |damping| {
            let mut reverb = Reverb::new(1, RATE);
            reverb.set(
                ReverbSettings {
                    mix: 1.0,
                    size: 0.9,
                    damping,
                    predelay_ms: 0.0,
                },
                RATE,
            );
            let out = hit(&mut reverb, 1, 96_000);
            energy(&out[0][24_000..96_000])
        };
        assert!(tail(1.0) < tail(0.0) * 0.5, "{} against {}", tail(1.0), tail(0.0));
    }

    #[test]
    fn the_two_channels_are_not_the_same_room() {
        // Two identical mono reverbs fed the same thing produce the same tail
        // twice, which collapses to the middle. The spread is what stops that.
        let mut reverb = Reverb::new(2, RATE);
        reverb.set(
            ReverbSettings {
                mix: 1.0,
                size: 0.8,
                damping: 0.2,
                predelay_ms: 0.0,
            },
            RATE,
        );
        let out = hit(&mut reverb, 2, 48_000);
        let difference: f64 = out[0]
            .iter()
            .zip(out[1].iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(difference > 0.01, "both channels gave the same tail");
    }

    #[test]
    fn nothing_a_control_surface_can_send_makes_it_misbehave() {
        let mut reverb = Reverb::new(2, RATE);
        reverb.set(
            ReverbSettings {
                mix: f64::NAN,
                size: 1e9,
                damping: -4.0,
                predelay_ms: f64::INFINITY,
            },
            RATE,
        );
        assert!(reverb.is_wire(), "a NaN mix should read as off");

        reverb.set(
            ReverbSettings {
                mix: 1.0,
                size: 1e9,
                damping: -4.0,
                predelay_ms: f64::INFINITY,
            },
            RATE,
        );
        let out = hit(&mut reverb, 2, 48_000);
        assert!(out[0].iter().all(|s| s.is_finite()), "a NaN got into the room");
        // A size past the top is the top, not a runaway.
        let start = energy(&out[0][0..1_000]);
        let end = energy(&out[0][47_000..48_000]);
        assert!(end < start * 10.0, "the room ran away: {start} then {end}");
    }
}
