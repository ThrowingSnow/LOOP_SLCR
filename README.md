# LOOP_SLCR

> **Status: v0.1 through v0.4 complete.** The whole
> chain runs — read → cut → foldback → fade → varispeed → tape character →
> normalize → dither → write — behind `loopslcr cut`, and `loopslcr batch` puts
> the whole 279-file archive through it in **1.3 seconds**: 261 cut, 16 refused
> with a named reason, 2 not WAVE files. **No runtime dependencies** in the core;
> `hound` and `rubato` are test-only second opinions.
> See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the breakdown.

A precision loop-trimming tool. Feed it a rendered drum loop with a warmup head and
an FX tail; it returns a **sample-exact, seamless N-bar loop**, optionally transposed
by tape varispeed, with the resulting tempo declared in the file.

Two front ends, one core:

- **CLI** (Linux first, macOS/Windows come free) — batch-capable, the daily driver
- **Android APK** — two tabs (Cutter / Calculator), touch-first, tape-riding preview

---

## The problem

Drum loops are built in **Caustic** on a phone. Every export needs the same manual
surgery in a wave editor:

1. **Head:** the first bars are unusable — delay and reverb only start receiving
   material once the first trigger fires, so the FX are not in steady state yet.
2. **Tail:** the render ends with an FX decay hanging past the last bar.

The current workaround is to duplicate every pattern in Caustic so the song is twice
as long, render, then delete the first half and cut the tail — by hand, for every
single loop.

### Reference case — `103 29Jul26 1Punkt1 Cstc.wav`

| | |
|---|---|
| Tempo | 103 BPM (encoded in the filename) |
| Song length | 16 bars |
| Bar length | 2.3301 s |
| Total | 37.28 s + ~0.7 s tail |
| Desired cut | 18.641 s → 37.282 s |

### What it does today

```console
$ loopslcr cut "103 29Jul26 1Punkt1 Cstc.wav"
  source       1875540 frames, 0:42.529  at 103 BPM (from filename), 4/4
  shape        18.2521 bars, 17.5749 audible — warmup render (path A)
  loop         8 bars (from file length), skip 8, align loop
  path A — cut 822058 .. 1644116, tail discarded
  fade         23 frames each end, Cosine
  result       822058 frames, 0:18.641  peak 1.000000 (+0.00 dBFS)
  length       -5.724 µs (-0.307 ppm)
  out          103 29Jul26 1Punkt1 Cstc_103bpm_8bars.wav
  wrote        4932560 bytes, 24-bit
```

Tempo, loop length and workflow are all read off the file; `--dry-run` reports
the same without writing. Add `--bpm`, `--bars`, `--skip` or `--path` to override
any of it.

A whole directory in one command:

```console
$ loopslcr batch "…/DRUMLOOPS" --out-dir ./cut --allow-short
  ok      102-MTRX-01.wav — 4 bars at 102 BPM, 415059 frames → 102-MTRX-01_102bpm_4bars.wav, 24-bit
  …
  261 cut, 16 failed, 2 not WAVE files
failed:
  …/ACEVNTRA-01.wav: no tempo known — pass --bpm (no acid chunk, none in the name)
  …
```

Every file is attempted and every failure keeps its own reason, because a batch
that stopped at the first unusable file would never reach the other 261. The exit
code is still 1 when anything failed — a half-finished batch that reports success
is a trap for whatever script called it. A zip file among the loops is *skipped*
rather than failed, decided on the first twelve bytes rather than the extension,
since two of these WAVE files carry no `.wav` at all.

**The output does not depend on the thread that produced it.** `--jobs 1` and
`--jobs 16` print identical bytes and write 261 byte-identical files.

Per-directory settings live in `loopslcr.args`, holding the flags you would have
typed — one grammar rather than a TOML schema that has to be kept in step with
the flag set:

```
# 4-bar loops at 16 bit, the way this folder was rendered
--bars 4
--depth 16
--out-dir /mnt/loops/cut clean
```

Everything after the first space is the value, verbatim, so a path with spaces
needs no quoting. Typed flags override the preset, the preset is named in the
report, and `--no-preset` ignores it.

Varispeed fits a loop to another tempo, exactly:

```console
$ loopslcr cut "103 29Jul26 1Punkt1 Cstc.wav" --target-bpm 90
  varispeed    -2.336 st (-233.6 cents), -12.621 % speed, ratio 0.873786408 = 90/103
               103 BPM → 90 BPM, 940800 frames, exact ratio
  length       940800 vs 940800.0000 exact at 90 BPM — sample-exact
```

The ratio 90/103 is a fraction, so the new length is exact rather than rounded
twice — and 8 bars at 90 BPM happens to be a whole 940 800 samples, leaving no
residual at all. `--snap` on its own finds the nearest tempo where that is true.

Tape character is one switch, and it costs the loop nothing:

