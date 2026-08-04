//! Live playback of a loop at a speed that can change while it plays.
//!
//! This is the other resampler the [`Resampler`](super::resample::Resampler)
//! trait was written for. The offline one is given a target length and derives
//! the rate from it, because a loop that is one sample long drifts. Here there
//! is no target length at all: the rate is whatever the user's finger is doing,
//! it changes between one output sample and the next, and the output runs until
//! someone stops it.
//!
//! # What it is for
//!
//! Riding the varispeed by ear. A number that says `-2.336 st` is not the same
//! information as hearing the loop slow down, and the whole reason this tool
//! models tape rather than a pitch shifter is that the two are audibly
//! different. A preview that could only play at fixed rates would leave the
//! interesting part — the movement — untestable.
//!
//! # Why it does not glide instantly
//!
//! A rate that jumps discontinuously is not something tape can do, and it does
//! not sound like tape either: it sounds like a sample being retriggered. The
//! target rate is therefore approached through a one-pole smoother whose time
//! constant stands in for the inertia of a reel. Dragging a slider then bends
//! the pitch the way a hand on a capstan would.
//!
//! # Why there is no locking, no allocation and no error path
//!
//! [`Preview::read`] is written to be called from an audio callback. Anything
//! that can block, allocate or fail there produces a dropout, which is not a
//! degraded result but a click. So the buffer is owned up front, the output
//! slice belongs to the caller, and the only thing that can go wrong — a rate of
//! zero, a NaN from a control surface — is clamped rather than reported.
//!
//! Threading is deliberately *not* handled here. This type is plain `&mut self`
//! code with no atomics in it, so it can be tested like anything else; the
//! handoff between a UI thread and an audio thread belongs to whoever owns both,
//! which on Android is the JNI layer.

// Playback is the sample domain.
#![allow(clippy::float_arithmetic)]

use core::f64::consts::FRAC_PI_2;

use crate::buffer::AudioBuffer;
use crate::ops::resample::{Edge, SincResampler};

/// How far the rate may be pushed in either direction.
///
/// Two octaves down and one up. The lower bound matters more than the upper:
/// as the rate approaches zero the read position stops advancing and the loop
/// becomes a drone, which is a valid sound but not a valid *speed*, and dividing
/// by it would produce infinities in anything downstream that reasons about
/// length.
pub const MIN_RATIO: f64 = 0.25;
pub const MAX_RATIO: f64 = 2.0;

/// The default reel inertia, in milliseconds.
///
/// Long enough to be heard as a bend rather than a jump, short enough that the
/// loop arrives at the requested speed before the user has stopped listening
/// for it. Tape machines vary far more than this; the number is a feel, not a
/// measurement, and it is a parameter so it can be argued with.
pub const DEFAULT_GLIDE_MS: f64 = 120.0;

/// How the displacement moves from one step to the next.
///
/// All four are functions of the step index alone. That is the whole trick: the
/// index runs 0, 1, … and resets at the loop boundary, so whatever the shape
/// does, it does the same thing on the next time round. Nothing here may consult
/// a clock, a random number generator or how long playback has been running, or
/// the loop stops being a loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// Climbs a step at a time and drops back. Reads as an accelerating stutter.
    Rise,
    /// The same, downwards — each step reaches further back.
    Fall,
    /// Up and back down again. Nothing repeats twice in a row at the turn.
    Swing,
    /// Scattered, but the *same* scatter every time round.
    ///
    /// Each move is independent of the last, so it lurches: the interesting
    /// shape when the point is that you cannot predict the next piece.
    Scatter,
    /// A random walk: each move is a step away from the one before.
    ///
    /// Also the same every time round. The difference from [`Scatter`] is
    /// audible and is the whole reason it exists — neighbouring pieces stay
    /// near each other, so it wanders through the loop instead of shuffling it,
    /// and a phrase survives being moved.
    Walk,
}

/// A stepped displacement of the play head that always lands on the grid.
///
/// # What it is
///
/// The loop is divided into [`steps`](Motion::steps) equal pieces. On each piece
/// boundary the play head is displaced by a whole number of pieces — never a
/// fraction, never a millisecond — so the audio that comes out always starts
/// where a piece starts. Bars get rearranged into bars, beats into beats.
///
/// # Why it stays a loop
///
/// Two properties do it, and both are load-bearing:
///
/// - **The grid is the loop, divided.** Not a duration in seconds, not a rate in
///   hertz. A step is exactly `frames / steps`, so the grid closes at the loop
///   boundary with nothing left over — the seam this whole tool exists to keep
///   clean cannot be landed on from the wrong side.
/// - **The displacement is a pure function of the step index**, which resets
///   every time round. So the second pass through the loop is sample-identical
///   to the first. An LFO with a period of its own would beat against the loop
///   and produce something that never repeats, which is a fine effect and not
///   this one.
///
/// # Why it crossfades
///
/// A jump is a discontinuity in the waveform even when it is perfectly on the
/// grid: the sample before the jump and the sample after are unrelated, and the
/// step between them is a click. A few milliseconds of equal-power crossfade
/// covers it. Equal-power rather than linear because the two sides are
/// uncorrelated — a linear fade between uncorrelated signals dips in the middle,
/// which is audible as a hole exactly where the ear is already listening for a
/// transient.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Motion {
    /// How many equal pieces the loop is divided into. The grid.
    pub steps: u32,
    /// How far a displacement may reach, in pieces.
    pub depth: u32,
    /// How many pieces pass between one move and the next.
    ///
    /// The grid says where a jump may *land*; this says how often it happens.
    /// They were one control to begin with, which meant asking for a fine
    /// landing grid also asked for a frantic rate — half-beat resolution could
    /// only ever be heard as a half-beat stutter.
    ///
    /// It is counted in pieces rather than in milliseconds, which is what makes
    /// the rate beat-synced by construction: there is no number here that can
    /// put a move between two beats. One means every piece.
    pub every: u32,
    pub shape: Shape,
}

