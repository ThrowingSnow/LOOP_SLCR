//! The preview handle: one loop, playing, reachable from two threads.
//!
//! # The problem this solves
//!
//! Playback is driven by an audio thread that must never block, and the rate is
//! set by a UI thread that has no idea when that thread is running. Both reach
//! the same loop. Getting this wrong does not produce a wrong number — it
//! produces a click, or a use-after-free, and both only show up on a device.
//!
//! # The arrangement
//!
//! Two channels, deliberately asymmetric:
//!
//! - **UI → audio: an atomic.** [`Handle::set_ratio`] stores a `f64` as bits and
//!   returns. It never locks, never allocates, and cannot be made to wait by an
//!   audio thread that is mid-block.
//! - **audio → UI: two more atomics.** Position and frames played are published
//!   after each block, so a UI can draw a play head without asking the audio
//!   thread for anything.
//!
//! The [`Preview`] itself sits behind a mutex, but only one thread ever takes
//! it: the one calling [`Handle::read`]. It is there so that this type is safe
//! Rust rather than an `UnsafeCell` with a comment, and it is uncontended by
//! construction — the price is a few nanoseconds of atomic exchange per block,
//! not per sample.
//!
//! # Lifetime
//!
//! [`create`] leaks a `Box` into a `jlong`; [`destroy`] takes it back. Using a
//! handle after destroying it is undefined behaviour and no check here can make
//! it otherwise, so the Kotlin side clears its field in the same breath. A zero
//! handle *is* checked, because that is the one bad value a caller can produce
//! by ordinary mistake rather than by ignoring the contract.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use loopslcr_core::ops::fx::{FxSettings, Mode as FxMode, Route};
use loopslcr_core::ops::preview::{Motion, Pair, Preview, Shape, DEFAULT_GLIDE_MS};
use loopslcr_core::pipeline;
use loopslcr_core::wav::Wav;

use crate::api;
use crate::json::Object;

pub struct Handle {
    /// What the UI wants, as `f64` bits. Written by the UI thread only.
    target: AtomicU64,
    /// The stepped displacement, packed — see [`pack_motion`]. Written by the UI
    /// thread only.
    ///
    /// Packed into one word rather than kept behind the mutex for the same
    /// reason the rate is: three fields set one at a time could be read by the
    /// audio thread half-changed, and half of a change is a grid the user never
    /// asked for. One store, one load, and the block either sees the whole new
    /// motion or the whole old one.
    motion: AtomicU64,
    /// The swap schedule, packed — see [`pack_pair`]. Written by the UI thread.
    pair: AtomicU64,
    /// The two level trims, as `f32` bits side by side. Written by the UI.
    gains: AtomicU64,
    /// The insert, in two words — see [`pack_filter`] and [`pack_drive`].
    ///
    /// Two rather than one because six fields do not fit in sixty-four bits, and
    /// two rather than the mutex because the mutex is held by the audio thread
    /// for the length of a block. Unlike the motion, a block that saw the new
    /// cutoff with the old resonance is harmless: every field here is a
    /// continuous knob whose values are each independently valid, so a torn read
    /// is a position the hand passed through, not a setting nobody chose.
    fx_filter: AtomicU64,
    fx_drive: AtomicU64,
    /// The trim on the sum, as `f32` bits. Written by the UI.
    ///
    /// Its own word rather than a third field beside the two loop gains, which
    /// would not fit: three `f32` are ninety-six bits. Nothing is lost by the
    /// split — the master is a separate decision from the balance between the
    /// loops, and a block that saw a new master beside an old balance is a
    /// console with two hands on it, which is a real thing and not a glitch.
    master_gain: AtomicU64,
    /// The loudest sample each loop contributed to the last block, as `f32`
    /// bits side by side. Written by the audio thread.
    peaks: AtomicU64,
    /// The loudest sample that left in the last block, after the master gain,
    /// as `f32` bits. Written by the audio thread.
    peak_master: AtomicU64,
    /// Whether a partner is installed and whether it is the one sounding, as
    /// 0/1. Written by the audio thread, so a UI can ask without taking the
    /// mutex the audio thread is holding for the length of a block.
    partnered: AtomicU64,
    on_second: AtomicU64,
    /// Where the play head is, as `f64` bits. Written by the audio thread only.
    position: AtomicU64,
    /// Where the audio being heard comes from, as `f64` bits. The same as
    /// `position` unless a motion is displacing it. Written by the audio thread.
    sounding: AtomicU64,
    /// Output frames produced. Written by the audio thread only.
    played: AtomicU64,
    inner: Mutex<Preview>,
    channels: usize,
    sample_rate: u32,
    frames: usize,
}

