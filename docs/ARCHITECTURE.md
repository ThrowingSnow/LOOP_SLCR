# LOOP_SLCR — Architecture

> Name fixed: **LOOP_SLCR**. The `loopslcr-*` crate names below are the real
> ones; the binary is `loopslcr`.

---

## 1. Design Principles

1. **The core knows nothing about UI.** No Android types, no `clap`, no JNI.
   `loopslcr-core` compiles and tests on any platform with zero platform code.
2. **Exact arithmetic in the timing domain.** All cut points derive from `i128`
   rationals. Floats appear only in the audio sample domain.
3. **One number crosses the varispeed boundary.** The core sees `ratio: f64`.
   Taper laws, semitones, percentages and target BPM are UI-side mappings.
4. **The clean path is bit-exactly reproducible.** With character bypassed, the
   same input and parameters must produce a byte-identical file, forever.
5. **The audio thread allocates nothing.** All preview buffers preallocated,
   parameter handoff lock-free.
6. **Every operation is a pure function on an `AudioBuffer`.** The pipeline is a
   composition, not a mutable god object.

---

## 2. Repository Layout

```
loopslcr/
├── Cargo.toml                    # workspace
├── crates/
│   ├── loopslcr-core/             # the entire brain — no platform deps
│   ├── loopslcr-cli/              # clap binary          ← v0.1
│   └── loopslcr-jni/              # cdylib for Android   ← v1.0
├── android/
│   ├── app/                      # Kotlin + Compose
│   └── build.gradle.kts          # cargo-ndk wiring
├── docs/
│   ├── BRAINSTORMING.md
│   ├── ROADMAP.md
│   └── ARCHITECTURE.md
└── .github/workflows/
    ├── ci.yml                    # cargo test + clippy
    └── release.yml               # signed APK on tag push
```

### `loopslcr-core` module tree

```
loopslcr-core/src/
├── lib.rs                 # public API surface
├── rational.rs            # exact i128 rational arithmetic
├── timing/
│   ├── mod.rs
│   ├── signature.rs       # TimeSignature, BpmUnit
│   ├── tempo.rs           # Tempo
│   └── grid.rs            # Grid — the cut-point authority
├── wav/
│   ├── mod.rs
│   ├── read.rs            # RIFF parser
│   ├── write.rs           # RIFF writer
│   └── chunks.rs          # AcidChunk, SmplChunk, InfoChunk
├── buffer.rs              # AudioBuffer (planar f64)
├── naming.rs              # tempo + bar count from a filename
├── analysis.rs            # Tail, Workflow, guess_loop_bars
├── ops/
│   ├── mod.rs
│   ├── cut.rs
│   ├── foldback.rs
│   ├── fade.rs
│   ├── resample/
│   │   ├── mod.rs         # Resampler trait
│   │   ├── rubato.rs      # fixed-ratio, offline export
│   │   └── varirate.rs    # variable-rate, realtime preview
│   ├── tape.rs            # wow/flutter, hf rolloff, head bump
│   ├── gain.rs            # peak analysis, normalize
│   └── dither.rs          # TPDF
├── analysis.rs            # peak buckets, tail detect, workflow detect
├── params.rs              # all parameter structs
├── pipeline.rs            # the ordered chain
└── preview.rs             # PreviewEngine (owned by the audio thread)
```

---

## 3. Core Types

### 3.1 Exact arithmetic

```rust
/// Exact rational on i128. All timing math lives here.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rational { num: i128, den: i128 }   // always normalised, den > 0

impl Rational {
    pub fn new(num: i128, den: i128) -> Self;
    pub fn round_half_up(self) -> i128;
    // mul, div, add, sub — all exact, panic on overflow in debug
}
```

### 3.2 Timing

