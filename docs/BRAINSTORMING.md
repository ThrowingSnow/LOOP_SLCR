# LOOPCUT — Brainstorming

> Working title **LOOPCUT**, repository name **LOOP_SLICR**. Final name not yet
> decided (see §9); `loopcut-*` crate names in the docs are placeholders.
> Status: idea space consolidated, decisions locked, ready for implementation.

---

## 1. The Problem

Drum loops are produced in **Caustic** on a phone. Every export needs the same
manual surgery in a wave editor before it is usable:

1. **Head:** the first bars are unusable. Delay/reverb only start receiving
   material once the first trigger fires on step 1, so the FX are not yet in
   steady state.
2. **Tail:** the render ends with an FX decay tail hanging past the last bar.

Current workaround: duplicate all patterns in Caustic so the song is twice as
long, render, then manually delete the first half and cut the tail.

**Every single loop. By hand. On a phone.**

### Reference case
`103 29Jul26 1Punkt1 Cstc.wav`

| | |
|---|---|
| Tempo | 103 BPM (encoded in filename) |
| Song length | 16 bars (Caustic song view, positions 0–15) |
| Bar length | 2.3301 s |
| Total | 37.28 s + ~0.7 s tail |
| Desired cut | 18.641 s → 37.282 s |

---

## 2. Core Insight — Two Paths to a Seamless Loop

### Path A — Warmup render (the current manual method)

Bars 9–16 already contain the tails bleeding in from bars 1–8. Since bars 1–8
are content-identical to 9–16, the extracted region **is** the steady-state
loop. A straight cut is correct; the tail after bar 16 is discarded because its
energy is already represented inside the extract.

> ⚠️ Applying foldback *on top of* path A would double the tails. The two
> methods are alternatives, never combined.

### Path B — Tail foldback

Render only 8 bars + tail. The tail is the decay of the hits in bars 1–8. In a
true infinite loop that energy would sound during bars 1–2 of the *next*
repetition. In a naive cut it is simply missing, which is why the loop start
sounds dead and the seam sounds chopped.

Foldback overlays the tail additively onto the loop head:

```
out[i % loopLen] += tail[i]
```

```
[ Bar1+Tail ][ Bar2+Tail ][ Bar3 ]...[ Bar8 ]
     ^ decay of bar 8 lands here
```

**Mathematically this is circular rather than linear convolution** — for linear
FX (reverb, delay without saturation) it produces the *exact* periodic steady
state, not an approximation. Path A is linear convolution repeated until "settled
enough". So foldback is not merely faster, it is more precise.

- Tail longer than the loop wraps automatically via `% loopLen`
- Summation can overshoot → peak check required afterwards

> ⚠️ Breaks for **nonlinear** FX (distortion, compressor, saturated delay
> feedback) where superposition does not hold. Path A stays the safe choice there.

### Why this matters
Path B removes the pattern-duplication step from the Caustic workflow entirely
and halves render time. **This is the actual killer feature — not the cut itself.**

---

## 3. Timing Mathematics

### 3.1 Generalised bar length

BPM needs a reference unit. DAW and Caustic convention is the quarter note, but
compound meters are often counted in dotted quarters. So the unit is a parameter:

```
U = BPM unit as a fraction of a whole note   (default 1/4)

secondsPerBar = (60/BPM) · (N/D) / U
```

| Signature | U | Quarters per bar |
|---|---|---|
| 4/4 | 1/4 | 4 |
| 3/4 | 1/4 | 3 |
| 7/8 | 1/4 | 3.5 |
| 6/8 | 3/8 | 2 dotted quarters |

### 3.2 Exact integer arithmetic — no floats anywhere

With BPM stored as a rational `p/q` and `U = a/b`, every cut point is exactly
rational:

```
cutSample(i) = round( i · SR · 60 · N · b · q  /  (p · D · a) )
```