impl Handle {
    fn new(preview: Preview) -> Self {
        Handle {
            target: AtomicU64::new(1.0f64.to_bits()),
            motion: AtomicU64::new(0),
            pair: AtomicU64::new(0),
            gains: AtomicU64::new(pack_two(1.0, 1.0)),
            fx_filter: AtomicU64::new(pack_filter(0, 1_000.0, 0.0, 0)),
            fx_drive: AtomicU64::new(pack_two(0.0, 1.0)),
            master_gain: AtomicU64::new(u64::from(1.0f32.to_bits())),
            peaks: AtomicU64::new(0),
            peak_master: AtomicU64::new(0),
            partnered: AtomicU64::new(0),
            on_second: AtomicU64::new(0),
            position: AtomicU64::new(0.0f64.to_bits()),
            sounding: AtomicU64::new(0.0f64.to_bits()),
            played: AtomicU64::new(0),
            channels: preview.channel_count(),
            sample_rate: preview.sample_rate(),
            frames: preview.frames(),
            inner: Mutex::new(preview),
        }
    }

    /// Asks for a speed. Callable from any thread, at any time.
    ///
    /// `Relaxed` because there is nothing to order it against: the value stands
    /// alone, a block that reads the old one simply glides from a rate one
    /// buffer stale, and no reader can tell which of two adjacent requests it
    /// saw. Paying for an acquire/release pair here would buy nothing.
    pub fn set_ratio(&self, ratio: f64) {
        self.target.store(ratio.to_bits(), Ordering::Relaxed);
    }

    /// Asks for a stepped displacement, or for none. Callable from any thread.
    ///
    /// `steps` of zero, or `on` false, means none. An unknown shape number is
    /// treated as the first one rather than refused: this is a control surface,
    /// and the audio thread is the last place to start reporting errors.
    pub fn set_motion(&self, on: bool, steps: u32, depth: u32, every: u32, shape: u32) {
        self.motion.store(
            pack_motion(on, steps, depth, every, shape),
            Ordering::Relaxed,
        );
    }

    /// Sets the level trim for each loop, linear. Callable from any thread.
    ///
    /// Both in one word: set one at a time, the audio thread could read a block
    /// with the new first gain and the old second one — a momentary balance
    /// nobody asked for, right where a swap makes it audible.
    pub fn set_gains(&self, first: f32, second: f32) {
        self.gains
            .store(pack_two(first, second), Ordering::Relaxed);
    }

    /// Sets the insert. Callable from any thread.
    ///
    /// `mode` and `route` are numbers rather than enums because they come from
    /// Java, where they are ordinals; an unknown one becomes the harmless
    /// choice rather than an error, for the same reason an unknown motion shape
    /// does — the audio thread is the last place to start reporting faults.
    pub fn set_fx(&self, mode: u32, cutoff: f32, resonance: f32, route: u32, drive: f32, output: f32) {
        self.fx_filter
            .store(pack_filter(mode, cutoff, resonance, route), Ordering::Relaxed);
        self.fx_drive
            .store(pack_two(drive, output), Ordering::Relaxed);
    }

    /// Sets the trim on the sum, linear. Callable from any thread.
    pub fn set_master_gain(&self, gain: f32) {
        self.master_gain
            .store(u64::from(gain.to_bits()), Ordering::Relaxed);
    }

    /// The loudest sample each loop contributed to the last block, after gain.
    pub fn peaks(&self) -> (f32, f32) {
        unpack_two(self.peaks.load(Ordering::Relaxed))
    }

    /// The loudest sample that left in the last block, after the master gain.
    pub fn master_peak(&self) -> f32 {
        f32::from_bits(self.peak_master.load(Ordering::Relaxed) as u32)
    }

    /// Sets or clears the swap schedule. Callable from any thread.
    pub fn set_pair(&self, on: bool, steps: u32, hold_a: u32, hold_b: u32) {
        self.pair
            .store(pack_pair(on, steps, hold_a, hold_b), Ordering::Relaxed);
    }

