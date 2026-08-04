# LOOP_SLCR — Roadmap

> Name fixed: **LOOP_SLCR**. Crates are `loopslcr-core` / `loopslcr-cli` /
> `loopslcr-jni`, the binary is `loopslcr`.
>
> **Status:** M1–M4 done, v0.2 now complete including noise-shaped dither.
> The whole chain runs:
> read → cut → foldback → fade → varispeed → tape character → normalize → dither
> → write, and `loopslcr batch` puts the 279-file archive through it in 1.3
> seconds. M5 is under way: the Android toolchain is installed, `loopslcr-jni`
> is done including the preview engine, and the APK builds, installs and runs —
> both tabs work end to end, the loop can be ridden by ear, the cut markers drag
> against the bar grid, and the shrunk 2.4 MB release build passes the whole
> instrumented suite. Last updated 01.08.2026.

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

### Audio — done

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
- [x] `analysis::peaks` — min/max buckets for waveform display, plus an ASCII
      waveform in `info --waveform` so the code is exercised, not just written

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

- [x] `Resampler` trait
- [x] `SincResampler` — **hand-rolled, not rubato.** Windowed sinc,
      Blackman–Harris, 32 zero crossings. Two things rubato cannot do: take the
      output length as an *input*, and read the kernel **circularly** so the
      seam keeps its level. rubato stays a dev-dependency reference, as hound is
- [x] Anti-alias cutoff scaling: `cutoff = 0.5 / max(1, step)`, with the kernel
      widened by the same factor so the zero-crossing count holds
- [x] `Ratio` — semitone law + speed law + BPM fitting, exact where it can be
- [x] Drive modes: pitch-driven, BPM-driven, snap-to-sample-exact
- [x] `--pitch <st|cents>` / `--target-bpm <n>` / `--snap`
- [x] Bit depth selection: 16 / 24 / 32 / 32f (`BitDepth`, default 24)
- [x] TPDF dither, ±1 LSB, **deterministically seeded** so the reproducibility
      invariant survives it; `auto` applies it only when the depth drops
- [x] `acid` chunk writer — tempo, beats, meter, loop flag
- [x] `smpl` chunk writer — loop points (spec-inclusive `end`), unity note
- [x] `LIST/INFO` → `ICMT` plain-text tag
- [x] Filename template `{name}_{bpm}bpm_{bars}bars.wav`, named with the tempo
      the file actually plays at after varispeed
- [x] Peak check + overshoot warning (no auto-normalize)
- [x] `--normalize` as an explicit opt-in
- [x] Noise-shaped dither — `--dither shaped`, second-order `(1 - z⁻¹)²`

**What shaping buys and what it costs, measured on silence at 16 bit:** below
5 kHz the added noise drops by more than 8 dB, above 15 kHz it rises by more
than 6 dB, and the *total* noise power rises by 7.78 dB. That last figure is not
tuned — `(1 - z⁻¹)²` has coefficients 1, −2, 1, so the power gain is 1 + 4 + 1 = 6,
and the test asserts the prediction rather than the observation.

Shaping needs the quantisation error to feed back, so `Dither::Shaped`
**quantises as well as dithering** — there is no way to feed back an error that
has not been made yet. It leaves the samples exactly on the target grid, so the
writer's own rounding becomes a no-op and `wav::write` needs no special case.
The fed-back error is clamped at ±2 LSB: without that, a passage sitting at full
scale clips the quantiser and the shaper rings on the clipping error, putting a
burst of noise exactly where the music is loudest.

**What varispeed gets right that a nominal ratio would not:** the output length
comes from `Grid::resampled_length`, which divides the *exact* bar mathematics
rather than the already-rounded cut length. At 103 BPM, 8 bars, half speed, those
two differ: 1 644 117 against 1 644 116. One sample short is a loop that drifts.

Fitting 103 → 90 BPM is exact — the ratio is 90/103 — and lands on 940 800
samples with no residual whatsoever. `--snap` alone nudges 103 → 105 BPM, +33
cents, and buys the same exactness for a loop that had none.

---

## v0.3 — Tape Character

- [x] `TapeParams` + single bypass switch. Naming any amount implies `--tape`:
      a flag that was clearly asked for must not be a silent no-op
- [x] Wow & flutter with **loop-periodic rate quantisation** (`k / loopDur`)
- [x] Zero-mean guarantee over the loop period — a theorem here rather than a
      calibration, see below
- [x] Variable-rate read path driven by the modulation, sharing the varispeed's
      sinc kernel (`SincResampler::read`, which the live preview will want too)
