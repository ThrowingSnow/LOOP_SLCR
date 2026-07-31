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

use loopslcr_core::ops::preview::{Preview, DEFAULT_GLIDE_MS};
use loopslcr_core::pipeline;
use loopslcr_core::wav::Wav;

use crate::api;
use crate::json::Object;

pub struct Handle {
    /// What the UI wants, as `f64` bits. Written by the UI thread only.
    target: AtomicU64,
    /// Where the play head is, as `f64` bits. Written by the audio thread only.
    position: AtomicU64,
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
            position: AtomicU64::new(0.0f64.to_bits()),
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

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
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
        let frames = preview.read(out);
        self.position
            .store(preview.position().to_bits(), Ordering::Relaxed);
        self.played.store(preview.played(), Ordering::Relaxed);
        frames
    }

    pub fn seek(&self, frame: f64) {
        if let Ok(mut preview) = self.inner.lock() {
            preview.seek(frame);
            self.position
                .store(preview.position().to_bits(), Ordering::Relaxed);
        }
    }

    /// What the caller needs to configure an `AudioTrack`, as JSON.
    pub fn info(&self) -> String {
        let mut out = Object::new();
        out.integer("channels", self.channels as i64)
            .integer("sampleRate", self.sample_rate as i64)
            .integer("frames", self.frames as i64)
            .number("position", self.position())
            .integer("played", self.played() as i64);
        out.render()
    }
}

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