    /// Gives the preview a second loop, or takes it away.
    ///
    /// **Not lock-free, and not for the audio thread.** It runs the pipeline,
    /// which takes as long as a cut does. The run happens *outside* the mutex
    /// and only the swap is inside it, for the same reason `start` builds before
    /// it installs: a lock held across a pipeline run would stall the audio
    /// thread for the length of a cut, which is a dropout, not a delay.
    ///
    /// The varispeed *is* applied here, unlike [`create`]. The partner has to
    /// come out at the first loop's own tempo and length, and the handle's live
    /// ratio then moves both together — one ratio, one clock, nothing to drift.
    pub fn set_partner(&self, bytes: &[u8], name: &str, params: &str) -> Result<(), String> {
        let wav = Wav::parse(bytes).map_err(|e| e.to_string())?;
        let (params, _) = api::params_from_json(params)?;
        let outcome = pipeline::run(&wav, name, &params).map_err(|e| e.to_string())?;
        if outcome.buffer.frames() == 0 {
            return Err("the second loop is empty".to_string());
        }

        let wanted = self.frames;
        let got = outcome.buffer.frames();
        let mut preview = self.inner.lock().map_err(|_| "the preview is gone")?;
        let outcome_result = preview.set_partner(Some(outcome.buffer));
        self.partnered
            .store(u64::from(preview.has_partner()), Ordering::Relaxed);
        outcome_result.map_err(|why| {
            // The numbers, not just the complaint. "A different length" leaves
            // the user with nothing to change; "352 800 against 344 000" tells
            // them which loop is wrong and by how much.
            format!("{why} — {got} frames against {wanted}")
        })
    }

    pub fn clear_partner(&self) {
        if let Ok(mut preview) = self.inner.lock() {
            let _ = preview.set_partner(None);
            self.partnered.store(0, Ordering::Relaxed);
            self.on_second.store(0, Ordering::Relaxed);
        }
    }

    /// Whether a partner is installed, as last published by the audio thread.
    ///
    /// Read from an atomic rather than from the preview, so a UI polling thirty
    /// times a second never waits on the lock the audio thread holds.
    pub fn has_partner(&self) -> bool {
        self.partnered.load(Ordering::Relaxed) != 0
    }

    /// Whether the second loop is the one being heard.
    pub fn on_second(&self) -> bool {
        self.on_second.load(Ordering::Relaxed) != 0
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
    }

    /// Where the audio being heard comes from, in source frames.
    pub fn sounding(&self) -> f64 {
        f64::from_bits(self.sounding.load(Ordering::Relaxed))
    }

    pub fn played(&self) -> u64 {
        self.played.load(Ordering::Relaxed)
    }

    /// Fills `out` with interleaved `f32` frames. For the audio thread.
    ///
    /// A poisoned mutex — which can only mean a previous read panicked — is
    /// answered with silence rather than a second panic. The audio thread is the
    /// worst possible place to unwind from, and silence is at least a defined
    /// sound.
    pub fn read(&self, out: &mut [f32]) -> usize {
        let Ok(mut preview) = self.inner.lock() else {
            out.fill(0.0);
            return 0;
        };
        preview.set_target_ratio(f64::from_bits(self.target.load(Ordering::Relaxed)));
        preview.set_motion(unpack_motion(self.motion.load(Ordering::Relaxed)));
        preview.set_pair(unpack_pair(self.pair.load(Ordering::Relaxed)));
        let (first, second) = unpack_two(self.gains.load(Ordering::Relaxed));
        preview.set_gains(f64::from(first), f64::from(second));
        preview.set_fx(unpack_fx(
            self.fx_filter.load(Ordering::Relaxed),
            self.fx_drive.load(Ordering::Relaxed),
        ));
        preview.set_master_gain(f64::from(f32::from_bits(
            self.master_gain.load(Ordering::Relaxed) as u32,
        )));
        let frames = preview.read(out);
        let (peak_first, peak_second) = preview.peaks();
        self.peaks
            .store(pack_two(peak_first, peak_second), Ordering::Relaxed);
        self.peak_master
            .store(u64::from(preview.master_peak().to_bits()), Ordering::Relaxed);
        self.position
            .store(preview.position().to_bits(), Ordering::Relaxed);
        self.sounding
            .store(preview.sounding_position().to_bits(), Ordering::Relaxed);
        self.played.store(preview.played(), Ordering::Relaxed);
        self.partnered
            .store(u64::from(preview.has_partner()), Ordering::Relaxed);
        self.on_second
            .store(u64::from(preview.on_second()), Ordering::Relaxed);
        frames
    }