- [x] HF rolloff, cutoff scaling with speed, clamped below Nyquist
- [x] HF rolloff settling handled via double-pass warmup (keep second pass)
- [x] Head bump — peaking EQ at 40–100 Hz, Q 0.7, centre scaling with speed
- [x] `--tape`, `--wow`, `--flutter`, `--hf-rolloff`, `--head-bump`
- [x] Regression test: character bypassed → output bit-identical to v0.2, checked
      on the **written bytes** of the real reference file, not just on the buffer

**The design decision that made this possible: modulate position, not rate.**

The wobble is a *displacement* of the read position, `p(j) = j + D(j)`, where `D`
is a sum of sinusoids whose periods divide the loop exactly. Two things follow,
neither of them approximate:

- **The length is untouched.** The output index still advances by one per frame,
  so there is nothing to round. A rate-modulated wobble would have to integrate
  the rate and land wherever it lands.
- **The seam stays continuous.** `D` is loop-periodic, so the position the next
  repeat starts from is the position this one would have continued to.

The rate deviation is then `D'(j)`, and the integral of the derivative of a
periodic function over its period is exactly zero. The zero-mean guarantee is not
tuned, it is structural. `D(0)` is deliberately *not* pinned to zero: forcing a
node at the seam would put a fixed point in the modulation once per repeat, which
is the tick this whole approach exists to avoid.

Depth is specified as a **speed** deviation, the way a tape machine is, and the
displacement follows as `A = depth · N / (2π k)`. So at equal depth a slow wobble
displaces far more than a fast one — 0.3 % of wow on a 21-second loop is ±22
frames, the same 0.3 % of flutter under one. That asymmetry is physical: it is
why wow is heard as pitch movement and flutter as roughness.

**Nothing here is random.** A random drift would either break byte-identical
reproducibility or need a seed, and a seeded pseudo-random drift on a two-second
loop is a fixed pattern anyway. Two sinusoids at unrelated rates already sound
irregular over a loop.

On the reference file at 90 BPM the quantisation grid is 0.047 Hz, so the
requested rates land within 0.005 Hz of themselves:

```
  tape         wow 0.300 % at 0.703 + 1.922 Hz (15 + 41 cycles/loop), ±22.35 frames
               flutter 0.150 % at 11.016 + 27.000 Hz (235 + 576 cycles/loop), ±0.73 frames
               HF rolloff 6 dB/oct from 10485 Hz
               head bump +2.0 dB at 52 Hz, Q 0.7
```

The rolloff and the bump have moved down with the speed — 12 kHz and 60 Hz at
nominal, 10485 Hz and 52 Hz at 87.4 %. A tape played slower is duller as well as
lower, and that is the same coefficient doing both.

---

## v0.4 — Batch + Ergonomics

- [x] `loopslcr batch <dir>` with rayon parallelism, `--jobs`, `--recursive`
- [x] `--bpm-from-name` — done in v0.1 already, because the archive forced it:
      `102-MTRX-01.wav` and `105CSTC-APRL02-...` have no word break after the
      number, `00005 136BPM E01...` puts the marker in the middle, and
      `58.5 DL_4BAR_...` is a **decimal tempo** that truncates to a file off the
      grid. 14 files carry no tempo anywhere and need one passed in.
- [x] Per-directory preset file — **`loopslcr.args`, not `loopslcr.toml`**, see below
- [x] Progress reporting (stderr), per-file error collection, non-fatal continue
- [x] `--out-dir` with structure preservation
- [x] Shell completions: `loopslcr completions fish|bash|zsh|…`

**Milestone met. The archive is processable in one command:**

```console
$ loopslcr batch "…/DRUMLOOPS" --out-dir ./cut --allow-short
  ok      102-MTRX-01.wav — 4 bars at 102 BPM, 415059 frames → 102-MTRX-01_102bpm_4bars.wav, 24-bit
  …
  261 cut, 16 failed, 2 not WAVE files
failed:
  …/ACEVNTRA-01.wav: no tempo known — pass --bpm (no acid chunk, none in the name)
  …
```

**1.29 s wall for 261 files** at 1481 % CPU, 692 MB written. The 16 failures are
the 14 files with no tempo anywhere plus 2 too short to name a loop length; the
2 skips are the zip archives. Exit code 1 — a half-finished batch that reports
success is a trap for whatever script called it, so *failed* and *worked* are
distinguished by the exit status, not only by the text.

**Skipped is not failed.** A zip file in a folder of drum loops is not an error,
it is not a drum loop. The test is the first twelve bytes rather than the
extension, because two of the archive's WAVE files have no `.wav` on them.

**The output is deterministic.** Work runs on every core, but the report is
assembled in sorted path order and the audio does not depend on the thread that
produced it: `--jobs 1` and `--jobs 16` print identical bytes, and two runs write
261 byte-identical files. That is invariant 4 holding across parallelism.