One `i128` product, one division, round-half-up. **Zero accumulated error, no
float drift, bit-identically reproducible** — including 103.5 BPM in 7/8.

Verification (103 BPM, 4/4, 44.1 kHz, i = 8):
`8·44100·60·4·4 / (103·4·1) = 338688000 / 412 = 822058.25…` → **822058** ✓

This removes the numerical question entirely. What remains is a *design* choice:

| Mode | Behaviour |
|---|---|
| **Loop priority** | cut-in on grid, length = `round(bars · samplesPerBar)` |
| **Grid priority** | both markers rounded to the bar grid (length may differ by ±1 sample) |

Residual error is displayed in µs / ppm. At 103 BPM: 0.25 samples ≈ 5.7 µs ≈ 0.3 ppm.

### 3.3 Sample-exact BPM suggestion

At 44.1 kHz with 8 bars of 4/4 the sample count is `84 672 000 / BPM`, integer
only for divisors of 84 672 000 (= 2⁹ · 3³ · 5³ · 7²).

Within 90–130: **90, 96, 98, 100, 105, 108, 112, 120, 125, 126, 128**

→ *"103 BPM is not sample-exact — nearest exact: 100 / 105."*
Pairs naturally with the varispeed BPM-driven mode (§4.3).

### 3.4 Workflow auto-detection

File duration ÷ bar length reveals the render type:

| Ratio | Suggestion |
|---|---|
| ≈ 2× loop + tail | Path A — discard, `skip = bars` |
| ≈ 1× loop + tail | Path B — foldback, `skip = 0` |

---

## 4. Varispeed

### 4.1 Varispeed ≠ pitch shift

The requested behaviour is a bipolar tape transport control (Octatrack feel):
**pure resampling**, pitch and tempo move together. No PSOLA, no phase vocoder,
no formant correction.

This is fortunate: **a perfect loop stays a perfect loop after varispeed.** A
time-preserving pitch shift would smear artefacts across exactly the seam this
tool exists to clean up.

```
ratio     = 2^(semitones/12)
newBPM    = oldBPM × ratio
newLength = oldLength / ratio
```

### 4.2 Where the "feel" actually lives

Export is a static factor — there is no feel in a number. The tape feel happens
in **preview**: moving the control makes the playback rate glide with motor
inertia (time constant ~80–200 ms) instead of jumping. Ride the loop live,
export the final value. Plus a **detent at 0**.

### 4.3 Taper laws — switchable, default semitones

The core only ever knows one number: `ratio: f64`. The taper is pure UI
mathematics, so supporting both costs ~15 lines.

```rust
// Semitone law (default) — sampler behaviour, musically symmetric
ratio = 2f64.powf(x * st_max / 12.0)

// Speed law — real tape, ±X % of nominal speed
ratio = 1.0 + x * pct_max
```

| Law | Behaviour | Character |
|---|---|---|
| **Semitones** | ±12 st equal both ways | Sampler (Digitakt, Octatrack), exact intervals |
| **% speed** | +50 % = +7.02 st, −50 % = −12 st | Real tape — **the asymmetry IS the tape feel** |

> The display always shows **all three units simultaneously** — semitones/cents,
> % speed, resulting BPM. The taper affects *feel*, never *information*.

> The speed law is also the physically correct axis for tape character: HF loss
> and wow depth scale with **speed**, not with semitones.

### 4.4 Three drive modes

| Mode | Input | Output | Use |
|---|---|---|---|
| **Pitch-driven** | semitones / cents | new BPM | sound design |
| **BPM-driven** | target BPM | ratio + semitones | fitting a loop to a track |
| **Snap** | — | nearest sample-exact target BPM | combines with §3.3 |

Example: 103 → 90 BPM gives ratio 0.8738 = **−2.34 semitones**.

### 4.5 Two resamplers, not one