/// Two loops, swapped on the grid.
///
/// # What it is
///
/// A second loop of exactly the same length, read at exactly the same phase.
/// The swap changes only *which buffer* the play head reads from — the head
/// itself keeps running — so bar three of one loop is followed by bar four of
/// the other, in time, without either loop being restarted.
///
/// # Why the phase is shared rather than synchronised
///
/// There is nothing to synchronise. Two clocks kept in step is a thing that can
/// drift, and a drift of a few samples per pass is exactly the artefact this
/// tool exists to remove. One clock cannot drift from itself.
///
/// That is what makes the length requirement absolute rather than fussy: the
/// partner must be the same number of frames as the loop it joins, or "the same
/// phase" stops meaning anything. Matching it is not a guess — both tempi are
/// known exactly, so the second loop is pulled to the first with the same exact
/// rational arithmetic as any other cut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pair {
    /// The loop divided into this many equal pieces, for the swap to land on.
    ///
    /// Its own grid rather than the motion's: the two are independent, and a
    /// swap is useful with no motion running at all.
    pub steps: u32,
    /// How many pieces to stay on the first loop.
    pub hold_a: u32,
    /// How many pieces to stay on the second.
    pub hold_b: u32,
}

impl Pair {
    /// Whether piece `index` belongs to the second loop.
    ///
    /// A function of the index, like everything else here, so the alternation
    /// repeats when the loop does.
    pub fn on_second(&self, index: u64) -> bool {
        let cycle = u64::from(self.hold_a) + u64::from(self.hold_b);
        if cycle == 0 || self.hold_b == 0 {
            return false;
        }
        index % cycle >= u64::from(self.hold_a)
    }
}

/// How long a jump takes to cross over, in milliseconds.
///
/// Short enough not to smear a transient, long enough to cover a step edge.
/// Below about two milliseconds the fade stops hiding the discontinuity and
/// starts merely shortening it.
pub const JUMP_FADE_MS: f64 = 4.0;

impl Motion {
    /// Which move a piece index belongs to.
    ///
    /// Pieces inside one move share a slot, so the displacement does not change
    /// under them — that is what a rate coarser than the grid *is*.
    pub fn slot(&self, index: u64) -> u64 {
        index / u64::from(self.every.max(1))
    }

    /// The displacement for move `slot`, in pieces.
    ///
    /// Deliberately total: a depth of zero gives zero everywhere rather than a
    /// division by zero, and any index works, including one past the end.
    pub fn offset(&self, index: u64) -> u64 {
        if self.depth == 0 {
            return 0;
        }
        let span = u64::from(self.depth) + 1;
        match self.shape {
            Shape::Rise => index % span,
            Shape::Fall => self.depth as u64 - (index % span),
            Shape::Swing => {
                // A triangle of period 2·depth, so the turn does not repeat the
                // end value twice — 0,1,2,1,0,1,2 rather than 0,1,2,2,1,0.
                let period = u64::from(self.depth) * 2;
                let j = index % period;
                if j <= u64::from(self.depth) {
                    j
                } else {
                    period - j
                }
            }
            // A hash rather than a generator: same index, same answer, forever.
            // Splitmix64's finaliser, which scatters adjacent integers well and
            // is four lines rather than a dependency.
            Shape::Scatter => hash(index) % span,
            // Walked from the beginning rather than carried in a field, so it
            // stays a pure function of the index — the property the whole
            // feature rests on. It costs `index` iterations, at most once per
            // move rather than once per sample, on an index that resets every
            // time round the loop.
            Shape::Walk => {
                let mut at = 0i64;
                let span = span as i64;
                for i in 0..=index {
                    // Reflected at the ends rather than wrapped. A wrap would
                    // teleport from one edge of the reach to the other, which
                    // is the lurch this shape exists not to do.
                    let step = if hash(i) & 1 == 0 { 1 } else { -1 };
                    at += step;
                    if at < 0 {
                        at = 1.min(span - 1);
                    } else if at >= span {
                        at = (span - 2).max(0);
                    }
                }
                at as u64
            }
        }
    }
}