### The preset is a file of flags, not TOML

The plan said `loopslcr.toml`. TOML would have been a second source of truth for
the flag set: every flag needs a key, every key a type, and the two lists drift
the first time a flag is added without remembering the parser.

`loopslcr.args` holds the flags you would have typed, and clap parses them — one
grammar, nothing to keep in sync, and a flag added tomorrow works in a preset
written today. One flag per line; everything after the first space is the value,
verbatim, so a path with spaces needs no quoting. `#` comments, blank lines
ignored.

```
# 4-bar loops at 16 bit, the way this folder was rendered
--bars 4
--depth 16
--out-dir /mnt/loops/cut clean
```

Presets are spliced in *ahead* of what was typed, so the command line wins —
which needs `args_override_self`, or clap refuses the repetition and a preset can
only ever add, never override. The preset is named in the report: a file that
changes what a command does must not do so invisibly. `--no-preset` ignores it,
which is the honest way out of a switch a preset turned on, since inventing a
`--no-` twin for every boolean would be the config file dictating the interface.

### What the archive found this time

`Path::file_stem` strips whatever follows the last dot. The archive holds
`78-SMPL.BRN-21OCT23-01` and `78-SMPL.BRN-21OCT23-02` — two WAVE files with no
extension at all — and `file_stem` reduces both to `78-SMPL`, so the second
output refused to overwrite the first and one loop went missing. 260 files out of
261. `naming::output_stem` now drops an extension only when it recognises one.

---

## v0.5 — One pipeline, two front ends

Not a planned milestone; it became one the moment the Android app stopped being
hypothetical. The cut used to live in the CLI's `run_cut`, interleaved with the
lines it printed, and an app that reimplemented it would have been a second set
of answers to *which tempo, which loop length, which shape, fade or not*. The
two would have disagreed the first time either was touched.

- [x] `core::pipeline` — `Params` in, `Outcome` out, no filesystem, no printing
- [x] `Outcome` records every decision as data: tempo and its source, bars and
      theirs, the detected shape *and* the chosen one, region, `short_by`, which
      path ran, fade, ratio, resulting tempo, tape report, normalize factor,
      dither mode, peak
- [x] The CLI became a renderer: read the file, call the pipeline, format, write
- [x] `Tempo::from_f32` — an `acid` chunk saying 103.5 becomes the exact fraction
      207/2 via its decimal spelling, not the nearest binary approximation

**The pipeline does not refuse a short loop.** It records `short_by` and the
caller decides: the CLI refuses on a real run and reports on a dry one, and a UI
would want to grey out a button rather than raise an error. Deciding in the core
would make one of those impossible.

**Verified by byte-identity, not by inspection.** After the refactor the batch
over the archive produced **261 byte-identical files and a byte-identical
report**. A refactor of the one thing this program exists to get exactly right is
worth exactly as much as its regression check.

---

## v1.0 — Android APK

> **Both tabs work, the loop plays, and the shrunk release build passes the
> whole instrumented suite.** See `docs/TOOLCHAIN.md` for how to build and test
> it. What remains is refinement — more overrides on the cutter, an export sheet,
> a play head that can be dragged — and CI.

- [x] `loopslcr-jni` cdylib, `cargo-ndk` integration
- [x] Gradle ↔ cargo build wiring (`:app:cargoNdk`, inputs declared)
- [x] JNI surface: `analyze`, `plan`, `process`, `peaks`, `version`
- [x] JNI surface: `previewCreate/Read/SetRatio/Seek/Info/Destroy`
- [x] Direct `ByteBuffer` transfer for PCM (no copies)
- [x] Panic guard at every entry point, proved from Java on host *and* device
- [x] Preview starts/stops serialised — a rebuild racing itself built a second
      `AudioTrack` and pump thread, and the loop played over itself (found by the
      tape sliders, the only continuous control that invalidates a preview)