```console
$ loopslcr cut "103 29Jul26 1Punkt1 Cstc.wav" --target-bpm 90 --tape --normalize
  tape         wow 0.300 % at 0.703 + 1.922 Hz (15 + 41 cycles/loop), ±22.35 frames
               flutter 0.150 % at 11.016 + 27.000 Hz (235 + 576 cycles/loop), ±0.73 frames
               HF rolloff 6 dB/oct from 10485 Hz
               head bump +2.0 dB at 52 Hz, Q 0.7
  length       940800 vs 940800.0000 exact at 90 BPM — sample-exact
```

Still 940 800 samples. The wobble is a *displacement* of the read position whose
period divides the loop, not a modulated rate, so the output index still advances
one frame at a time and there is nothing to round; the rate deviation, being the
derivative of a periodic function, integrates to exactly zero over the loop. The
rates printed are the quantised ones — the grid here is 0.047 Hz, far finer than
the ear. And the two filters have moved down with the speed, from 12 kHz and 60 Hz:
a tape played slower is duller as well as lower.

---

## Two paths to a seamless loop

**Path A — warmup render (the current manual method).** Bars 9–16 already contain
the tails bleeding in from bars 1–8, so the extracted region *is* the steady-state
loop. A straight cut is correct and the trailing tail is discarded.

**Path B — tail foldback.** Render only 8 bars + tail and overlay the tail additively
onto the loop head:

```
out[i % loopLen] += tail[i]
```

Mathematically this is **circular** rather than linear convolution: for linear FX
(reverb, delay without saturation) it produces the *exact* periodic steady state, not
an approximation. It also removes the pattern-duplication step from the Caustic
workflow and halves render time.

**This is the actual killer feature — not the cut itself.**

Both modes ship; auto-detect (file duration ÷ bar length) suggests which one applies.
They are alternatives, never combined — foldback on top of path A doubles the tails.
Path A stays the safe choice for nonlinear FX where superposition does not hold.

---

## Exact timing, no floats

BPM is stored as a rational `p/q`, the BPM reference unit as `U = a/b` (default 1/4),
the signature as `N/D`. Every cut point is then exactly rational:

```
cutSample(i) = round( i · SR · 60 · N · b · q  /  (p · D · a) )
```

One `i128` product, one division, round-half-up. **Zero accumulated error, no float
drift, bit-identically reproducible** — including 103.5 BPM in 7/8.

Verification (103 BPM, 4/4, 44.1 kHz, i = 8):
`8·44100·60·4·4 / (103·4·1) = 338688000 / 412 = 822058.25…` → **822058** ✓

Residual rounding error is reported in µs / ppm. Arbitrary time signatures are
supported, and alignment is a visible toggle: **loop priority** (cut-in on grid,
length = `round(bars · samplesPerBar)`) or **grid priority** (both markers on grid).

---

## Feature overview

- WAV in/out: PCM 16/24/32-bit int and 32-bit float, mono/stereo, sample rate from
  the header — never assumed
- Cut on an exact bar grid, skip-bars and loop-bars configurable
- Tail mode: discard or foldback, with auto-detect suggestion
- **Varispeed** — pure resampling (pitch and tempo move together, tape style), never
  a time-preserving pitch shift, which would smear exactly the seam this tool cleans
  up. Pitch-driven, BPM-driven, or snap to the nearest sample-exact BPM
- **Tape character** — wow & flutter as a loop-periodic *displacement* of the read
  position rather than a modulated rate, so the length is untouched and the seam
  stays continuous by construction; plus HF rolloff and head bump, both scaling
  their corner frequencies with playback speed. One bypass switch, and with it off
  the output is byte-identical to the clean path
- Bit depth selectable (16/24/32f) with TPDF dither, optionally noise-shaped:
  `--dither shaped` puts the noise where the ear is not, buying >8 dB below 5 kHz
  for 7.78 dB more of it in total
- BPM declared four ways: filename template, `acid` chunk, `smpl` chunk, `LIST/INFO`
- **Batch** — a directory at a time on every core, carrying on past the files it
  cannot cut and naming each reason; deterministic output regardless of thread
  count, `--out-dir` mirroring the source tree, per-directory `loopslcr.args`
  presets, shell completions
- **BPM/time calculator** in the style of `toolstud.io/music/bpm.php`, extended with
  dotted and triplet rows plus sample counts

### Pipeline order (binding)

```
read → cut → foldback → fade → resample → tape character
     → peak check / normalize → dither → write + chunks
```

Cut and foldback happen in the original time domain where the bar grid is exact;
resampling afterwards costs one single rounding of the output length. Dither is last,
immediately before quantisation.

### Deliberately out of scope

No multi-track, no mixer, no FX rendering, no codec zoo. Minimal means minimal.

---

## Android

The APK builds, installs and runs. The cutter works end to end offline: pick a
WAVE file through the Storage Access Framework, see it as a waveform with the
cut region marked, set bars, skip, workflow, varispeed, tape character and bit
depth, watch the dry run update as you go, and export.

No permissions are asked for. The file arrives because the user handed it over,
and nothing else on the device is readable.