| Job | Ratio | Requirement | Solution |
|---|---|---|---|
| **Preview** | time-varying (glide) | allocation-free, per-sample rate change | **own** phase accumulator, short kernel (16–32 taps) |
| **Export** | fixed | maximum quality, offline | **`rubato`** |

`rubato` is block-based and built for largely fixed ratios — a poor fit for live
tape riding. A variable-rate reader has to be written regardless (~60 lines).
Both sit behind a `Resampler` trait; `rubato` then doubles as a **trusted
reference** if an own polyphase sinc replaces it later.

> ⚠️ Anti-aliasing when pitching up is mandatory: `cutoff = 0.5 / max(1, ratio)`.

---

## 5. Tape Character (v1)

### The problem
Wow & flutter normally destroys loop determinism. A modulated ratio means output
length ≠ `oldLen / ratio` and the seam no longer matches.

### The solution — loop-periodic modulation
- Quantise wow/flutter LFO rates to `k / loopDuration`, k integer
- LFO must be zero-mean over its period (sine, or a sum of sines)

The integral of rate deviation over one loop is then **exactly zero**, so length
stays `round(oldLen / ratio)` and the seam stays continuous.

**Is the grid fine enough?** For an 18.64 s loop the rates are multiples of
0.0536 Hz. Wow (0.5–6 Hz) → k = 10…112. Flutter (6–100 Hz) → k = 112…1864.
Plenty of choice, finer than the ear. ✅

| Element | Implementation | Loop risk |
|---|---|---|
| **Wow & flutter** | LFO rates quantised to `k/loopDur`, zero-mean | solved |
| **HF rolloff** | filter cutoff scales with speed | settling transient → **same warmup trick as the whole app**: run the loop twice, keep the second pass |
| **Head bump** | low shelf / peak 40–100 Hz, centre scales with speed | static, harmless |

Applied **after** resampling. **One single bypass switch** — the clean varispeed
path must stay bit-exactly reproducible.

### Saturation — deferred to v1.1
Varispeed, zero-mean wow, HF rolloff and head bump are **all linear**. Saturation
would be the only nonlinear element, and therefore not a knob but a subsystem:
harmonics above Nyquist → aliasing → 4× oversampling + ADAA waveshaping +
up/downsampling filters.

v1 stays a chain built entirely from LTI blocks: verifiable, bit-exactly
reproducible, finishable in one pass. The ADAA stage lands in v1.1 where existing
OktoTakt work can be ported rather than rewritten.

---

## 6. Pipeline Order (binding)

```
read → cut (exact, original domain)
     → foldback (optional, original domain)
     → fade (optional)
     → resample (varispeed)
     → tape character (optional)
     → peak check / normalize
     → dither
     → write + chunks
```

- **Cut before resample.** Bar mathematics is exactly rational in the original
  domain; afterwards it is a single `round(oldLen / ratio)`. The other order
  would cut on a fractional grid.
- **Foldback before resample** — the tail belongs to the original timing.
- **Dither last**, after normalising, immediately before quantisation.

---

## 7. Declaring BPM in the Output

| Layer | Location | Content |
|---|---|---|
| 1 | **Filename** | `{name}_{bpm}bpm_{bars}bars.wav` — matches existing convention |
| 2 | **`acid` chunk** | ACIDized loop: tempo, beats, root note, loop flag. Read by most DAWs |
| 3 | **`smpl` chunk** | loop points, unity note, fine tune |
| 4 | **`LIST/INFO` → `ICMT`** | plain text for humans |

> ⚠️ **Architectural consequence:** `hound` cannot write custom chunks, so it is
> out as a writer. RIFF writing is ~150 lines — write it. Reading could stay on
> `hound`, but for a minimal tool doing both means **zero WAV dependency**.

### Dither
32f → 16 bit without dither produces quantisation distortion that is audible on
quiet reverb tails — precisely this material. TPDF by default, noise shaping
optional. Order: normalize → dither → quantise, in that order and no other.

---

## 8. Idea Space — Parked and Rejected

