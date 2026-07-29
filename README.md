# LOOP_SLCR

> **Status: M1 complete.** Exact timing core, WAV read/write, `cut`, `foldback`
> and `fade`, driven by `loopslcr cut`. The exit criterion is met: `--dry-run`
> over the 279-file archive processes 261 and refuses 18 with a named reason.
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
- **Tape character** — wow & flutter with loop-periodic modulation (LFO rates
  quantised to `k / loopDuration`, zero-mean, so output length stays invariant),
  HF rolloff, head bump — behind one bypass switch
- Bit depth selectable (16/24/32f) with TPDF dither
- BPM declared four ways: filename template, `acid` chunk, `smpl` chunk, `LIST/INFO`
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
| M2 | v0.2 varispeed, bit depth, dither, BPM tagging | |
| M3 | v0.3 tape character with loop-periodic modulation | |
| M4 | v0.4 batch processing, CLI feature-complete | |
| M5 | v1.0 Android APK, two tabs, tape-riding preview | |
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

---

## Open questions

- Peak buckets: computed in Rust and passed over JNI, or computed in Kotlin?
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