```rust
pub struct TimeSignature { pub num: u32, pub den: u32 }   // N/D

/// BPM reference unit as a fraction of a whole note. Default 1/4.
pub struct BpmUnit(pub Rational);

pub struct Tempo { pub bpm: Rational, pub unit: BpmUnit }

pub struct Grid {
    pub tempo: Tempo,
    pub sig: TimeSignature,
    pub sample_rate: u32,
}

impl Grid {
    /// Exact, never accumulated:
    ///   cutSample(i) = round( i · SR · 60 · N · b · q / (p · D · a) )
    pub fn cut_sample(&self, bar: u64) -> u64;
    pub fn samples_per_bar(&self) -> Rational;
    pub fn seconds_per_bar(&self) -> Rational;
    /// Rounding residual for `bars`, in ppm and microseconds.
    pub fn residual(&self, bars: u64) -> Residual;
    /// Nearest BPM values yielding integer sample counts for `bars`.
    pub fn sample_exact_bpms(&self, bars: u64, window: u32) -> Vec<u32>;
}
```

### 3.3 Audio buffer

```rust
/// Planar, f64 internally. Bit depth is an I/O concern only.
pub struct AudioBuffer {
    pub channels: Vec<Vec<f64>>,
    pub sample_rate: u32,
}
```

f64 internally: the whole chain is short, offline, and f64 removes any doubt
about accumulation in foldback summation and resampling.

### 3.4 Parameters

```rust
pub enum Align  { Loop, Grid }
pub enum TailMode { Discard, Foldback }

pub struct CutParams {
    pub grid: Grid,
    pub skip_bars: u64,      // default = loop_bars
    pub loop_bars: u64,
    pub align: Align,
    pub tail: TailMode,
    pub fade: Option<FadeParams>,
}

pub struct VarispeedParams { pub ratio: f64 }   // taper lives in the UI

pub struct TapeParams {
    pub enabled: bool,
    pub wow:     LfoParams,   // rate quantised to k / loop_duration
    pub flutter: LfoParams,
    pub hf_rolloff: bool,
    pub head_bump:  bool,
}

pub enum BitDepth { I16, I24, F32 }
pub enum Dither   { None, Tpdf, NoiseShaped }

pub struct OutputParams {
    pub bits: BitDepth,
    pub dither: Dither,
    pub tags: TagSet,          // acid | smpl | info
    pub normalize: Option<f64>,
    pub filename_template: String,
}

pub struct Job {
    pub cut: CutParams,
    pub speed: Option<VarispeedParams>,
    pub tape: Option<TapeParams>,
    pub out: OutputParams,
}
```

---

## 4. Signal Flow

```
                    ┌──────────────┐
   input.wav  ─────▶│  wav::read   │  RIFF → AudioBuffer (f64 planar)
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  ops::cut    │  exact rational boundaries
                    │              │  Align::Loop | Align::Grid
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ops::foldback │  optional — out[i % L] += tail[i]
                    │              │  multi-wrap safe, then peak check
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  ops::fade   │  optional micro-fades
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ops::resample │  varispeed — rubato, fixed ratio
                    │              │  cutoff = 0.5 / max(1, ratio)
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  ops::tape   │  optional — wow/flutter, HF, head bump
                    │              │  single bypass, LTI only in v1
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  ops::gain   │  peak analysis → warn or normalize
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │ ops::dither  │  TPDF, only when reducing bit depth
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
                    │  wav::write  │  + acid + smpl + LIST/INFO
                    └──────┬───────┘
                           │
                     output.wav
```

**Order is binding.** Cut and foldback operate in the original time domain where
the bar grid is exact; resampling afterwards costs one single rounding of the
output length. Dither is last, after normalisation, immediately before
quantisation.

---

## 5. Key Algorithms

### 5.1 Exact cut point

With BPM = `p/q`, BPM unit `U = a/b`, signature `N/D`, sample rate `SR`:

```
cutSample(i) = round( i · SR · 60 · N · b · q  /  (p · D · a) )
```

Single `i128` product, single division, round-half-up. Never accumulate per bar.