    pub fn seek(&self, frame: f64) {
        if let Ok(mut preview) = self.inner.lock() {
            preview.seek(frame);
            self.position
                .store(preview.position().to_bits(), Ordering::Relaxed);
            self.sounding
                .store(preview.sounding_position().to_bits(), Ordering::Relaxed);
        }
    }

    /// What the caller needs to configure an `AudioTrack`, as JSON.
    pub fn info(&self) -> String {
        let mut out = Object::new();
        out.integer("channels", self.channels as i64)
            .integer("sampleRate", self.sample_rate as i64)
            .integer("frames", self.frames as i64)
            .number("position", self.position())
            .number("sounding", self.sounding())
            .integer("played", self.played() as i64)
            .bool("hasPartner", self.has_partner())
            .bool("onSecond", self.on_second())
            .number("peakFirst", f64::from(self.peaks().0))
            .number("peakSecond", f64::from(self.peaks().1))
            .number("peakMaster", f64::from(self.master_peak()));
        out.render()
    }
}

/// Packs a motion into one word.
///
/// The layout is an implementation detail of this file and its inverse — it
/// crosses no boundary but the atomic, and nothing outside is entitled to read
/// it. Bit 63 says whether there is a motion at all, so "off" is a distinct
/// value rather than a depth that happens to be zero.
fn pack_motion(on: bool, steps: u32, depth: u32, every: u32, shape: u32) -> u64 {
    if !on || steps == 0 {
        return 0;
    }
    // Clamped rather than refused. These arrive from a control surface, and a
    // depth wider than the loop is a request for something that does not exist,
    // not an error worth failing playback over.
    let steps = steps.min(4096);
    let depth = depth.min(steps.saturating_sub(1));
    // A rate slower than the whole loop would mean one move per pass, which is
    // no motion with extra steps; a rate of zero would mean dividing by it.
    let every = every.clamp(1, steps);
    // Masked, an unknown shape becomes whichever one its low bits name — a
    // shape nobody chose, arriving as a plausible one. Out of range means the
    // default, and it means it here rather than three layers down.
    let shape = if shape > LAST_SHAPE { 0 } else { shape };
    1 << 63
        | u64::from(steps)
        | u64::from(depth) << 16
        | u64::from(shape) << 32
        | u64::from(every) << 36
}

/// The filter half of the insert: cutoff, resonance, mode, route.
///
/// The cutoff keeps all thirty-two of its bits because it is the one a hand
/// sweeps and a coarse step in it is audible as a staircase. The resonance is
/// quantised to a sixteenth-bit of its range, which is four decimal places on a
/// knob that has two.
fn pack_filter(mode: u32, cutoff: f32, resonance: f32, route: u32) -> u64 {
    let resonance = if resonance.is_nan() { 0.0 } else { resonance };
    let quantised = (resonance.clamp(0.0, 1.0) * f32::from(u16::MAX)) as u16;
    u64::from(cutoff.to_bits()) << 32
        | u64::from(quantised) << 16
        // Out of range means off, and means it here rather than three layers
        // down. Masking instead would turn a number nobody sent into a mode
        // somebody appears to have chosen.
        | u64::from(if mode > 3 { 0 } else { mode }) << 8
        | u64::from(if route > 1 { 0 } else { route })
}

fn unpack_fx(filter: u64, drive: u64) -> FxSettings {
    let (drive, output) = unpack_two(drive);
    FxSettings {
        mode: match filter >> 8 & 0xFF {
            1 => FxMode::LowPass,
            2 => FxMode::HighPass,
            3 => FxMode::BandPass,
            _ => FxMode::Off,
        },
        cutoff_hz: f64::from(f32::from_bits((filter >> 32) as u32)),
        resonance: f64::from((filter >> 16 & 0xFFFF) as u16) / f64::from(u16::MAX),
        drive: f64::from(drive),
        output: f64::from(output),
        route: if filter & 0xFF == 1 {
            Route::DriveFirst
        } else {
            Route::FilterFirst
        },
    }
}