All of the audio is the same Rust core the CLI uses, reached through a JNI
surface of five calls. Nothing about timing, cutting or resampling is
reimplemented in Kotlin — the UI decides what to ask for and shows what came
back, and that is all it does.

**The loop can be ridden by ear.** Press play and the cut plays at its own
tempo; move the varispeed and the pitch bends rather than jumping, because the
rate is approached through a one-pole smoother standing in for the inertia of a
reel. The audio thread reads blocks straight out of Rust into a direct buffer,
and the rate reaches it through a single atomic — nothing the UI does can make
it wait, and nothing it does can make the UI wait.

The preview deliberately does *not* bake the varispeed into the loop: the
pipeline runs with the ratio neutralised, so changing speed costs an atomic
store rather than a re-cut. Tape character is applied at unity, which makes a
heavily transposed preview slightly brighter than the render, where the filters
sit lower.

**The calculator tab does no arithmetic.** It asks Rust and lays the answer out.
Beat and bar lengths, the note-value table from 1/1 to 1/32 with dotted and
triplet rows, in milliseconds, hertz and **samples** — and each row marked for
whether it lands on a whole sample, because a delay time that does not drifts
out of the grid over a long loop. At 103 BPM and 44.1 kHz it reports 8 bars as
822 058 samples, which is exactly what the cutter cuts.

A reimplementation in Kotlin would have agreed for a while and then, at some
tempo nobody tested, quietly not — and being right about exactly this is the
whole job.

Still to come: draggable cut markers and a signed release build.

Build and test it with [`docs/TOOLCHAIN.md`](docs/TOOLCHAIN.md).

---

## Layout

```
├── crates/
│   ├── loopslcr-core/   # exact rational math, WAV I/O, cut, foldback — no platform deps
│   ├── loopslcr-cli/    # clap binary `loopslcr`                        ← v0.1
│   └── loopslcr-jni/    # cdylib for Android                            ← v1.0
├── android/             # Kotlin + Compose, cargo-ndk                   ← v1.0
└── docs/
```

Stack: **Rust core, staged.** v0.1 is a pure CLI that is useful on the desktop before
a single line of Android code exists — validated with `--dry-run` against an existing
archive of 279 loops. v1.0 adds a deliberately narrow JNI surface and a Compose UI.

---

## Roadmap at a glance

| Milestone | Content | |
|---|---|---|
| M1 | v0.1 core + CLI, exact cut math, dry-run over the archive | **done** |
| M2 | v0.2 varispeed, bit depth, dither, BPM tagging | **done** |
| M3 | v0.3 tape character with loop-periodic modulation | **done** |
| M4 | v0.4 batch processing, CLI feature-complete | **done** |
| M5 | v1.0 Android APK, two tabs, tape-riding preview | in progress |
| M6 | v1.1 saturation (oversampling + ADAA) | |
| M7 | v2.0 slice export, Elektron export, desktop GUI | |

Full detail in [`docs/ROADMAP.md`](docs/ROADMAP.md).

---

## Documentation

| Document | Content |
|---|---|
| [`docs/BRAINSTORMING.md`](docs/BRAINSTORMING.md) | Problem space, the two paths, timing math, varispeed, tape character, locked decisions, rejected ideas |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Version-by-version task lists, milestones, exit criteria |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Design principles, module tree, core types, signal flow, key algorithms, JNI boundary, threading, testing strategy, invariants |
| [`docs/CONTEXT.md`](docs/CONTEXT.md) | Session handover notes (German), the raw thinking behind the above |
| [`docs/TOOLCHAIN.md`](docs/TOOLCHAIN.md) | The Android toolchain: exact versions, why each, how to build and test the APK |

---

## Open questions

- Should the calculator tab get its own JNI surface, or share `analyze`?
  (computing the grid in Kotlin is not an option — it would be a second source
  of truth for the one thing this tool exists to get right)
- Wow/flutter default depths, and whether character settings are presetable
- iOS: worth it, or do CLI + Android cover the real workflow?

### Answered while building

- **Micro-fades** are path-dependent: on for a straight cut (0.5 ms, raised
  cosine), off for a foldback — which is circular by construction, so fading
  both ends to zero would undo the continuity it just computed.
- **WAV reading is hand-rolled too**, sharing the chunk layer with the writer;
  `hound` stays as a dev-dependency for cross-checking in both directions. It
  paid for itself immediately: Caustic writes the RIFF size field 44 bytes
  short, and believing it cost audio from the end of every export.
- **There is no fixed `--bars` default.** The loop length is derived from the
  file's own duration, because 8 bars is right for the reference render and
  wrong for four fifths of the archive.

---

## Project family

| Project | What |
|---|---|
| **HexaTakt** | 16-track JUCE groovebox, VST3 + standalone |
| **OktoTakt** | 8-voice Rytm-style drum machine, JUCE |
| **DRUMOID** | simple Android drum app |
| **LOOP_SLCR** | ← this project — a tool, not an instrument |

Deliberately the **smallest** project in the family: finishable scope, an immediately
useful CLI stage, and a real archive to validate against on day one.

---

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([`LICENSE-MIT`](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
