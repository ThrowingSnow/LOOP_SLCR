# LOOP_SLCR — Roadmap

> Name fixed: **LOOP_SLCR**. Crates are `loopslcr-core` / `loopslcr-cli` /
> `loopslcr-jni`, the binary is `loopslcr`.
>
> **Status:** M1 in progress — timing core and WAV reading are done and
> verified against the archive; the writer and `ops::cut` are next.
> Last updated 29.07.2026.

## Vision

A precision loop-trimming tool. Feed it a rendered drum loop with a warmup head
and an FX tail; it returns a sample-exact, seamless N-bar loop, optionally
transposed by tape varispeed, with the resulting tempo declared in the file.

Two front ends, one core:
- **`loopslcr` CLI** — Linux, batch-capable, the daily driver
- **Android APK** — two tabs (Cutter / Calculator), touch-first, tape-riding preview

Targets: **Linux CLI first, Android APK second.** macOS/Windows CLI come free.

---

## v0.1 — Core + CLI (offline, no Android)

The goal is a tool that is genuinely useful on the desktop before a single line
of Android code exists.

### Timing core — done

- [x] Cargo workspace scaffold (`loopslcr-core`, `loopslcr-cli`)
- [x] `Rational` — exact `i128` rational arithmetic, round-half-up
- [x] `TimeSignature`, `BpmUnit`, `Tempo`, `Grid`
- [x] `Grid::cut_sample(bar)` — exact cut points, no floats
- [x] `Grid::region(skip, bars, align)` — the index math behind `ops::cut`,
      with `Align::Loop` / `Align::Grid`
- [x] Residual error reporting (µs / ppm), for the cut-in and for the length
- [x] `Grid::sample_exact_bpms` — the nearest tempos needing no rounding
- [x] `Tempo` parsing: `103`, `103.5` and `207/2`, all held exactly
- [x] CLI: `loopslcr grid` — the part of `--dry-run` that needs no audio
- [x] Unit tests: known-good cut points for 4/4, 3/4, 7/8, 6/8 at 44.1/48 kHz
- [x] `#![deny(clippy::float_arithmetic)]` on the core — invariant 1 enforced by
      the build, with three individually justified display-only exceptions

### Audio — in progress

- [x] RIFF reader: PCM 16/24/32-bit int, 32/64-bit float, mono/stereo,
      WAVE_FORMAT_EXTENSIBLE — hand-rolled, zero runtime dependencies
- [x] Chunk layer shared with the writer: `fmt `, `data`, `acid`, `smpl`,
      `LIST`/`INFO`, unknown chunks skipped, odd sizes padded correctly
- [x] `AudioBuffer` — planar f64 internal representation
- [x] Verified against `hound` (dev-dependency only) on synthetic files and on
      the whole archive: 277 files, 177 790 491 frames, zero mismatches
- [x] CLI: `loopslcr info <file>`
- [x] `analysis::tail` — tail length via −60 dBFS threshold, plus workflow
      detection (path A / path B / already trimmed / unclear)
- [x] RIFF writer: 16/24/32-bit int and 32-bit float, `acid` + `smpl` +
      `LIST`/`INFO` written, honest RIFF size, tags after `data` so readers
      that assume audio at byte 44 still work
- [x] Verified in both directions: `hound` reads what we write, and a
      decode → encode round trip at the source depth is byte-identical
- [x] End-to-end on the real reference file: 8-bar region cut from the Caustic
      export and written back, 822 058 frames, sample-for-sample unchanged
- [x] `ops::cut` — apply `Grid::region` to an `AudioBuffer`, reporting a source
      too short rather than quietly delivering a loop that drifts
- [x] `ops::foldback` — `out[i % loopLen] += tail[i]`, multi-wrap safe, peak
      reported so an overshoot is visible instead of clipped
- [x] `ops::fade` — micro-fades, raised cosine, 0.5 ms default
- [x] `analysis::detect_workflow` — path A vs path B vs already trimmed
- [x] `analysis::guess_loop_bars` — loop length from the file's own duration
- [x] `naming` — tempo and bar count from the filename, the archive's primary
      tempo source since not one of its files carries an `acid` chunk