fn unpack_motion(bits: u64) -> Option<Motion> {
    if bits & 1 << 63 == 0 {
        return None;
    }
    Some(Motion {
        steps: (bits & 0xFFFF) as u32,
        depth: (bits >> 16 & 0xFFFF) as u32,
        every: (bits >> 36 & 0xFFFF) as u32,
        shape: match bits >> 32 & 0xF {
            1 => Shape::Fall,
            2 => Shape::Swing,
            3 => Shape::Scatter,
            4 => Shape::Walk,
            _ => Shape::Rise,
        },
    })
}

/// Packs a swap schedule into one word, for the same reason a motion is packed:
/// three fields set one at a time could be read half-changed, and half of a
/// swap schedule is an alternation nobody asked for.
fn pack_pair(on: bool, steps: u32, hold_a: u32, hold_b: u32) -> u64 {
    if !on || steps == 0 || hold_b == 0 {
        return 0;
    }
    let steps = steps.min(4096);
    let hold_a = hold_a.min(steps);
    let hold_b = hold_b.min(steps);
    1 << 63 | u64::from(steps) | u64::from(hold_a) << 16 | u64::from(hold_b) << 32
}

fn unpack_pair(bits: u64) -> Option<Pair> {
    if bits & 1 << 63 == 0 {
        return None;
    }
    Some(Pair {
        steps: (bits & 0xFFFF) as u32,
        hold_a: (bits >> 16 & 0xFFFF) as u32,
        hold_b: (bits >> 32 & 0xFFFF) as u32,
    })
}

/// Two `f32` in one word, so a pair of numbers that mean something together
/// cannot be read half-changed.
fn pack_two(first: f32, second: f32) -> u64 {
    u64::from(first.to_bits()) | u64::from(second.to_bits()) << 32
}

fn unpack_two(bits: u64) -> (f32, f32) {
    (
        f32::from_bits((bits & 0xFFFF_FFFF) as u32),
        f32::from_bits((bits >> 32) as u32),
    )
}

/// The highest shape number [`unpack_motion`] knows by name.
///
/// Kept beside the match it belongs to: a shape added there and not here would
/// arrive as `Rise` from every caller, silently.
const LAST_SHAPE: u32 = 4;