/// Splitmix64's finaliser: same input, same answer, forever.
///
/// A hash rather than a generator, and that is the point — a generator would
/// answer differently on the second pass through the loop, which is exactly
/// what must not happen. Four lines rather than a dependency.
fn hash(index: u64) -> u64 {
    let mut x = index.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// A loop, playing.
///
/// Owns the audio it plays. The buffer is expected to be a finished loop — cut,
/// faded, and with whatever character was asked for already applied — because
/// the one thing being varied here is the speed.
pub struct Preview {
    buffer: AudioBuffer,
    reader: SincResampler,
    /// Fractional read position, always inside `[0, frames)`.
    position: f64,
    /// The rate actually being played, which chases `target`.
    ratio: f64,
    target: f64,
    /// One-pole coefficient for the glide, per output sample.
    glide: f64,
    /// Output frames produced since the last reset. Cheap play-head telemetry
    /// for a UI that wants to draw a cursor without asking the audio thread
    /// anything it would have to lock for.
    played: u64,
    /// The stepped displacement, when there is one.
    motion: Option<Motion>,
    /// The step the play head was in last block, so a boundary can be noticed.
    /// `u64::MAX` means "no step yet", which is not a step index anything can
    /// reach in the lifetime of a preview.
    step: u64,
    /// The displacement in frames actually being played, and the one being faded
    /// out of. Frames rather than steps, because a step is not an integer number
    /// of frames and rounding it twice would drift.
    displacement: f64,
    previous: f64,
    /// Output frames left in the crossfade, and how long it was.
    fade_left: f64,
    fade_length: f64,
    /// The second loop, when there is one. Guaranteed by [`set_partner`] to be
    /// the same shape as [`buffer`](Preview::buffer) — the phase is shared, so
    /// a partner of a different length is not a partner.
    partner: Option<AudioBuffer>,
    pair: Option<Pair>,
    /// The piece the swap grid is in, so a boundary can be noticed. Separate
    /// from [`step`](Preview::step), which counts the motion's own slots.
    piece: u64,
    /// Which loop is sounding, and which one a crossfade is leaving.
    second: bool,
    was_second: bool,
}

impl Preview {
    /// A preview of `buffer`, at unity, from the start.
    ///
    /// Fewer sinc taps than the offline path: eight rather than thirty-two. The
    /// stopband drops from about −90 dB to about −60, which is inaudible under a
    /// drum loop and costs a quarter of the work per sample — and unlike the
    /// offline path, this one has a deadline.
    pub fn new(buffer: AudioBuffer, glide_ms: f64) -> Self {
        let rate = buffer.sample_rate();
        Preview {
            buffer,
            reader: SincResampler::fast().with_edge(Edge::Wrap),
            position: 0.0,
            ratio: 1.0,
            target: 1.0,
            glide: glide_coefficient(glide_ms, rate),
            played: 0,
            motion: None,
            step: u64::MAX,
            displacement: 0.0,
            previous: 0.0,
            fade_left: 0.0,
            fade_length: (JUMP_FADE_MS * 0.001 * f64::from(rate)).max(1.0),
            partner: None,
            pair: None,
            piece: u64::MAX,
            second: false,
            was_second: false,
        }
    }

    /// Gives the preview a second loop to swap with, or takes it away.
    ///
    /// **The length, rate and channel count must match exactly.** Not a
    /// fussiness: the two are read at one shared phase, and a partner of a
    /// different length has no shared phase to be read at. Refused rather than
    /// stretched, because stretching it here would silently undo the exactness
    /// the rest of the tool is built on — the caller knows both tempi and can
    /// ask the pipeline for a partner that fits.
    pub fn set_partner(&mut self, partner: Option<AudioBuffer>) -> Result<(), &'static str> {
        let Some(buffer) = partner else {
            self.partner = None;
            if self.second {
                self.swap_to(false);
            }
            return Ok(());
        };
        if buffer.frames() != self.buffer.frames() {
            return Err("the second loop is a different length");
        }
        if buffer.channel_count() != self.buffer.channel_count() {
            return Err("the second loop has a different number of channels");
        }
        if buffer.sample_rate() != self.buffer.sample_rate() {
            return Err("the second loop has a different sample rate");
        }
        self.partner = Some(buffer);
        Ok(())
    }

    pub fn has_partner(&self) -> bool {
        self.partner.is_some()
    }

    /// Whether the second loop is the one being heard right now.
    pub fn on_second(&self) -> bool {
        self.second
    }

    pub fn pair(&self) -> Option<Pair> {
        self.pair
    }

    /// Sets or clears the swap schedule.
    ///
    /// Like [`set_motion`](Preview::set_motion), it lands at the next piece
    /// boundary rather than immediately — a swap in the middle of a beat is the
    /// one thing this feature exists not to do.
    pub fn set_pair(&mut self, pair: Option<Pair>) {
        let pair = pair.filter(|p| p.steps > 0);
        if pair == self.pair {
            return;
        }
        self.pair = pair;
        if pair.is_none() && self.second {
            self.swap_to(false);
        }
        self.piece = u64::MAX;
    }

    /// Starts a crossfade from one loop to the other.
    fn swap_to(&mut self, second: bool) {
        if second == self.second {
            return;
        }
        self.begin_fade();
        self.second = second;
    }

    /// Remembers what is being left, and starts the crossfade.
    ///
    /// One fade covers both a jump and a swap, because on a boundary where both
    /// happen there is only one discontinuity to hide — two overlapping fades
    /// would each be hiding half of it.
    ///
    /// Mid-fade the outgoing signal is already a mixture, so what is captured
    /// stays whatever the fade was already leaving. Capturing the current
    /// values there would restart the move that is still in progress and leave
    /// the fade chasing itself.
    fn begin_fade(&mut self) {
        if self.fade_left <= 0.0 {
            self.previous = self.displacement;
            self.was_second = self.second;
        }
        self.fade_left = self.fade_length;
    }

    pub fn channel_count(&self) -> usize {
        self.buffer.channel_count()
    }

    pub fn sample_rate(&self) -> u32 {
        self.buffer.sample_rate()
    }

    pub fn frames(&self) -> usize {
        self.buffer.frames()
    }

    /// Where the play head is, in source frames.
    ///
    /// The *clock*, not where the audio is being read from — see
    /// [`sounding_position`](Preview::sounding_position). This one advances
    /// evenly and is what a seek round-trips through; displacing it would make
    /// "stop, then resume where you were" mean somewhere the loop had jumped to.
    pub fn position(&self) -> f64 {
        self.position
    }

    /// Where the audio being heard is coming from, in source frames.
    ///
    /// The same as [`position`](Preview::position) with no motion. This is what
    /// a play head on a waveform should follow: it is the part of the file you
    /// can actually hear.
    pub fn sounding_position(&self) -> f64 {
        let frames = self.buffer.frames() as f64;
        if frames <= 0.0 {
            return 0.0;
        }
        (self.position + self.displacement).rem_euclid(frames)
    }

    pub fn motion(&self) -> Option<Motion> {
        self.motion
    }

    /// Sets or clears the stepped displacement.
    ///
    /// Takes effect at the next step boundary rather than immediately, which is
    /// what stops a turn of the depth control from being a click. Switching it
    /// off is the one exception: it crossfades straight back to zero, because
    /// "off" that waits for a boundary reads as a control that is not working.
    ///
    /// A motion with no steps is no motion — the grid it describes does not
    /// exist, and dividing the loop by zero is not a musical position.
    pub fn set_motion(&mut self, motion: Option<Motion>) {
        let motion = motion.filter(|m| m.steps > 0);
        if motion == self.motion {
            return;
        }
        self.motion = motion;
        if motion.is_none() {
            self.jump_to(0.0);
        }
        // Forget the step we were in, so the next block reads as a boundary and
        // the new grid is picked up there rather than half a step late.
        self.step = u64::MAX;
    }

    /// Starts a crossfade from wherever the head is to a new displacement.
    fn jump_to(&mut self, displacement: f64) {
        if displacement == self.displacement {
            return;
        }
        self.begin_fade();
        self.displacement = displacement;
    }

    /// The rate being played right now, which may still be gliding.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    pub fn target_ratio(&self) -> f64 {
        self.target
    }

    pub fn played(&self) -> u64 {
        self.played
    }

    /// Asks for a new speed. Reached over the glide time, not immediately.
    ///
    /// A NaN — which a control surface can produce from a text field mid-edit —
    /// leaves the target alone rather than poisoning every future sample.
    pub fn set_target_ratio(&mut self, ratio: f64) {
        if ratio.is_finite() {
            self.target = ratio.clamp(MIN_RATIO, MAX_RATIO);
        }
    }

    /// Sets the speed with no glide at all. For starting playback at a rate.
    pub fn snap_to_ratio(&mut self, ratio: f64) {
        self.set_target_ratio(ratio);
        self.ratio = self.target;
    }

    /// Moves the play head. Wraps, because the buffer is a loop.
    pub fn seek(&mut self, frame: f64) {
        let frames = self.buffer.frames() as f64;
        self.position = if frames > 0.0 && frame.is_finite() {
            frame.rem_euclid(frames)
        } else {
            0.0
        };
    }

    /// Fills `out` with interleaved frames and returns how many it wrote.
    ///
    /// `out.len()` must be a multiple of the channel count; the remainder is
    /// left untouched rather than half-filled. Allocation-free and infallible:
    /// see the module docs for why both matter here.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let channels = self.buffer.channel_count();
        let frames = self.buffer.frames();
        if channels == 0 || frames == 0 {
            out.fill(0.0);
            return 0;
        }

        let wanted = out.len() / channels;
        let length = frames as f64;

        // Hoisted: the grid cannot change inside a block, and this is a division
        // that would otherwise happen once per sample to produce the same
        // number every time.
        let step_frames = self
            .motion
            .map(|m| length / f64::from(m.steps))
            .unwrap_or(0.0);
        let pair_frames = self
            .pair
            .map(|p| length / f64::from(p.steps))
            .unwrap_or(0.0);

        for frame in 0..wanted {
            // Glide first, so the rate used for this sample is the one the
            // smoother has reached — the alternative leaves the output one
            // sample ahead of the rate that produced it.
            self.ratio += (self.target - self.ratio) * self.glide;

            // A boundary in the *clock*, not in the displaced head: the grid is
            // a property of the loop, so it has to be read off the thing that
            // moves through the loop evenly. Measuring it on the displaced head
            // would make each jump decide where the next one falls, and the
            // pattern would wander instead of repeating.
            if let Some(motion) = self.motion {
                if step_frames > 0.0 {
                    // The *slot*, not the piece: with a rate coarser than the
                    // grid, several pieces pass without a move, and the jump
                    // still lands on a piece because the displacement is
                    // counted in pieces.
                    let slot = motion.slot((self.position / step_frames) as u64);
                    if slot != self.step {
                        self.step = slot;
                        self.jump_to(motion.offset(slot) as f64 * step_frames);
                    }
                }
            }

            // The swap runs on its own grid, because it is useful with no
            // motion at all — and when both land on the same boundary, one
            // crossfade covers the pair. See `begin_fade`.
            if let Some(pair) = self.pair {
                if pair_frames > 0.0 {
                    let piece = (self.position / pair_frames) as u64;
                    if piece != self.piece {
                        self.piece = piece;
                        let want = self.partner.is_some() && pair.on_second(piece);
                        self.swap_to(want);
                    }
                }
            }

            let head = (self.position + self.displacement).rem_euclid(length);
            // Equal power, not linear: the two sides of a jump are unrelated
            // audio, and a linear fade between uncorrelated signals dips in the
            // middle — a hole exactly where the ear is listening for the beat.
            let crossing = if self.fade_left > 0.0 {
                let t = 1.0 - self.fade_left / self.fade_length;
                let old = (self.position + self.previous).rem_euclid(length);
                Some((old, (t * FRAC_PI_2).cos(), (t * FRAC_PI_2).sin()))
            } else {
                None
            };

            // Which loop each side of the fade reads from. Two field borrows
            // rather than one selected buffer, so the resampler — another
            // field — can still be borrowed mutably beside them.
            let arriving = match (self.second, self.partner.as_ref()) {
                (true, Some(partner)) => partner,
                _ => &self.buffer,
            };
            let leaving_from = match (self.was_second, self.partner.as_ref()) {
                (true, Some(partner)) => partner,
                _ => &self.buffer,
            };

            let base = frame * channels;
            for channel in 0..channels {
                let sample = self.reader.read(arriving.channel(channel), head, self.ratio);
                out[base + channel] = match crossing {
                    Some((old, out_gain, in_gain)) => {
                        let leaving =
                            self.reader
                                .read(leaving_from.channel(channel), old, self.ratio);
                        (leaving * out_gain + sample * in_gain) as f32
                    }
                    None => sample as f32,
                };
            }

            if self.fade_left > 0.0 {
                self.fade_left -= 1.0;
                if self.fade_left <= 0.0 {
                    self.fade_left = 0.0;
                    self.previous = self.displacement;
                    self.was_second = self.second;
                }
            }

            self.position += self.ratio;
            // Wrapping every sample rather than every block keeps the position
            // small, which matters: at 48 kHz a float position accumulating for
            // an hour would start losing fractional precision, and the seam is
            // exactly where that would be heard.
            if self.position >= length {
                self.position -= length;
            } else if self.position < 0.0 {
                self.position += length;
            }
        }

        self.played += wanted as u64;
        wanted
    }
}