- [x] Build pinned to its own JDKs, so a system JDK bump cannot break it
- [x] SAF file picking (`ACTION_OPEN_DOCUMENT` / `ACTION_CREATE_DOCUMENT`), no broad permissions
- [x] Compose UI shell
- [x] Two tabs, with a shared view model
- [ ] **Tab: CUTTER**
  - [x] Waveform view from Rust peak buckets
  - [x] Cut region shown as an overlay with markers
  - [x] Bar grid under the waveform, from the plan's own bar length
  - [x] Path A/B selector naming the action, with the detection's evidence shown
  - [x] Pitch control answers under the finger, not on release
  - [x] File figures behind the name instead of ahead of the controls
  - [x] Play in the corner, Open behind the name, sections that fold
        (a folded plan card still shows `clips` / `short by`)
  - [x] Zoom — two fingers to magnify, one to pan; 4096 buckets measured once
        at load and spent by the zoom, so `MAX_ZOOM` is bound to the
        measurement rather than to taste
  - [x] Compact rows; the typed tempo rides the varispeed strip beside the
        speed it sets rather than sitting a slider away from it
  - [ ] BPM detector — only the 14 archive files carrying no tempo need it
  - [x] Draggable markers — they snap to bar lines, which is the only way a
        finger is allowed near a cut point; with handles, and a line that
        follows the finger rather than the pipeline
  - [x] Loop bars, skip bars, workflow selector
  - [x] BPM and signature overrides, BPM unit — without these a file that
        declares no tempo could not be cut on the phone at all
  - [ ] Tail mode selector with auto-detect suggestion
  - [x] **Grid vs. loop priority — visible toggle**
  - [x] Bipolar varispeed control, detent at 0
  - [x] Target-BPM entry as the other way to ask for the same thing
  - [x] Live readout in all three units (st/cents, % speed, BPM), from the
        plan rather than from the control
  - [x] Tape character panel + bypass
  - [x] Bit depth, normalize, snap, short-loop override
  - [ ] Export sheet: dither mode, tags, filename template
- [x] **Tab: CALCULATOR**
  - [x] Beats/bar, bars/min, beat length, bar length, Hz
  - [x] Note table 1/1 … 1/32: ms, Hz, **samples**, share of the bar
  - [x] Dotted and triplet rows
  - [x] Total duration for N bars
  - [x] Sample-exactness marked per row — the one entry that is not decoration
  - [x] "→ send to Cutter" action (tempo becomes a varispeed target)
  - [ ] Percent-of-bar column in the table (computed, not yet shown)
- [x] Shared ViewModel: the calculator follows the file the cutter opened
- [x] **Preview engine**
  - [x] `AudioTrack` streaming from Rust variable-rate resampler
  - [x] Ratio glide via one-pole smoother in the audio thread (tape inertia)
  - [x] Lock-free ratio handoff (atomic)
  - [x] Seamless loop playback across the seam
  - [x] **Stepped motion of the play head** — the loop divided into equal
        pieces, displaced by whole pieces on each piece boundary, so it is
        rearranged and still repeats exactly once per loop. Four shapes, the
        scattered one a hash of the step index rather than a generator.
        Crossfaded 4 ms at equal power. Preview only: it never reaches the file
  - [ ] Bake the motion into an export (needs the displacement in exact
        rational frames, not the preview's floats)
  - [ ] Draggable play head (seek exists; nothing drives it from the waveform yet)
- [x] Dark theme
- [x] Instrumented tests: the engine on a real Android runtime, the screen rendered
- [x] Shrunk, signed release build — R8 rules keep the JNI entry points, and
      `-PtestRelease` runs all 25 instrumented tests against the shrunk APK
- [x] Large files survive: read straight into a direct buffer, `OutOfMemoryError`
      caught and reported instead of killing the app
- [x] `pipeline::Stage::Plan` — the dry run skips resampling, 33 ms against
      6.26 s, so the UI answers while a control is still moving
- [ ] Stream rather than decode whole — the real fix for long files
- [ ] GitHub Actions: release APK on tag push

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
| M1 | v0.1 Core + CLI, exact cut math, dry-run over the archive | **DONE** |
| M2 | v0.2 Varispeed, bit depth, dither, BPM tagging | **DONE** bar noise-shaped dither |
| M3 | v0.3 Tape character with loop-periodic modulation | **DONE** |
| M4 | v0.4 Batch processing, CLI feature-complete | **DONE** |
| M5 | v1.0 Android APK, two tabs, tape-riding preview | **IN PROGRESS** — both tabs, preview, draggable markers and a signed release build; CI open |
| M6 | v1.1 Saturation (oversampling + ADAA) | TODO |
| M7 | v2.0 Slice export, Elektron export, desktop GUI | TODO |

---

## Open Questions

- ~~Project name~~ → **LOOP_SLCR**, crates `loopslcr-*`, binary `loopslcr`
- Micro-fades: default on (0.5 ms) or default off?
- ~~Peak buckets: Rust over JNI, or Kotlin?~~ → **Rust**, as one flat `float[]`
  interleaved in drawing order. Measured once per file; a resize only redraws.
- WAV reading: keep `hound`, or hand-rolled for zero dependency?
  (blocks the next task — the writer has to be hand-rolled either way for
  `acid`/`smpl`, so reading it too costs perhaps 200 lines and drops the
  dependency entirely)
- Wow/flutter default depths — and should character settings be presetable?
- iOS: worth it, or does the CLI plus Android cover the real workflow?