| Idea | Verdict |
|---|---|
| Time-preserving pitch shift (PSOLA / phase vocoder) | **Rejected** — smears the loop seam, defeats the purpose |
| `symphonia` for decoding | **Rejected** — decoder zoo for formats not needed |
| Multi-track / mixer / FX rendering | **Rejected** — this is a tool, not an instrument |
| MP3/OGG/FLAC support | **Rejected for v1** — Caustic exports WAV |
| Onset/transient-based auto-trim | **Rejected** — BPM + bars is deterministic and exact; detection is guessing |
| Automatic BPM detection from audio | **Parked** — filename parsing covers the real workflow |
| Zero-crossing snap | **Rejected** — breaks the exact grid, micro-fades solve clicks better |
| Web Audio + Capacitor | **Rejected** — Termux constraint gone, WebView + SAF is friction |
| Kotlin-only core | **Rejected** — core would not survive past the APK |
| C++ / JUCE | **Rejected** — no realtime DSP requirement, massive overkill |
| Cue-chunk bar markers | **v2** — useful for later slicing |
| Slice export (8×1 bar, 16×½ bar) | **v2** |
| Elektron-ready export (mono sum, SR/bit conversion for DT2) | **v2** |
| Desktop GUI (drag & drop) | **v2** |
| Tape saturation | **v1.1** — see §5 |

---

## 9. Naming Candidates

| Name | Note |
|---|---|
| **LOOP_SLICR** | current repository name — working candidate |
| **TAKTSCHNITT** | German, fits the HexaTakt / OktoTakt family |
| **LOOPKLIPP** | short, DE/EN hybrid |
| **TAILFOLD** | names the killer feature |
| **BARCUT** | international, solid, unexciting |
| **SCHNITTTAKT** | triple T, very German |
| ~~CSTC-CUT~~ | too Caustic-specific, limits scope |

Decision deferred.

---

## 10. Project Family Context

| Project | What | Status |
|---|---|---|
| **HexaTakt** | 16-track JUCE groovebox, VST3 + standalone | exists, module donor |
| **OktoTakt** | 8-voice Rytm-style drum machine, JUCE | architecture + roadmap done |
| **DRUMOID** | simple Android drum app | brainstorming |
| **LOOPCUT** | ← this project — a tool, not an instrument | ready to build |

LOOPCUT is deliberately the **smallest** project in the family. It has a
finishable scope, an immediately useful CLI stage, and an existing archive of
279+ loops to validate against on day one.

---

## 11. Locked Decisions

| # | Decision |
|---|---|
| 1 | Rust core, staged: v0.1 CLI → v1.0 JNI + Compose APK |
| 2 | Build: Gradle locally (Threadripper, Code-OSS), GitHub Actions for signed releases |
| 3 | Arbitrary time signatures N/D plus BPM unit U |
| 4 | Two tabs: Cutter / Calculator, shared state |
| 5 | Both tail modes (discard + foldback), auto-detect suggests |
| 6 | Selectable bit depth (16/24/32f) + TPDF dither |
| 7 | Foldback overshoot: warn, user decides — no auto-normalize |
| 8 | Varispeed (resampling), not time-preserving pitch shift |
| 9 | Taper switchable, default semitones, all three units always displayed |
| 10 | Two resamplers: `rubato` (export) + own variable-rate (preview) |
| 11 | Tape character in v1, with loop-periodic modulation |
| 12 | Saturation deferred to v1.1 |
| 13 | Grid vs. loop priority: **visible toggle** |
| 14 | Own RIFF writer (chunks), zero WAV dependency preferred |

---

## 12. Still Open

- Name
- Micro-fades: default on (0.5 ms) or default off?
- Peak buckets: computed in Rust and passed over JNI, or in Kotlin?
- WAV reading: keep `hound` or also hand-rolled for zero dependency?
- Wow/flutter default depths and whether character settings are presetable