- [x] CLI: `loopslcr cut <file> [flags]`
- [x] **`--dry-run`** — cut points, residual in µs/ppm, tail length, shape; and
      it *reports* a short source where a real run refuses it
- [ ] `analysis::peaks` — min/max buckets for waveform display

**Exit criterion — met.** `--dry-run` over all 279 archive entries:

| | |
|---|---|
| processed | **261** |
| already trimmed | 248 |
| warmup renders (path A) | 8 |
| one loop plus tail (path B) | 5 |
| refused, with a named reason | 18 |

The 18: **14** carry no tempo in the name and no `acid` chunk, **2** are zip
archives, **2** are shorter than a single bar. No crash, no silent wrong answer.

**106 files are short of an exact loop** — by a median of 50 frames, worst case
19 518. These were trimmed by some earlier tool that rounded down, and 50 frames
per repeat is still drift, so a real run refuses them and names the deficit;
`--allow-short` accepts one knowingly.

The derived loop lengths match the archive's own convention: 4 bars dominates,
which is why there is no fixed `--bars` default any more.

The archive holds 279 entries: **277 are WAVE files** (two of them without a
`.wav` extension), plus two zip archives. The reader also sweeps all 277 —
see `tests/archive_sweep.rs`, gated behind `LOOPSLCR_ARCHIVE`.

---

## v0.2 — Varispeed + Output Formats

- [ ] `Resampler` trait
- [ ] `RubatoResampler` — fixed-ratio offline export path
- [ ] Anti-alias cutoff scaling: `cutoff = 0.5 / max(1, ratio)`
- [ ] `Taper` — semitone law + speed law, both directions
- [ ] Drive modes: pitch-driven, BPM-driven, snap-to-sample-exact
- [ ] `--pitch <st|cents>` / `--target-bpm <n>` / `--snap`
- [x] Bit depth selection: 16 / 24 / 32 / 32f (`BitDepth`, default 24)
- [ ] TPDF dither (noise shaping optional, later) — only place quantisation
      still rounds bare
- [x] `acid` chunk writer — tempo, beats, meter, loop flag
- [x] `smpl` chunk writer — loop points (spec-inclusive `end`), unity note
- [x] `LIST/INFO` → `ICMT` plain-text tag
- [ ] Filename template `{name}_{bpm}bpm_{bars}bars.wav`
- [ ] Peak check + overshoot warning (no auto-normalize)
- [ ] `--normalize` as an explicit opt-in

---

## v0.3 — Tape Character

- [ ] `TapeParams` + single bypass switch
- [ ] Wow & flutter LFO with **loop-periodic rate quantisation** (`k / loopDur`)
- [ ] Zero-mean guarantee over the loop period (unit-tested: output length invariant)
- [ ] Variable-rate resample path driven by the LFO
- [ ] HF rolloff, cutoff scaling with speed
- [ ] HF rolloff settling handled via double-pass warmup (keep second pass)
- [ ] Head bump — low shelf / peak 40–100 Hz, centre scaling with speed
- [ ] `--tape`, `--wow`, `--flutter`, `--hf-rolloff`
- [ ] Regression test: character bypassed → output bit-identical to v0.2

---

## v0.4 — Batch + Ergonomics

- [ ] `loopslcr batch <dir>` with rayon parallelism
- [ ] `--bpm-from-name` — regex `^(\d{2,3})\b` is not enough on its own. The
      archive holds `102-MTRX-01.wav` and `105CSTC-APRL02-...` (no word break
      after the number), `00005 136BPM E01...` (marker, not leading), and
      `58.5 DL_4BAR_...` (**decimal tempo** — truncating it to 58 puts the file
      off the grid). 14 files carry no tempo in the name at all and need it
      passed in.
- [ ] Per-directory preset file (`loopslcr.toml`)
- [ ] Progress reporting, per-file error collection, non-fatal continue
- [ ] `--out-dir` with structure preservation
- [ ] Shell completions (fish, bash, zsh)

**Milestone: the CLI is feature-complete and the archive is processable in one command.**

---

## v1.0 — Android APK