Reference: 103 BPM, 4/4, 44.1 kHz, i = 8
→ `8·44100·60·4·4 / (103·4·1) = 338688000 / 412 = 822058.25…` → **822058**

**Align::Loop** — `in = cut_sample(skip)`, `len = round(loop_bars · spb)`
**Align::Grid** — `in = cut_sample(skip)`, `out = cut_sample(skip + loop_bars)`

### 5.2 Foldback

```rust
for i in 0..tail_len {
    for ch in 0..channels {
        out[ch][i % loop_len] += tail[ch][i];
    }
}
```

Wraps automatically when the tail exceeds the loop length. Peak analysis runs
afterwards; on overshoot the user is warned and decides — no silent
normalisation.

### 5.3 Loop-periodic wow & flutter

To keep the output length invariant under modulation, the rate deviation must
integrate to zero over one loop period:

```
allowed rates:  f_k = k / loopDuration,   k ∈ ℕ
LFO:            zero-mean over its own period (sine or sum of sines)
```

Then `outputLength = round(oldLen / ratio)` still holds exactly and the seam
stays continuous.

For an 18.64 s loop the rate grid is 0.0536 Hz.
Wow (0.5–6 Hz) → k = 10…112. Flutter (6–100 Hz) → k = 112…1864.

*Unit test:* for every enabled character configuration, assert output length is
identical to the bypassed path.

### 5.4 HF rolloff settling

A filter whose cutoff tracks speed has a settling transient at the loop start.
Solved with the same trick the whole application is built around: run the loop
through the filter **twice**, discard the first pass, keep the second. The filter
state entering the kept pass is then the steady state.

### 5.5 Resampling

| Path | Implementation | Ratio |
|---|---|---|
| Export | `rubato`, sinc, long kernel | fixed |
| Preview | own phase accumulator, 16–32 tap windowed sinc | per-sample variable |

Anti-aliasing when pitching up: `cutoff = 0.5 / max(1, ratio)`.

### 5.6 Dither

TPDF, applied only when the target bit depth is lower than the working
precision. Amplitude 1 LSB peak-to-peak, generated as the sum of two independent
uniform RNG draws. Order: normalize → dither → quantise.

---

## 6. Preview Engine (Android)

```
┌─────────────┐   atomic f64    ┌──────────────────┐   PCM    ┌────────────┐
│  Compose UI │ ──────────────▶ │  PreviewEngine   │ ───────▶ │ AudioTrack │
│  (main)     │  target_ratio   │  (audio thread)  │  blocks  │            │
└─────────────┘                 └──────────────────┘          └────────────┘
```

- UI writes `target_ratio` into an `AtomicU64` (f64 bit pattern). No locks.
- The audio thread runs a **one-pole smoother** toward the target:

  ```
  current += (target - current) * coeff       // coeff from τ ≈ 80–200 ms
  ```

  **This one-pole is the entire tape-inertia feel.** The glide is not a UI
  animation; it is the actual playback rate moving with motor lag.

- `VariRateResampler` reads the preloaded loop buffer at `current` ratio via a
  phase accumulator, wrapping at the loop boundary so playback is seamless.
- Zero allocation in the read path. The loop buffer and all resampler state are
  allocated once in `previewCreate`.

---

## 7. JNI Boundary

Kept deliberately narrow. Five functions plus analysis.

```
analyze(path: String) -> String              // JSON: format, duration, peaks, tail, suggestion
process(jobJson: String) -> String           // JSON: output path, peak, residual, warnings

previewCreate(path: String, jobJson: String) -> i64   // handle
previewRead(handle: i64, buf: ByteBuffer, frames: i32) -> i32
previewSetRatio(handle: i64, ratio: f64)
previewSeek(handle: i64, frame: i64)
previewDestroy(handle: i64)
```

- Parameters cross as **JSON** — one serde struct, no per-field JNI plumbing,
  trivially versionable.
- PCM crosses via **direct `ByteBuffer`** — no copies, no JNI array pinning.
- Handles are opaque `i64` pointers into a Rust-side registry. Kotlin never sees
  a raw pointer semantically.