/// One-pole coefficient reaching 1 − 1/e of the way in `ms`.
///
/// Clamped to something non-zero: a glide time of zero would mean an infinite
/// coefficient, and a negative one would make the smoother diverge.
fn glide_coefficient(ms: f64, sample_rate: u32) -> f64 {
    if !ms.is_finite() || ms <= 0.0 || sample_rate == 0 {
        return 1.0;
    }
    let samples = ms * 0.001 * f64::from(sample_rate);
    (1.0 - (-1.0 / samples).exp()).clamp(f64::MIN_POSITIVE, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// A loop whose sample value is its own index, so a read tells you exactly
    /// where the play head was.
    fn ramp(frames: usize, channels: usize) -> AudioBuffer {
        let data: Vec<Vec<f64>> = (0..channels)
            .map(|c| (0..frames).map(|i| (i + c * 1000) as f64).collect())
            .collect();
        AudioBuffer::new(data, RATE)
    }

    fn sine(frames: usize) -> AudioBuffer {
        let data: Vec<f64> = (0..frames)
            .map(|i| (i as f64 * 0.02).sin() * 0.5)
            .collect();
        AudioBuffer::new(vec![data.clone(), data], RATE)
    }

    #[test]
    fn at_unity_the_play_head_advances_one_frame_per_frame() {
        let mut preview = Preview::new(ramp(1000, 1), 0.0);
        let mut out = vec![0.0f32; 64];
        assert_eq!(preview.read(&mut out), 64);
        assert!((preview.position() - 64.0).abs() < 1e-9);
    }

    #[test]
    fn the_position_wraps_instead_of_running_off_the_end() {
        let mut preview = Preview::new(ramp(100, 1), 0.0);
        let mut out = vec![0.0f32; 250];
        preview.read(&mut out);
        // 250 frames through a 100-frame loop is two and a half times round.
        assert!((preview.position() - 50.0).abs() < 1e-9);
        assert_eq!(preview.played(), 250);
    }

    #[test]
    fn a_loop_read_twice_round_repeats_itself() {
        // The seam is the whole point: reading past the end must give the same
        // samples as reading from the start, or the preview clicks once per
        // repeat at exactly the place this tool exists to make clean.
        let mut preview = Preview::new(sine(512), 0.0);
        let mut first = vec![0.0f32; 512 * 2];
        let mut second = vec![0.0f32; 512 * 2];
        preview.read(&mut first);
        preview.read(&mut second);
        for (a, b) in first.iter().zip(second.iter()) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn double_speed_covers_the_loop_in_half_the_frames() {
        let mut preview = Preview::new(ramp(1000, 1), 0.0);
        preview.snap_to_ratio(2.0);
        let mut out = vec![0.0f32; 100];
        preview.read(&mut out);
        assert!((preview.position() - 200.0).abs() < 1e-9);
    }

    #[test]
    fn the_rate_glides_rather_than_jumping() {
        let mut preview = Preview::new(sine(4800), DEFAULT_GLIDE_MS);
        preview.set_target_ratio(2.0);
        assert_eq!(preview.ratio(), 1.0, "the target is not the rate");

        // One glide time in, a one-pole is 1 − 1/e ≈ 63.2 % of the way there.
        let mut out = vec![0.0f32; (RATE as usize / 1000 * 120) * 2];
        preview.read(&mut out);
        let travelled = (preview.ratio() - 1.0) / (2.0 - 1.0);
        assert!(
            (travelled - 0.632).abs() < 0.01,
            "expected ~63.2 % of the way, got {:.1} %",
            travelled * 100.0
        );
    }

    #[test]
    fn the_glide_gets_there_eventually() {
        let mut preview = Preview::new(sine(4800), DEFAULT_GLIDE_MS);
        preview.set_target_ratio(0.5);
        // Two seconds is about sixteen time constants. A one-pole never arrives
        // exactly, so the question is only whether it gets close enough that the
        // difference is far below anything audible — 1e-6 of a semitone is not
        // a pitch, it is a rounding difference.
        let mut out = vec![0.0f32; RATE as usize * 2 * 2];
        preview.read(&mut out);
        assert!(
            (preview.ratio() - 0.5).abs() < 1e-6,
            "still at {}",
            preview.ratio()
        );
    }

    #[test]
    fn a_glide_of_zero_is_no_glide() {
        let mut preview = Preview::new(sine(512), 0.0);
        preview.set_target_ratio(1.5);
        let mut out = vec![0.0f32; 2];
        preview.read(&mut out);
        assert_eq!(preview.ratio(), 1.5);
    }

    #[test]
    fn the_rate_is_clamped_and_nonsense_is_ignored() {
        let mut preview = Preview::new(sine(512), 0.0);
        preview.set_target_ratio(100.0);
        assert_eq!(preview.target_ratio(), MAX_RATIO);
        preview.set_target_ratio(0.0);
        assert_eq!(preview.target_ratio(), MIN_RATIO);

        preview.set_target_ratio(1.25);
        preview.set_target_ratio(f64::NAN);
        assert_eq!(preview.target_ratio(), 1.25, "a NaN must not poison the rate");
    }

    #[test]
    fn seeking_wraps_and_survives_nonsense() {
        let mut preview = Preview::new(sine(512), 0.0);
        preview.seek(600.0);
        assert!((preview.position() - 88.0).abs() < 1e-9);
        preview.seek(-1.0);
        assert!((preview.position() - 511.0).abs() < 1e-9);
        preview.seek(f64::NAN);
        assert_eq!(preview.position(), 0.0);
    }

    #[test]
    fn every_channel_gets_its_own_samples() {
        // Interleaving is the one thing a reader can get subtly wrong and still
        // produce sound: swapped channels are audible only on material that
        // differs between them.
        let mut preview = Preview::new(ramp(1000, 2), 0.0);
        let mut out = vec![0.0f32; 8];
        preview.read(&mut out);
        for frame in 0..4 {
            let left = out[frame * 2];
            let right = out[frame * 2 + 1];
            assert!((right - left - 1000.0).abs() < 1.0, "frame {frame}: {left} / {right}");
        }
    }

    #[test]
    fn a_partial_frame_at_the_end_is_left_alone() {
        let mut preview = Preview::new(ramp(1000, 2), 0.0);
        let mut out = vec![-7.0f32; 5];
        assert_eq!(preview.read(&mut out), 2, "two whole frames fit in five slots");
        assert_eq!(out[4], -7.0, "the odd slot is untouched, not half-written");
    }

    #[test]
    fn an_empty_buffer_plays_silence_rather_than_panicking() {
        let mut preview = Preview::new(AudioBuffer::silence(2, 0, RATE), 0.0);
        let mut out = vec![1.0f32; 8];
        assert_eq!(preview.read(&mut out), 0);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    // --- the stepped displacement -------------------------------------------

    #[test]
    fn the_second_time_round_the_loop_is_the_same_as_the_first() {
        // **The claim the whole feature rests on.** A displacement that moves
        // the play head around is only musically usable if the result still
        // repeats — otherwise it is not a loop with motion in it, it is a
        // generative patch that happens to use a loop.
        //
        // It holds because the grid is the loop divided into equal pieces and
        // the displacement is a function of the step index, which resets at the
        // seam. Nothing about elapsed time enters into it.
        //
        // The *first* pass is deliberately not part of the claim, and finding
        // out why was worth the test on its own: playback starts cold, so the
        // first step has nothing to cross-fade from, while every later pass
        // arrives at the seam fading out of the last step of the pass before.
        // A first pass identical to the rest would mean pretending audio had
        // been playing before it started. So the period is checked where a
        // period is a meaningful idea — between two consecutive later passes.
        for shape in [Shape::Rise, Shape::Fall, Shape::Swing, Shape::Scatter, Shape::Walk] {
            let mut preview = Preview::new(sine(4800), 0.0);
            preview.set_motion(Some(Motion {
                steps: 8,
                depth: 3,
                every: 1,
                shape,
            }));

            let mut cold = vec![0.0f32; 4800 * 2];
            let mut second = vec![0.0f32; 4800 * 2];
            let mut third = vec![0.0f32; 4800 * 2];
            preview.read(&mut cold);
            preview.read(&mut second);
            preview.read(&mut third);

            for (i, (a, b)) in second.iter().zip(third.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-6,
                    "{shape:?} differs at sample {i}: {a} vs {b}",
                );
            }
        }
    }

    #[test]
    fn the_grid_closes_at_the_seam() {
        // The other half of "always to the right position", and the half the
        // repetition test does *not* cover — that one passes for any grid at
        // all, because the step index is read off a position that wraps with
        // the loop, so the pattern restarts whatever the pieces are.
        //
        // What has to hold here is that the pieces tile the loop exactly. A
        // grid measured in milliseconds — the obvious way to build an LFO —
        // leaves a short piece at the end, and that short piece sits on the
        // seam this tool exists to keep clean.
        let frames = 4800usize;
        let steps = 8u32;
        let mut preview = Preview::new(sine(frames), 0.0);
        preview.set_motion(Some(Motion {
            steps,
            depth: 3,
            every: 1,
            shape: Shape::Rise,
        }));

        // One frame at a time, so a boundary is seen at the frame it happens.
        let mut boundaries = Vec::new();
        let mut out = vec![0.0f32; 2];
        let mut last = preview.step;
        for frame in 0..(frames * 2) {
            preview.read(&mut out);
            if preview.step != last {
                boundaries.push(frame);
                last = preview.step;
            }
        }

        let expected = frames / steps as usize;
        for pair in boundaries.windows(2) {
            assert_eq!(
                pair[1] - pair[0],
                expected,
                "a piece from {} to {} in a {frames}-frame loop divided {steps} ways",
                pair[0],
                pair[1],
            );
        }
        assert_eq!(boundaries.len(), (steps * 2) as usize, "{boundaries:?}");
    }

    #[test]
    fn every_displacement_is_a_whole_number_of_steps() {
        // "Always to the right position" means exactly this: a jump is a whole
        // piece of the grid, never a fraction of one and never a duration. A
        // displacement of two and a half beats would put the transient in the
        // middle of nowhere and no amount of crossfading would rescue it.
        let frames = 4800;
        let steps = 16;
        let step_frames = frames as f64 / steps as f64;

        let mut preview = Preview::new(sine(frames), 0.0);
        preview.set_motion(Some(Motion {
            steps,
            depth: 5,
            every: 1,
            shape: Shape::Scatter,
        }));

        let mut out = vec![0.0f32; 64 * 2];
        for _ in 0..(frames / 64) {
            preview.read(&mut out);
            let displacement = preview.displacement / step_frames;
            assert!(
                (displacement - displacement.round()).abs() < 1e-9,
                "displacement of {displacement} steps",
            );
        }
    }

    #[test]
    fn the_displacement_never_leaves_the_loop() {
        // It cannot read outside the cut. That is not a nicety — the buffer
        // *is* the loop, so a head outside it is either silence or a panic.
        let frames = 2400;
        let mut preview = Preview::new(sine(frames), 0.0);
        preview.set_motion(Some(Motion {
            steps: 8,
            depth: 7,
            every: 1,
            shape: Shape::Rise,
        }));

        let mut out = vec![0.0f32; 32 * 2];
        for _ in 0..200 {
            preview.read(&mut out);
            let head = preview.sounding_position();
            assert!(
                (0.0..frames as f64).contains(&head),
                "the head is at {head} in a {frames}-frame loop",
            );
        }
    }

    #[test]
    fn a_jump_crossfades_instead_of_clicking() {
        // A jump is a discontinuity even when it is perfectly on the grid: the
        // sample before and the sample after are unrelated audio. Without the
        // crossfade the step between them is a click, which is the one artefact
        // this whole tool exists to avoid.
        //
        // Measured as the largest step between adjacent output samples, against
        // the same loop playing straight. A click shows up here as a jump of
        // most of the signal's amplitude in one sample.
        let biggest_step = |motion: Option<Motion>| {
            let mut preview = Preview::new(sine(4800), 0.0);
            preview.set_motion(motion);
            let mut out = vec![0.0f32; 4800 * 2];
            preview.read(&mut out);
            out.chunks(2)
                .zip(out.chunks(2).skip(1))
                .fold(0.0f32, |worst, (a, b)| worst.max((b[0] - a[0]).abs()))
        };

        let straight = biggest_step(None);
        let moved = biggest_step(Some(Motion {
            steps: 8,
            depth: 3,
            every: 1,
            shape: Shape::Scatter,
        }));

        // Some excess is inevitable — a crossfade is not a splice — but it has
        // to stay in the neighbourhood of the signal's own slew rate rather
        // than in the neighbourhood of its amplitude.
        assert!(
            moved < straight * 4.0,
            "straight steps by at most {straight}, moved by {moved}",
        );
    }

    #[test]
    fn switching_the_motion_off_puts_the_head_back() {
        let mut preview = Preview::new(sine(4800), 0.0);
        preview.set_motion(Some(Motion {
            steps: 4,
            depth: 3,
            every: 1,
            shape: Shape::Rise,
        }));
        let mut out = vec![0.0f32; 2400 * 2];
        preview.read(&mut out);

        preview.set_motion(None);
        preview.read(&mut out);
        assert!(
            (preview.sounding_position() - preview.position()).abs() < 1e-9,
            "still displaced by {}",
            preview.sounding_position() - preview.position(),
        );
    }

    #[test]
    fn no_motion_plays_exactly_what_it_played_before() {
        // The feature must be free when it is off. Not approximately the same
        // audio — the same audio, sample for sample.
        let mut plain = Preview::new(sine(1024), 0.0);
        let mut idle = Preview::new(sine(1024), 0.0);
        idle.set_motion(Some(Motion {
            steps: 8,
            depth: 0,
            every: 1,
            shape: Shape::Scatter,
        }));

        let mut a = vec![0.0f32; 2048];
        let mut b = vec![0.0f32; 2048];
        plain.read(&mut a);
        idle.read(&mut b);
        assert_eq!(a, b, "a depth of zero is not silence, it is no motion");
    }

    #[test]
    fn a_grid_of_nothing_is_refused_rather_than_dividing_by_it() {
        let mut preview = Preview::new(sine(512), 0.0);
        preview.set_motion(Some(Motion {
            steps: 0,
            depth: 4,
            every: 1,
            shape: Shape::Rise,
        }));
        assert_eq!(preview.motion(), None);

        let mut out = vec![0.0f32; 64];
        preview.read(&mut out);
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn each_shape_is_a_function_of_the_step_alone() {
        // Which is what makes the loop repeat. Asked twice for the same index,
        // every shape has to answer the same thing — including the scattered
        // one, whose whole trick is being a hash rather than a generator.
        for shape in [Shape::Rise, Shape::Fall, Shape::Swing, Shape::Scatter, Shape::Walk] {
            let motion = Motion {
                steps: 16,
                depth: 4,
                every: 1,
                shape,
            };
            for index in 0..64u64 {
                assert_eq!(motion.offset(index), motion.offset(index), "{shape:?}");
                assert!(
                    motion.offset(index) <= 4,
                    "{shape:?} reached {} with a depth of 4",
                    motion.offset(index),
                );
            }
        }

        let rise = Motion {
            steps: 16,
            depth: 3,
            every: 1,
            shape: Shape::Rise,
        };
        assert_eq!(
            (0..8).map(|i| rise.offset(i)).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 0, 1, 2, 3],
        );

        let swing = Motion {
            steps: 16,
            depth: 2,
            every: 1,
            shape: Shape::Swing,
        };
        assert_eq!(
            (0..8).map(|i| swing.offset(i)).collect::<Vec<_>>(),
            vec![0, 1, 2, 1, 0, 1, 2, 1],
        );
    }

    #[test]
    fn a_rate_coarser_than_the_grid_moves_less_often_and_still_on_the_grid() {
        // The grid says where a jump may *land*; the rate says how often one
        // happens. They were one control, which meant asking for half-beat
        // landings also asked for a half-beat stutter — the fine resolution was
        // unusable at any musical rate.
        let frames = 4800usize;
        let steps = 16u32;
        let piece = frames as f64 / steps as f64;

        let moves = |every: u32| {
            let mut preview = Preview::new(sine(frames), 0.0);
            preview.set_motion(Some(Motion {
                steps,
                depth: 3,
                every,
                shape: Shape::Scatter,
            }));
            let mut out = vec![0.0f32; 2];
            let mut seen = Vec::new();
            let mut last = f64::NAN;
            for _ in 0..frames {
                preview.read(&mut out);
                if preview.displacement != last {
                    last = preview.displacement;
                    seen.push(last);
                }
            }
            seen
        };

        let fine = moves(1);
        let coarse = moves(4);
        assert!(
            coarse.len() * 2 < fine.len(),
            "every piece: {} moves, every fourth: {}",
            fine.len(),
            coarse.len(),
        );

        // And a coarser rate does not buy the fine grid away: the landings are
        // still whole pieces of it, which is the point of separating the two.
        for displacement in coarse {
            let pieces = displacement / piece;
            assert!(
                (pieces - pieces.round()).abs() < 1e-9,
                "landed {pieces} pieces along",
            );
        }
    }

    #[test]
    fn the_walk_wanders_where_the_scatter_lurches() {
        // The difference between the two random shapes, and the reason both
        // exist. Neighbouring moves of a walk stay near each other — a phrase
        // survives being moved — while a scatter is free to leap the whole
        // reach every time.
        let walk = Motion {
            steps: 64,
            depth: 7,
            every: 1,
            shape: Shape::Walk,
        };
        let scatter = Motion {
            shape: Shape::Scatter,
            ..walk
        };

        let jumpiness = |m: Motion| {
            (1..64u64)
                .map(|i| (m.offset(i) as i64 - m.offset(i - 1) as i64).abs())
                .sum::<i64>() as f64
                / 63.0
        };

        let walked = jumpiness(walk);
        let scattered = jumpiness(scatter);
        assert!(
            (walked - 1.0).abs() < 1e-9,
            "a walk moved by {walked} pieces on average, not one",
        );
        assert!(
            scattered > walked * 2.0,
            "walk {walked}, scatter {scattered} — they are the same shape",
        );

        // Still inside the reach, and still the same every time round.
        for i in 0..200u64 {
            assert!(walk.offset(i) <= 7, "reached {} of 7", walk.offset(i));
            assert_eq!(walk.offset(i), walk.offset(i));
        }
    }

    // --- the second loop ----------------------------------------------------

    /// A loop whose every sample is `value`, so which buffer is sounding can be
    /// read straight off the output.
    fn flat(frames: usize, value: f64) -> AudioBuffer {
        AudioBuffer::new(vec![vec![value; frames], vec![value; frames]], RATE)
    }

    #[test]
    fn the_swap_alternates_between_the_two_loops_on_the_grid() {
        // The feature, at its plainest: one loop of −0.5 and one of +0.5, so the
        // output says which one is playing at every sample. Four pieces, two on
        // each — the first half of the loop from one, the second from the other.
        let frames = 4800usize;
        let mut preview = Preview::new(flat(frames, -0.5), 0.0);
        preview
            .set_partner(Some(flat(frames, 0.5)))
            .expect("the partner is the same shape");
        preview.set_pair(Some(Pair {
            steps: 4,
            hold_a: 2,
            hold_b: 2,
        }));

        let mut out = vec![0.0f32; frames * 2];
        preview.read(&mut out);

        // Sampled well clear of the crossfades at the piece boundaries.
        let at = |fraction: f64| out[(frames as f64 * fraction) as usize * 2];
        assert!(at(0.1) < -0.4, "the first quarter is not the first loop");
        assert!(at(0.4) < -0.4, "the second quarter is not the first loop");
        assert!(at(0.6) > 0.4, "the third quarter is not the second loop");
        assert!(at(0.9) > 0.4, "the last quarter is not the second loop");
    }

    #[test]
    fn the_swap_keeps_one_clock_rather_than_two() {
        // Why the phase is shared and not synchronised: there is one play head,
        // and swapping changes only which buffer it reads. Two clocks kept in
        // step is a thing that can drift, and a drift of a few samples per pass
        // is the artefact this tool exists to remove.
        let frames = 2400usize;
        let mut preview = Preview::new(sine(frames), 0.0);
        preview
            .set_partner(Some(sine(frames)))
            .expect("the partner is the same shape");
        preview.set_pair(Some(Pair {
            steps: 8,
            hold_a: 1,
            hold_b: 1,
        }));

        let mut out = vec![0.0f32; frames * 2];
        preview.read(&mut out);
        assert!(
            preview.position().abs() < 1e-9,
            "one pass left the head at {}",
            preview.position(),
        );
    }

    #[test]
    fn a_partner_of_the_wrong_shape_is_refused_rather_than_stretched() {
        // Stretching it here would silently undo the exactness the rest of the
        // tool is built on. The caller knows both tempi and can ask the pipeline
        // for a partner that fits — this is the wrong layer to guess at one.
        let mut preview = Preview::new(sine(4800), 0.0);
        assert!(preview.set_partner(Some(sine(2400))).is_err(), "length");
        assert!(
            preview
                .set_partner(Some(AudioBuffer::new(vec![vec![0.0; 4800]], RATE)))
                .is_err(),
            "channels",
        );
        assert!(
            preview
                .set_partner(Some(AudioBuffer::new(
                    vec![vec![0.0; 4800], vec![0.0; 4800]],
                    RATE * 2,
                )))
                .is_err(),
            "sample rate",
        );
        assert!(!preview.has_partner(), "a refused partner was kept anyway");
    }

    #[test]
    fn the_pair_repeats_with_the_loop_like_everything_else_here() {
        let frames = 4800usize;
        let mut preview = Preview::new(sine(frames), 0.0);
        preview
            .set_partner(Some(flat(frames, 0.3)))
            .expect("the partner is the same shape");
        preview.set_pair(Some(Pair {
            steps: 8,
            hold_a: 3,
            hold_b: 1,
        }));

        let mut cold = vec![0.0f32; frames * 2];
        let mut second = vec![0.0f32; frames * 2];
        let mut third = vec![0.0f32; frames * 2];
        preview.read(&mut cold);
        preview.read(&mut second);
        preview.read(&mut third);
        for (i, (a, b)) in second.iter().zip(third.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "differs at {i}: {a} vs {b}");
        }
    }

    #[test]
    fn a_swap_crossfades_like_a_jump_does() {
        // Two loops at opposite polarity is the worst case there is: a bare
        // swap steps the whole amplitude in one sample.
        let frames = 4800usize;
        let biggest_step = |paired: bool| {
            let mut preview = Preview::new(flat(frames, -0.5), 0.0);
            if paired {
                preview.set_partner(Some(flat(frames, 0.5))).expect("shape");
                preview.set_pair(Some(Pair {
                    steps: 8,
                    hold_a: 4,
                    hold_b: 4,
                }));
            }
            let mut out = vec![0.0f32; frames * 2];
            preview.read(&mut out);
            out.chunks(2)
                .zip(out.chunks(2).skip(1))
                .fold(0.0f32, |worst, (a, b)| worst.max((b[0] - a[0]).abs()))
        };

        let swapped = biggest_step(true);
        assert!(
            swapped < 0.2,
            "a swap stepped by {swapped} of a 1.0 span in one sample",
        );
    }

    #[test]
    fn taking_the_partner_away_leaves_the_first_loop_playing() {
        let frames = 2400usize;
        let mut preview = Preview::new(flat(frames, -0.5), 0.0);
        preview.set_partner(Some(flat(frames, 0.5))).expect("shape");
        preview.set_pair(Some(Pair {
            steps: 4,
            hold_a: 1,
            hold_b: 3,
        }));
        let mut out = vec![0.0f32; frames * 2];
        preview.read(&mut out);
        assert!(preview.on_second(), "the second loop never came in");

        preview.set_partner(None).expect("removing one always works");
        preview.read(&mut out);
        assert!(!preview.on_second(), "still on a partner that is gone");
        assert!(
            out.iter().all(|&s| s < -0.4),
            "something other than the first loop is playing",
        );
    }

    #[test]
    fn a_pair_with_no_second_half_never_swaps() {
        let frames = 2400usize;
        let mut preview = Preview::new(flat(frames, -0.5), 0.0);
        preview.set_partner(Some(flat(frames, 0.5))).expect("shape");
        preview.set_pair(Some(Pair {
            steps: 4,
            hold_a: 4,
            hold_b: 0,
        }));
        let mut out = vec![0.0f32; frames * 2];
        preview.read(&mut out);
        assert!(out.iter().all(|&s| s < -0.4), "it swapped with no hold");
    }

    #[test]
    fn the_scattered_shape_actually_scatters() {
        // A hash that returned a constant would pass every determinism test in
        // this file and be nothing at all.
        let motion = Motion {
            steps: 32,
            depth: 7,
            every: 1,
            shape: Shape::Scatter,
        };
        let seen: std::collections::BTreeSet<u64> = (0..32).map(|i| motion.offset(i)).collect();
        assert!(seen.len() >= 5, "only {} distinct offsets", seen.len());
    }

    #[test]
    fn playing_slower_does_not_alias_and_playing_faster_does_not_either() {
        // A ramp is the worst case for aliasing: its wrap is a step edge. What
        // is checked here is only that nothing leaves the range the source
        // occupies by more than the overshoot a windowed sinc is allowed —
        // a resampler that dropped its anti-aliasing shows up as excursions
        // several times the source amplitude.
        for ratio in [0.25, 0.5, 1.0, 1.5, 2.0] {
            let mut preview = Preview::new(sine(1024), 0.0);
            preview.snap_to_ratio(ratio);
            let mut out = vec![0.0f32; 4096];
            preview.read(&mut out);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak < 0.6, "ratio {ratio}: peak {peak}");
            assert!(peak > 0.4, "ratio {ratio}: peak {peak} — nothing came out");
        }
    }
}