/// Builds a preview of the loop `params` describes.
///
/// **Varispeed is deliberately left out of it.** The pipeline is run with the
/// ratio neutralised, so what plays is the cut loop at its own tempo and the
/// speed is whatever the handle is told at the time. Baking the varispeed in
/// would mean rebuilding the whole thing on every touch of the control, which is
/// the opposite of what a preview is for.
///
/// Tape character *is* applied, at unity. The exported file applies it at the
/// final speed, where the filters sit lower — so a heavily transposed preview is
/// a little brighter than the render. Naming that is better than either
/// pretending otherwise or rebuilding on every drag.
pub fn create(bytes: &[u8], name: &str, params: &str) -> Result<Box<Handle>, String> {
    let wav = Wav::parse(bytes).map_err(|e| e.to_string())?;
    let (mut params, _) = api::params_from_json(params)?;
    params.ratio = None;
    params.target_bpm = None;
    params.snap = false;

    let outcome = pipeline::run(&wav, name, &params).map_err(|e| e.to_string())?;
    if outcome.buffer.frames() == 0 {
        return Err("nothing to preview — the cut is empty".to_string());
    }
    Ok(Box::new(Handle::new(Preview::new(
        outcome.buffer,
        DEFAULT_GLIDE_MS,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loopslcr_core::wav::{write, BitDepth, Metadata, WriteSpec};
    use loopslcr_core::AudioBuffer;

    const RATE: u32 = 8_000;
    const BAR: usize = 9_600;

    fn loop_file(bars: usize) -> Vec<u8> {
        let frames = bars * BAR;
        let data: Vec<f64> = (0..frames).map(|i| 0.25 * (i as f64 * 0.05).sin()).collect();
        let buffer = AudioBuffer::new(vec![data.clone(), data], RATE);
        write::write(
            &buffer,
            &WriteSpec {
                depth: BitDepth::Int24,
                metadata: Metadata::default(),
            },
        )
        .expect("could not encode the test loop")
    }

    #[test]
    fn the_insert_survives_the_two_words_it_crosses_in() {
        // What goes into the atomics has to come out the other side as the same
        // panel. A cutoff that arrived a few hertz off would be inaudible and a
        // route that arrived flipped would not be, so both are checked.
        let packed = pack_filter(2, 640.0, 0.5, 1);
        let settings = unpack_fx(packed, pack_two(0.75, 0.5));
        assert_eq!(settings.mode, FxMode::HighPass);
        assert_eq!(settings.route, Route::DriveFirst);
        assert_eq!(settings.cutoff_hz, 640.0);
        assert!((settings.resonance - 0.5).abs() < 1e-4, "{}", settings.resonance);
        assert_eq!(settings.drive, 0.75);
        assert_eq!(settings.output, 0.5);

        // Numbers Java could send that name nothing become the harmless choice
        // rather than whatever their low bits happen to spell.
        let odd = unpack_fx(pack_filter(99, 1_000.0, -1.0, 99), pack_two(0.0, 1.0));
        assert_eq!(odd.mode, FxMode::Off);
        assert_eq!(odd.route, Route::FilterFirst);
        assert_eq!(odd.resonance, 0.0);
    }

    #[test]
    fn a_handle_that_is_asked_for_a_filter_plays_through_it() {
        let handle = create(&loop_file(8), "200 loop.wav", "{}").expect("no handle");
        let mut out = vec![0.0f32; 4_096];

        handle.read(&mut out);
        let plain = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(plain > 0.01, "silence came out");

        // A highpass far above the test tone, which is a slow sine — what is
        // left of it should be very little.
        handle.set_fx(2, 4_000.0, 0.0, 0, 0.0, 1.0);
        handle.read(&mut out);
        handle.read(&mut out);
        let filtered = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(filtered < plain * 0.2, "{plain} became {filtered}");

        // And switching it off gives the sound back, through the same handle.
        handle.set_fx(0, 4_000.0, 0.0, 0, 0.0, 1.0);
        handle.read(&mut out);
        let again = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(again > plain * 0.8, "{plain} came back as {again}");
    }

    #[test]
    fn a_handle_plays_the_cut_loop_at_its_own_tempo() {
        let handle = create(&loop_file(8), "200 loop.wav", "{}").expect("no handle");
        assert!(handle.info().contains("\"sampleRate\":8000"));
        assert!(handle.info().contains("\"channels\":2"));

        let mut out = vec![0.0f32; 512];
        assert_eq!(handle.read(&mut out), 256);
        assert!(out.iter().any(|&s| s.abs() > 0.01), "silence came out");
        assert!((handle.position() - 256.0).abs() < 1e-6);
        assert_eq!(handle.played(), 256);
    }

    #[test]
    fn the_varispeed_in_the_parameters_is_not_baked_in() {
        // Asking for half speed and previewing must give the same audio as
        // asking for nothing: the rate is the handle's business, not the
        // pipeline's, or every drag would rebuild the loop.
        let bytes = loop_file(8);
        let plain = create(&bytes, "200 loop.wav", "{}").expect("no handle");
        let transposed =
            create(&bytes, "200 loop.wav", "{\"targetBpm\":100}").expect("no handle");
        assert_eq!(plain.info(), transposed.info());

        let mut a = vec![0.0f32; 256];
        let mut b = vec![0.0f32; 256];
        plain.read(&mut a);
        transposed.read(&mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn the_rate_is_picked_up_from_the_atomic_on_the_next_block() {
        let handle = create(&loop_file(8), "200 loop.wav", "{}").expect("no handle");
        handle.set_ratio(2.0);
        let mut out = vec![0.0f32; 200];
        handle.read(&mut out);
        // Gliding, so it has not arrived — but it has moved, which is what the
        // handoff has to prove.
        assert!(handle.position() > 100.0, "at {}", handle.position());
    }

    #[test]
    fn seeking_moves_the_play_head() {
        let handle = create(&loop_file(8), "200 loop.wav", "{}").expect("no handle");
        handle.seek(1234.0);
        assert!((handle.position() - 1234.0).abs() < 1e-9);
    }

    #[test]
    fn a_bad_file_is_an_error_rather_than_a_handle() {
        assert!(create(b"not a wave file", "x.wav", "{}").is_err());
    }

    #[test]
    fn an_unknown_parameter_is_refused_here_too() {
        let Err(e) = create(&loop_file(8), "200 loop.wav", "{\"targetBPM\":90}") else {
            panic!("a misspelled parameter produced a handle");
        };
        assert!(e.contains("targetBPM"), "{e}");
    }

    #[test]
    fn a_motion_survives_the_word_it_travels_in() {
        // The one place this can go wrong silently: a field that overlaps its
        // neighbour comes back as a plausible-looking grid rather than as an
        // error, and the loop plays a pattern nobody asked for.
        for shape in [
            Shape::Rise,
            Shape::Fall,
            Shape::Swing,
            Shape::Scatter,
            Shape::Walk,
        ] {
            for (steps, depth, every) in [
                (1u32, 0u32, 1u32),
                (4, 3, 2),
                (64, 7, 16),
                (4096, 4095, 4096),
            ] {
                let number = match shape {
                    Shape::Rise => 0,
                    Shape::Fall => 1,
                    Shape::Swing => 2,
                    Shape::Scatter => 3,
                    Shape::Walk => 4,
                };
                let there = pack_motion(true, steps, depth, every, number);
                let back = unpack_motion(there).expect("a motion went in");
                assert_eq!(back.steps, steps, "{shape:?} {steps}/{depth}/{every}");
                assert_eq!(back.depth, depth, "{shape:?} {steps}/{depth}/{every}");
                assert_eq!(back.every, every, "{shape:?} {steps}/{depth}/{every}");
                assert_eq!(back.shape, shape, "{shape:?} {steps}/{depth}/{every}");
            }
        }

        assert_eq!(unpack_motion(pack_motion(false, 8, 3, 1, 0)), None, "off");
        assert_eq!(unpack_motion(pack_motion(true, 0, 3, 1, 0)), None, "no grid");

        // A depth wider than the loop is clamped, not wrapped. Wrapped, it
        // would come back as a tiny depth and look like a working control.
        let wide = unpack_motion(pack_motion(true, 4, 99, 1, 0)).expect("a motion went in");
        assert_eq!(wide.depth, 3);

        // A rate of zero would be a division by it; slower than the loop is one
        // move per pass, which is no motion with extra steps.
        let none = unpack_motion(pack_motion(true, 4, 1, 0, 0)).expect("a motion went in");
        assert_eq!(none.every, 1);
        let slow = unpack_motion(pack_motion(true, 4, 1, 999, 0)).expect("a motion went in");
        assert_eq!(slow.every, 4);

        // An unknown shape is the first one rather than a panic on the audio
        // thread — this is a control surface, not a parser.
        let odd = unpack_motion(pack_motion(true, 4, 1, 1, 77)).expect("a motion went in");
        assert_eq!(odd.shape, Shape::Rise);
    }

    #[test]
    fn the_motion_reaches_the_audio_and_the_head_says_where_it_went() {
        let handle = create(&loop_file(8), "200 loop.wav", "{}").expect("no handle");
        let mut out = vec![0.0f32; 512];

        handle.read(&mut out);
        assert!(
            (handle.position() - handle.sounding()).abs() < 1e-9,
            "displaced before anything asked for it",
        );

        // Four pieces, jumping up to three of them. Checked as "at some point
        // during a pass" rather than "at the end of one": the pattern returns
        // to zero displacement once per cycle, and the first version of this
        // test happened to stop exactly there and called it a broken control.
        handle.set_motion(true, 4, 3, 1, 0);
        let mut furthest = 0.0f64;
        for _ in 0..(8 * BAR / 256 + 4) {
            handle.read(&mut out);
            furthest = furthest.max((handle.position() - handle.sounding()).abs());
        }
        assert!(
            furthest > 1.0,
            "the audio never left the clock; furthest was {furthest}",
        );

        handle.set_motion(false, 4, 3, 1, 0);
        for _ in 0..8 {
            handle.read(&mut out);
        }
        assert!(
            (handle.position() - handle.sounding()).abs() < 1e-9,
            "still displaced after being switched off",
        );
    }

    #[test]
    fn two_threads_may_touch_one_handle() {
        // The arrangement this module exists for, exercised: one thread reads
        // while another sets the rate. Under a data race this is what the
        // sanitiser would catch, and without the atomic it would not compile.
        let handle = std::sync::Arc::new(
            create(&loop_file(8), "200 loop.wav", "{}").expect("no handle"),
        );
        let setter = {
            let handle = handle.clone();
            std::thread::spawn(move || {
                for i in 0..1000 {
                    handle.set_ratio(1.0 + (i % 10) as f64 * 0.05);
                }
            })
        };
        let mut out = vec![0.0f32; 1024];
        for _ in 0..50 {
            handle.read(&mut out);
        }
        setter.join().expect("the setter panicked");
        assert!(handle.played() > 0);
    }
}