- Peak buckets are computed **in Rust** so the APK and the CLI `info` command
  report identical numbers.

---

## 8. Threading Model

| Thread | Owns | Constraints |
|---|---|---|
| **Main / UI** | Compose state, shared ViewModel | no file I/O, no DSP |
| **Worker** (`Dispatchers.Default`) | `process()` calls over JNI | may block, reports progress |
| **Audio** (`AudioTrack` writer) | `PreviewEngine` | **no allocation, no locks, no panics** |

The CLI is single-threaded per file; `batch` uses `rayon` across files.

`loopslcr-core` never spawns threads itself. Concurrency is a front-end concern.

---

## 9. Android UI Structure

```
┌──────────────────────────────────────────┐
│  [ CUTTER ]   [ CALCULATOR ]             │  ← tabs
├──────────────────────────────────────────┤
│                                          │
│   ▁▃▅█▇▅▃▁▂▄▆█▇▅▃▁▂▄▆█▇▅▃▁               │  waveform + markers
│   │                     │                │
│   in                    out              │
│                                          │
│   BPM 103   4/4   unit 1/4               │
│   skip 8 bars     loop 8 bars            │
│   align  [Loop | Grid]                   │  ← visible toggle
│   tail   [Discard | Foldback]  (auto: A) │
│                                          │
│   ◀────────────●────────────▶            │  varispeed, detent at 0
│   −2.34 st  ·  −12.6 %  ·  90.0 BPM      │  all three units, always
│                                          │
│   TAPE  [bypass]  wow ─── flutter ───    │
│                                          │
│   [ ▶ preview ]            [ export ]    │
└──────────────────────────────────────────┘
```

**Shared state across tabs** (single ViewModel): `bpm`, `signature`, `bpmUnit`,
`sampleRate`. The calculator sets them, the cutter uses them, plus an explicit
"send to Cutter" action. Without shared state this is two apps in one APK
instead of one tool.

---

## 10. Dependencies

| Crate | Purpose | Justification |
|---|---|---|
| `rubato` | offline resampling | high-quality sinc, and later a reference to A/B an own implementation against |
| `clap` (derive) | CLI | standard |
| `serde` + `serde_json` | JNI parameter transport | one struct, versionable |
| `rayon` | batch parallelism | v0.4 only |
| `jni` | Android bridge | `loopslcr-jni` only |
| `thiserror` | error types | small |

**Deliberately absent:** `symphonia` (decoder zoo for formats not needed),
`hound` (cannot write `acid`/`smpl` chunks — own RIFF writer instead).

`loopslcr-core` has **no** platform dependencies and compiles for any target.

---

## 11. Testing Strategy

| Layer | Approach |
|---|---|
| `Rational` | property tests — associativity, exact round-trip, overflow behaviour |
| `Grid` | golden values for 4/4, 3/4, 7/8, 6/8 × 44.1/48 kHz; the 103 BPM reference case |
| `ops::cut` | length invariants for both align modes |
| `ops::foldback` | synthetic impulse + known decay → verify circular convolution identity |
| `ops::tape` | **output length identical to bypassed path** for every LFO configuration |
| Clean path | golden-file test: byte-identical output across versions |
| Resampling | sine sweep in, spectrum out, assert no aliasing above the anti-alias cutoff |
| Integration | `--dry-run` across the 279-file archive, all cut points verified |

---

## 12. Invariants

These must hold at all times and are enforced by tests:

1. `cut_sample()` never accumulates — it is always a single exact expression.
2. With `TapeParams::enabled == false`, output is byte-identical to the pure
   varispeed path.
3. With character enabled, output **length** is identical to the bypassed path.
4. The preview audio thread performs no allocation, takes no lock, and cannot panic.
5. `loopslcr-core` contains no `cfg(target_os)` and no UI-facing types.
6. Foldback never silently alters gain — overshoot is reported, never fixed.