- [ ] `loopslcr-jni` cdylib, `cargo-ndk` integration
- [ ] Gradle ↔ cargo build wiring
- [ ] JNI surface: `analyze`, `process`, `previewCreate/Read/SetRatio/Seek/Destroy`
- [ ] Direct `ByteBuffer` transfer for PCM (no copies)
- [ ] SAF file picking (`ACTION_OPEN_DOCUMENT` / `ACTION_CREATE_DOCUMENT`), no broad permissions
- [ ] Compose UI shell, two tabs
- [ ] **Tab: CUTTER**
  - [ ] Waveform view from Rust peak buckets
  - [ ] Draggable / numeric cut markers, live overlay
  - [ ] BPM, signature, BPM unit, skip bars, loop bars
  - [ ] Tail mode selector with auto-detect suggestion
  - [ ] **Grid vs. loop priority — visible toggle**
  - [ ] Bipolar varispeed control, detent at 0
  - [ ] Live readout in all three units (st/cents, % speed, BPM)
  - [ ] Tape character panel + bypass
  - [ ] Export sheet: bit depth, dither, tags, filename template
- [ ] **Tab: CALCULATOR**
  - [ ] Beats/min, beats/bar, bars/min, beat length, bar length, Hz
  - [ ] Fraction table 1/16 … 16/16: percent, ms, Hz, **samples**
  - [ ] Dotted and triplet rows
  - [ ] Total duration for N bars
  - [ ] "→ send to Cutter" action
- [ ] Shared ViewModel: BPM, signature, BPM unit, sample rate
- [ ] **Preview engine**
  - [ ] `AudioTrack` streaming from Rust variable-rate resampler
  - [ ] Ratio glide via one-pole smoother in the audio thread (tape inertia)
  - [ ] Lock-free ratio handoff (atomic)
  - [ ] Seamless loop playback across the seam
- [ ] Dark theme
- [ ] GitHub Actions: signed release APK on tag push

---

## v1.1 — Saturation

- [ ] 4× oversampling stage (up/down polyphase filters)
- [ ] ADAA waveshaper — port from OktoTakt work
- [ ] Tape saturation model wired into the character chain
- [ ] Aliasing verification: sine sweep + spectrum check against the clean path

---

## v1.2 — Polish

- [ ] Noise-shaped dither option
- [ ] Undo/redo on parameter changes
- [ ] Recent files
- [ ] Presets for character + export settings
- [ ] A/B compare (original vs. processed) in preview
- [ ] Localisation DE/EN

---

## v2.0 — Beyond

- [ ] `cue` chunk with bar markers
- [ ] Slice export: 8×1 bar, 16×½ bar, arbitrary divisions
- [ ] Elektron-ready export: mono sum, SR/bit conversion for Digitakt 2
- [ ] Desktop GUI with drag & drop
- [ ] Own polyphase sinc resampler, A/B verified against `rubato`
- [ ] Optional automatic BPM detection from audio

---

## Milestones

| Milestone | Content | Status |
|---|---|---|
| M1 | v0.1 Core + CLI, exact cut math, dry-run over the archive | **IN PROGRESS** — timing core done, audio I/O next |
| M2 | v0.2 Varispeed, bit depth, dither, BPM tagging | TODO |
| M3 | v0.3 Tape character with loop-periodic modulation | TODO |
| M4 | v0.4 Batch processing, CLI feature-complete | TODO |
| M5 | v1.0 Android APK, two tabs, tape-riding preview | TODO |
| M6 | v1.1 Saturation (oversampling + ADAA) | TODO |
| M7 | v2.0 Slice export, Elektron export, desktop GUI | TODO |

---

## Open Questions

- ~~Project name~~ → **LOOP_SLCR**, crates `loopslcr-*`, binary `loopslcr`
- Micro-fades: default on (0.5 ms) or default off?
- Peak buckets: computed in Rust and passed over JNI, or computed in Kotlin?
- WAV reading: keep `hound`, or hand-rolled for zero dependency?
  (blocks the next task — the writer has to be hand-rolled either way for
  `acid`/`smpl`, so reading it too costs perhaps 200 lines and drops the
  dependency entirely)
- Wow/flutter default depths — and should character settings be presetable?
- iOS: worth it, or does the CLI plus Android cover the real workflow?
