# LOOP_SLCR — Kontextfile (Session-Übergabe)

> **Zweck:** Diese Datei in einen neuen Chat ziehen → Claude Van Damme ist sofort auf Stand.
> **Status:** v0.1 UND v0.2 STEHEN (bis auf noise-shaped Dither).
> Die ganze Kette läuft: read → cut → foldback → fade → varispeed →
> normalize → dither → write, gesteuert von `loopslcr cut`.
> 178 Tests grün, Clippy sauber, **null Runtime-Dependencies im Core**
> (`hound` und `rubato` sind dev-only Gegenproben).
> Der 103-BPM-Referenzfall reproduziert die Docs exakt
> (in 822058 = 0:18.641, out 1644116 = 0:37.282).
> **Reader gegen das echte Archiv verifiziert:** 277 WAVE-Dateien,
> 177 790 491 Frames, sample-für-sample identisch mit `hound`.
> **`--dry-run` über alle 279 Archiv-Einträge:** 261 verarbeitet,
> 18 mit benanntem Grund verweigert.
> **`loopslcr batch` schiebt das ganze Archiv in 1,3 s durch:** 261 geschnitten,
> 16 mit benanntem Grund verweigert, 2 keine WAVE-Dateien. Ausgabe deterministisch
> unabhängig von der Thread-Zahl.
> Nächster Schritt: v1.0, die Android-APK.
> **Letztes Update:** Session 3 — 30.07.2026

---

## 1. Projekt in einem Satz

Minimalistische Android-APK, die Caustic-Drumloop-Exports **sample-genau auf einen
sauberen N-Bar-Loop schneidet** (Warmup-Bars weg, FX-Tail weg), optional per
**Tape-Varispeed** transponiert und die resultierende BPM im Output deklariert.
Dazu ein **BPM-/Zeit-Rechner** im Stil von `toolstud.io/music/bpm.php`.

Name: **LOOP_SLCR** (entschieden — §9). Crates `loopslcr-*`, Binary `loopslcr`.

---

## 2. Der Ist-Workflow (Mishkas Schmerz)

1. Drumloop in **Caustic** auf dem Handy bauen — 8 Bars Pattern.
2. Alle Patterns **duplizieren** → Song ist 16 Bars lang.
   Grund: Delay/Reverb/FX sind in Bars 1–8 noch nicht eingeschwungen,
   weil FX erst ab dem ersten Trigger auf Step 1 überhaupt Material bekommen.
3. Export → WAV mit 16 Bars **+ Fade-out-Tail** (Reverb/Delay-Ausklang).
4. Manuell in WaveEditor:
   - Bars 1–8 löschen (Warmup)
   - Tail hinten abschneiden
   - Ergebnis: sauberer 8-Bar-Loop mit eingeschwungenen FX

**Das ist jedes Mal Handarbeit. Genau das soll die App wegnehmen.**

### Beispiel aus den Screenshots
- Datei: `103 29Jul26 1Punkt1 Cstc.wav`
- Caustic-Songview: Positionen 0–15 belegt → **16 Bars**
- `103` im Dateinamen = **103 BPM**
- Rechnung: 1 Bar @103 BPM 4/4 = 2.3301 s → 16 Bars = **37.28 s**
- Waveform endet bei ~37.3 s, Tail läuft bis ~38 s → **Rechnung stimmt** ✅
- Gewünschter Cut: **18.641 s → 37.282 s**

---

## 3. Die Kernerkenntnis (wichtig!)

### 3a. Warmup-Render vs. Tail-Foldback — zwei Wege zum selben Ziel

**Weg A — Warmup (Mishkas aktueller Weg):**
Bars 9–16 enthalten bereits die Tails aus Bars 1–8. Da Bars 1–8 inhaltlich
identisch mit 9–16 sind, ist der Ausschnitt 9–16 **exakt der eingeschwungene
Loop-Zustand**. → Gerader Schnitt ist korrekt. Der Tail nach Bar 16 wird
**verworfen**, weil seine Energie im Ausschnitt schon drin ist.
⚠️ Tail-Foldback hier **zusätzlich** anwenden wäre falsch → doppelte Tails.

**Weg B — Tail-Foldback (spart den Duplizier-Schritt in Caustic):**
Nur 8 Bars + Tail rendern. Der Tail ist der Reverb-/Delay-Ausklang der Hits
aus Bars 1–8. In einer echten Endlosschleife würde genau diese Energie über
Bar 1–2 der *nächsten* Wiederholung klingen. Im geschnittenen Ausschnitt
fehlt sie → Anfang klingt tot, Naht klingt abgehackt.

Foldback = Tail **additiv auf den Loop-Anfang überlagern** (= overdubben):

```
out[i % loopLen] += tail[i]          // für alle Tail-Samples
```

```
[ Bar1+Tail ][ Bar2+Tail ][ Bar3 ]...[ Bar8 ]
     ↑ hier landet der Ausklang von Bar 8
```

Mathematisch = **zirkuläre** statt linearer Faltung → bei linearen FX
(Reverb/Delay ohne Sättigung) der **exakte** periodische Steady-State,
nicht bloss angenähert. Weg A ist lineare Faltung mehrfach wiederholt,
bis es „eingeschwungen genug" ist → Foldback ist präziser, nicht nur
schneller.

- Tail > Loop-Länge → wickelt durch `% loopLen` automatisch mehrfach um
- Nach dem Summieren: Peak-Check, Addition kann übersteuern

⚠️ Bricht bei nichtlinearen FX (Distortion, Kompressor, saturiertes
Delay-Feedback) — Superposition gilt dort nicht. Dann ist Weg A sicherer.

→ **Beide Modi in die App.** Weg B spart das Duplizieren in Caustic,
halbiert die Renderzeit und ist bei linearen FX mathematisch sauberer.
**Das ist das eigentliche Killer-Feature — nicht der Schnitt selbst.**

### 3b. Bar-Mathematik — generalisiert für beliebige Taktarten

BPM braucht eine Bezugseinheit. DAW-/Caustic-Konvention = Viertelnote,
in 6/8 zählt man aber oft punktierte Viertel. Also als Parameter mitführen:

```
U = BPM-Einheit als Bruchteil einer Ganzen   (default 1/4)

secondsPerBar = (60/BPM) · (N/D) / U
```

| Taktart | U | Viertel/Bar |
|---|---|---|
| 4/4 | 1/4 | 4 |
| 3/4 | 1/4 | 3 |
| 7/8 | 1/4 | 3.5 |
| 6/8 | 3/8 | 2 punktierte Viertel |

### 3c. Exakte Ganzzahl-Arithmetik (Float komplett vermeiden)

BPM als Bruch `p/q`, `U = a/b` → **jeder Cut-Punkt ist exakt rational**:

```
cutSample(i) = round( i · SR · 60 · N · b · q  /  (p · D · a) )
```

Ein `i128`-Produkt, eine Division, Round-Half-Up. **Null akkumulierter
Fehler, kein Float-Drift, bitidentisch reproduzierbar** — auch bei
103.5 BPM in 7/8.

Gegenprobe (103 BPM, 4/4, 44.1 kHz, i=8):
`8·44100·60·4·4 / (103·4·1) = 338688000 / 412 = 822058.25…` → **822058** ✓

Damit ist die numerische Frage erledigt. Übrig bleibt nur die *Design*-Wahl:

- **Grid-Priorität:** beide Marker aufs Bar-Raster (Länge kann ±1 Sample abweichen)
- **Loop-Priorität:** Cut-In aufs Raster, Länge = `round(bars · samplesPerBar)` ← Default für Loops

App zeigt den Restfehler in µs / ppm an. Bei 103 BPM: 0.25 Samples ≈ 5.7 µs ≈ 0.3 ppm.

### 3d. Feature-Idee "Sample-Exact BPM"
Bei 44.1 kHz und 8 Bars 4/4 ist `84 672 000 / BPM` die Samplezahl.
Ganzzahlig nur für Teiler von 84 672 000 (= 2⁹ · 3³ · 5³ · 7²).
Im Bereich 90–130: **90, 96, 98, 100, 105, 108, 112, 120, 125, 126, 128**.
→ *"103 BPM ist nicht sample-exakt — nächste exakte: 100 / 105."*

### 3e. Auto-Detect-Workflow
Dateidauer ÷ Bar-Länge verrät den Render-Typ:
- ~2× Loop + Tail → **Weg A vorschlagen** (Discard, skip = bars)
- ~1× Loop + Tail → **Weg B vorschlagen** (Foldback, skip = 0)

### 3f. Varispeed (Tape-Regler / Octatrack-Feeling)

**Varispeed ≠ Pitch-Shift.** Gewünscht ist reines Resampling — Pitch und Tempo
bewegen sich gemeinsam, wie an einer Bandmaschine. Kein PSOLA, kein
Phase-Vocoder, keine Formant-Korrektur.

**Warum das hier goldrichtig ist:** Ein perfekter Loop bleibt nach Varispeed
ein perfekter Loop. Ein zeiterhaltender Pitch-Shift würde an der Loop-Naht
Artefakte erzeugen — also genau dort, wo wir gerade mühsam aufräumen.

```
ratio     = 2^(semitones/12)
newBPM    = oldBPM × ratio
newLength = oldLength / ratio
```

#### Das "Feeling" sitzt im Preview, nicht im Export
Export = statischer Faktor, da gibt es kein Gefühl. Das Tape-Gefühl entsteht
beim **Preview**: Reglerbewegung → Abspielrate gleitet mit Motor-Trägheit
hinterher (Zeitkonstante ~80–200 ms) statt zu springen. Live am Loop reiten,
Endwert exportieren.
Dazu: **Detent bei 0** (Rasterung in der Mitte).

#### Taper — ENTSCHIEDEN: umschaltbar, Default Halbtöne

Der Core kennt nur **eine** Zahl: `ratio` (f64). Der Taper ist reine UI-Mathematik.
Umschaltbar kostet ~15 Zeilen.

```rust
// Halbton-Law (Default) — Sampler-Verhalten, musikalisch symmetrisch
ratio = 2f64.powf(x * st_max / 12.0)

// Speed-Law — echtes Tape, ±X % Nenngeschwindigkeit
ratio = 1.0 + x * pct_max
```

| Law | Verhalten | Charakter |
|---|---|---|
| **Halbtöne** | ±12 st beidseitig gleich | Sampler (Digitakt/Octatrack), exakte Intervalle |
| **% Speed** | +50 % = +7.02 st, −50 % = −12 st | echtes Tape — **die Asymmetrie IST das Tape-Gefühl** |

> Display zeigt **immer alle drei Einheiten gleichzeitig**: Halbtöne/Cents,
> % Speed, resultierende BPM. Taper beeinflusst nur das *Gefühl*, nie die *Information*.

> Speed-Law ist ausserdem die physikalisch richtige Achse für den Tape-Charakter
> (§3f-Charakter): HF-Verlust und Wow-Tiefe skalieren mit **Speed**, nicht mit Halbtönen.

#### Drei Ansteuerungs-Modi
| Modus | Eingabe | Ergebnis | Zweck |
|---|---|---|---|
| **Pitch-driven** | Halbtöne / Cents | neue BPM | Sounddesign |
| **BPM-driven** | Ziel-BPM | Ratio + Halbtöne | Loop auf Track anpassen |
| **Snap** | — | nächste sample-exakte Ziel-BPM | kombiniert mit §3d |

Beispiel BPM-driven: 103 → 90 BPM = Ratio 0.8738 = **−2.34 Halbtöne**.

#### Resampling — ENTSCHIEDEN: ZWEI Resampler, nicht einer

| Job | Ratio | Anforderung | Lösung |
|---|---|---|---|
| **Preview** | zeitvariabel (Glide) | allocation-free, Rate-Änderung pro Sample | **eigener** Phasen-Akkumulator + kurzer Kernel (16–32 Taps) |
| **Export** | fix | maximale Qualität, offline | **`rubato`** |

`rubato` ist blockweise und für weitgehend festes Ratio gebaut → schlechter Fit
fürs Tape-Riding im Preview. Der Variable-Rate-Reader muss also ohnehin selbst
geschrieben werden (~60 Zeilen: Phasenakkumulator + Interpolation).

Beides hinter ein **`Resampler`-Trait**. Gewinn: falls später ein eigener
Polyphase-Sinc für den Export kommt, ist `rubato` die **vertrauenswürdige
Referenz zum A/B-Vergleich**. Das ist mehr wert als die eingesparte Dependency.

⚠️ **Anti-Aliasing beim Hochpitchen zwingend:** `cutoff = 0.5 / max(1, ratio)`.
Bei `rubato` konfigurierbar, im eigenen Reader selbst dran denken.

#### Tape-Charakter — ENTSCHIEDEN: v1, unter einer Bedingung

**Das Problem:** Wow & Flutter zerstört normalerweise die Loop-Determiniertheit.
Moduliertes Ratio → Ausgabelänge ≠ `oldLen / ratio`, Naht passt nicht mehr.

**Die Lösung — Modulation muss loop-periodisch sein:**
- Wow/Flutter-LFO-Frequenzen auf `k / loopDuration` quantisieren (k ganzzahlig)
- LFO zero-mean über seine Periode (Sinus oder Summe von Sinussen)

→ Integral der Ratenabweichung über einen Loop ist **exakt null**
→ Länge bleibt `round(oldLen / ratio)`, Naht bleibt stetig.

**Ist das Raster fein genug?** Bei 18.64 s Loop: Vielfache von 0.0536 Hz.
Wow (0.5–6 Hz) → k = 10…112. Flutter (6–100 Hz) → k = 112…1864.
**Massenhaft Auswahl, feiner als das Ohr.** ✅

| Element | Umsetzung | Loop-Risiko |
|---|---|---|
| **Wow & Flutter** | LFO-Raten auf `k/loopDur` quantisiert, zero-mean | gelöst (s.o.) |
| **HF-Rolloff** | Filter-Cutoff skaliert mit Speed | Einschwingvorgang → **derselbe Warmup-Trick** wie die ganze App: Loop zweimal durchlaufen, zweiten Durchgang behalten |
| **Head-Bump** | Low-Shelf/Peak 40–100 Hz, Mittenfrequenz skaliert mit Speed | statisch, unkritisch |

Einbau **nach dem Resample**. **Ein einziger Bypass-Schalter** — der saubere
Varispeed-Pfad muss bit-exakt reproduzierbar bleiben.

---

### 3g. Pipeline-Reihenfolge (verbindlich)

```
read → cut (exakt, Original-Domain)
     → foldback (optional, Original-Domain)
     → fade (optional)
     → resample (Varispeed)
     → tape-charakter (optional: wow/flutter, HF-rolloff, head-bump)
     → peak-check / normalize
     → dither
     → write + Chunks
```

- **Schneiden VOR Resamplen.** Bar-Mathematik ist im Original exakt rational;
  danach einmalig `round(oldLen / ratio)`. Umgekehrt schneidet man auf krummem Raster.
- **Foldback ebenfalls vorher** — der Tail gehört ins Original-Timing.
- **Dither zuletzt**, nach dem Normalisieren, direkt vor der Quantisierung.

---

### 3h. BPM im Output deklarieren

| Ebene | Chunk / Ort | Inhalt |
|---|---|---|
| 1 | **Dateiname** | `{name}_{bpm}bpm_{bars}bars.wav` — passt zu Mishkas Konvention |
| 2 | **`acid`** | ACIDized-Loop: Tempo, Beats, Root Note, Loop-Flag. Von den meisten DAWs gelesen |
| 3 | **`smpl`** | Loop-Punkte, Unity Note, Fine Tune |
| 4 | **`LIST/INFO` → `ICMT`** | Klartext für Menschen |

> ⚠️ **Architektur-Konsequenz:** `hound` kann **keine Custom-Chunks schreiben**
> → fällt als Writer raus. RIFF-Writing ist ~150 Zeilen → selbst machen.
> Lesen könnte `hound` bleiben — bei "minimalistisch" aber lieber beides selbst
> und **null WAV-Dependency**.

---

---

## 4. Feature-Scope

### v1 — MVP
- [ ] WAV laden über SAF (`ACTION_OPEN_DOCUMENT`), keine breiten Permissions
- [ ] WAV-Parser: PCM 16/24/32-bit int + 32-bit float, Mono/Stereo, SR aus Header
- [ ] Waveform-Darstellung (Peak-Min/Max-Buckets, kein Full-Resolution-Draw)
- [ ] Eingabe: BPM, Taktart (4/4 default), Skip-Bars (default 8), Loop-Bars (default 8)
- [ ] Cut-Marker live als Overlay auf der Waveform
- [ ] Tail-Modus: **Discard** (Weg A) / **Foldback** (Weg B)
- [ ] Preview: Region loopen (nahtlos, um die Naht zu hören)
- [ ] **Varispeed**: bipolarer Tape-Regler, Detent bei 0, Glide im Preview
- [ ] Varispeed-Modi: Pitch-driven / BPM-driven / Snap
- [ ] Taper umschaltbar (Halbtöne / % Speed), Anzeige immer in allen 3 Einheiten
- [ ] **Tape-Charakter**: Wow & Flutter (loop-periodisch), HF-Rolloff, Head-Bump
      — ein einziger Bypass-Schalter
- [ ] Export als WAV, Bit-Tiefe wählbar (16/24/32f) + TPDF-Dither
- [ ] BPM-Deklaration: Dateiname + `acid` + `smpl` + `LIST/INFO`
- [ ] Dateiname-Template `{name}_{bpm}bpm_{bars}bars.wav`
- [ ] BPM-Rechner-Screen (siehe §5)

### v1.5
- [x] Batch-Modus: Preset auf ganzen Ordner anwenden (`loopslcr.args`)
- [ ] Auto-Detect: aus Dateiname BPM raten (`103 29Jul26...` → 103)
- [ ] Auto-Detect: aus Dauer + BPM die Bar-Anzahl vorschlagen
- [ ] Tail-Länge messen (Abfall unter −60 dBFS) + Warnung wenn Tail > Loop-Länge
- [ ] Micro-Fades (0.2–2 ms) an den Kanten gegen Klicks, optional
- [ ] Normalisieren / True-Peak-Check / DC-Offset entfernen
- [ ] Sample-Exact-BPM-Vorschlag (§3c)

### v2
- [ ] Desktop-CLI (Linux) aus demselben Core
- [ ] Slice-Export: 8-Bar-Loop zusätzlich in 8×1-Bar oder 16×½-Bar zerlegen
- [ ] `cue`-Chunk mit Bar-Markern für späteres Slicen
- [ ] Elektron-Ready-Export (Mono-Sum, Bitrate/SR-Konvertierung für DT2)
- [ ] Drag&Drop-Desktop-GUI

### Bewusst NICHT drin
- Kein Multi-Track, kein Mixer, kein Effekt-Rendering, kein MP3/Codec-Zoo.
  **Minimalistisch heisst minimalistisch.**

---

## 5. BPM-Rechner-Screen (nach toolstud.io-Vorbild)

Eingabe: BPM, Taktart, Base (4/8/12/16/24/32)

Ausgabe:
- Beats/min, Beats/bar, Bars/min
- Länge 1 Beat (s), Länge 1 Bar (s), Beats/sec (Hz)
- Tabelle der Fraktionen 1/16 … 16/16: Prozent, **ms**, **Hz**
- Zusätzlich (über toolstud hinaus): **punktiert** und **Triolen**
- Zusätzlich: **Samples** pro Fraktion bei gewähltem SR
- Zusätzlich: Bar-Länge × N Bars → Gesamtdauer (direkt für den Cutter nutzbar)

Praxisnutzen: Delay-/LFO-Werte für Caustic, Digitakt 2, Analog Four MkII ablesen.
→ Rechner sollte auch **standalone** gut sein, nicht nur Beiwerk.

---

## 6. Stack — Empfehlung: gestaffelter Rust-Core

**Entschieden:** Entwicklung auf Threadripper-Desktop, Code-OSS, eigenes GitHub-Repo.
Termux-Constraint entfällt damit komplett.

**Empfohlener Weg (Rust-Core, gestaffelt):**

| Stufe | Inhalt | Nutzen |
|---|---|---|
| **v0.1** | `loopslcr-core` (lib) + `loopslcr-cli` (bin), reines Rust, kein Android | Läuft am selben Abend. Batch über bestehendes `AUDIO/DRUMLOOPS/`-Archiv (279+ Files) → sofortiger Nutzen ohne eine Zeile Android-Code |
| **v1.0** | `loopslcr-jni` (cdylib) + Kotlin/Compose UI, `cargo-ndk` | Core schon auf echten Files validiert → APK ist nur noch UI-Arbeit |

**JNI-Oberfläche bewusst winzig — zwei Funktionen:**
- `analyze(path)` → Header, SR, Kanäle, Dauer, Peak-Buckets für Waveform
- `process(params)` → geschriebenes File

**Begründung:**
- Rust ist bekannt (nih-plug), Kotlin eher nicht → Lernkurve nur bei der UI
- Core lebt weiter: CLI, später Desktop-Tool. Kotlin-only wäre danach tot
- Web/Capacitor: Termux-Argument weg, WebView + SAF für beliebige WAVs = unnötige Reibung
- Kein Realtime-DSP → C++/JUCE wäre Overkill

**Ehrlicher Gegenpunkt:** `cargo-ndk` + NDK + Gradle-Verdrahtung ist ein Nachmittag
Setup, den Kotlin-only nicht hätte. Wenn nur die Handy-App zählt und das Archiv egal
ist → Kotlin-only ist die schnellere Linie.

### Cargo-Workspace-Layout

```
loopslcr/
├── crates/
│   ├── loopslcr-core/     # lib: WAV-IO, Rational-Mathe, Cut, Foldback
│   ├── loopslcr-cli/      # bin: clap-CLI  ← v0.1
│   └── loopslcr-jni/      # cdylib: 2 Funktionen  ← v1.0
├── android/              # Kotlin + Compose, cargo-ndk
└── Cargo.toml
```

### Crates
| Zweck | Wahl | Begründung |
|---|---|---|
| WAV-Read | eigener RIFF-Parser (oder `hound`) | siehe §3h |
| WAV-Write | **eigener RIFF-Writer, ~150 Zeilen** | `hound` kann keine `acid`/`smpl`-Chunks → raus |
| Resampling (Export) | **`rubato`** | offline, volle Qualität, dient später als Referenz |
| Resampling (Preview) | **eigener** Phasen-Akkumulator | zeitvariables Ratio, allocation-free |
| Dither | selbst (TPDF, ~20 Zeilen) | trivial |
| CLI | `clap` (derive) | Standard |
| Rationals | `num-rational` oder i128 handgerollt | für die exakte Cut-Mathematik (§3c) |
| ~~Decoder~~ | **nicht** `symphonia` | Decoder-Zoo für Formate die wir nicht brauchen |

### CLI-Oberfläche v0.1

```
loopslcr info  in.wav
loopslcr cut   in.wav --bpm 103 --bars 8 --skip 8 --tail discard -o out.wav
loopslcr cut   in.wav --bpm 103 --bars 8 --skip 0 --tail fold    -o out.wav
loopslcr batch ./drumloops --recursive --out-dir ./cut/ --jobs 8
```

Flags: `--sig 7/8` · `--bpm-unit 1/4` · `--align loop|grid` · `--fade 1ms`
· `--normalize` · `--dry-run` · `--bits 16|24|32f` · `--dither tpdf|none`
· `--pitch -2.34st` **oder** `--target-bpm 90` · `--tag acid,smpl,info`
· `--tape` · `--wow 0.3` · `--flutter 0.15` · `--hf-rolloff auto|off|<Hz>`
· `--head-bump 2|off` — jeder Charakter-Flag impliziert `--tape`
· batch: `--recursive` · `--out-dir` · `--jobs N` · `--verbose` · `--no-preset`
· Preset pro Ordner: `loopslcr.args` · Completions: `loopslcr completions fish`

- **`--dry-run` ist der wichtigste Flag:** druckt Cut-Punkte, Restfehler in µs/ppm,
  Tail-Länge — schreibt nichts. Damit einmal über die 279 Archiv-Loops laufen und
  sehen wo die Annahmen brechen, bevor irgendwas überschrieben wird.
- **`--bpm-from-name`** liest Mishkas Konvention `^(\d{2,3})\b`
  → `103 29Jul26 1Punkt1 Cstc.wav` → 103
- **`--skip` Default = `--bars`** ("überspringe genau eine Loop-Länge Warmup").
  Bei beliebigen Taktarten/Loop-Längen immer richtig, bei Mishka zufällig auch 8.

### Build-Pipeline (entschieden)
- **Gradle lokal** auf dem Threadripper für Entwicklung
- **GitHub Actions** nur für signierte Release-APKs bei Tag-Push
- Cargo-Workspace + Android-Modul in **einem** Repo

---

## 6b. Stack-Optionen (Vergleich, Archiv)

| | Option A | Option B | Option C |
|---|---|---|---|
| **Stack** | Kotlin + Jetpack Compose, reines JVM | Rust-Core + JNI, Kotlin/Compose UI | Web Audio + Capacitor |
| **APK-Grösse** | ~3–5 MB | ~5–8 MB | ~8–15 MB (WebView) |
| **Aufwand v1** | niedrig | mittel | niedrig |
| **NDK nötig** | nein | ja | nein |
| **Core wiederverwendbar** | nur JVM | **Linux-CLI + später Plugin** | nur Web |
| **Waveform-Render** | Compose Canvas | Compose Canvas | WebGL (Mishkas Heimspiel) |
| **Termux-baubar** | mühsam (aapt2) | Rust ok, APK-Teil mühsam | am ehesten |

**Erste Einschätzung:** Für „minimalistisch + schnell fertig" → **A**.
Für „Core lebt weiter" (Linux-CLI, später Desktop-Tool) → **B**.
WAV-Parsing + Trimmen ist kein Realtime-DSP → kein zwingender Grund für C++/JUCE.

**Empfehlung:** Core-Logik (WAV-IO, Bar-Mathe, Foldback) strikt von der UI trennen,
egal welche Option. Dann ist ein späterer Port billig.

---

## 6c. UI-Struktur (entschieden)

**Zwei Tabs: CUTTER / RECHNER**

Geteilter State über gemeinsames ViewModel: **BPM, Taktart, BPM-Einheit, Sample Rate**.
Im Rechner eingestellt → im Cutter sofort da. Plus "→ in Cutter übernehmen"-Button.
Ohne geteilten State sind es zwei Apps in einer APK statt einem Werkzeug.

---

## 7. Technische Notizen

- **Caustic** exportiert WAV, typ. 44.1 kHz. SR immer aus dem Header lesen, nie annehmen.
- **Android-Storage:** SAF statt `READ_EXTERNAL_STORAGE` → keine Permission-Dialoge,
  Play-Store-freundlich, funktioniert auf allen modernen Android-Versionen.
- **Preview-Playback:** `AudioTrack` im Static-Mode mit Loop-Points, oder `MediaPlayer`
  auf eine temp-WAV. `AudioTrack.setLoopPoints()` ist sample-genau → besser.
- **Waveform:** Peak-Buckets vorberechnen (min/max pro Pixelspalte), auf Handy reicht
  ein einziger Downsample-Pass. Kein Bedarf für GPU.
- **Foldback-Implementation:** `out[i % L] += tail[i]` für alle Tail-Samples,
  danach Clipping-Check (Summierung kann übersteuern) → optional Peak-Normalize.

---

## 8. Offene Fragen

### Entschieden ✅
1. ~~Stack~~ → **Rust-Core gestaffelt**: v0.1 CLI, v1.0 JNI + Compose
2. ~~Build~~ → Gradle lokal (Threadripper, Code-OSS) + GitHub Actions für Release-APKs
3. ~~Taktarten~~ → **beliebig N/D frei**, plus BPM-Einheit U
4. ~~UI~~ → **zwei Tabs** Cutter / Rechner mit geteiltem State
5. ~~Tail-Modus~~ → **beide** (Discard + Foldback), Auto-Detect schlägt vor
6. ~~Output-Format~~ → **Bit-Tiefe wählbar** (16/24/32f) + TPDF-Dither
7. ~~Foldback-Übersteuerung~~ → **warnen, User entscheidet** (kein Auto-Normalize)
8. ~~Pitch~~ → **Varispeed** (Resampling), kein zeiterhaltender Pitch-Shift
9. ~~Taper~~ → **umschaltbar**, Default Halbtöne, Anzeige in allen 3 Einheiten
10. ~~Resampler~~ → **zwei**: `rubato` (Export) + eigener Variable-Rate (Preview)
11. ~~Tape-Charakter~~ → **v1**, mit loop-periodischer Modulation
12. ~~Sättigung~~ → **v1.1**, nach Release (Oversampling + ADAA, Port aus OktoTakt)
13. ~~Grid vs. Loop~~ → **sichtbarer Toggle**
14. ~~Docs~~ → generiert, siehe `docs/`
15. ~~Name~~ → **LOOP_SLCR**, Crates `loopslcr-*`, Binary `loopslcr`

### v0.2-Entscheidungen (30.07.2026)

- **Der Resampler ist selbstgeschrieben, nicht `rubato`** — und zwar aus einem
  fachlichen Grund, nicht aus Dependency-Askese: **rubato kann nicht zirkulär
  resamplen.** Ein Loop *ist* periodisch, das Sample vor dem Anfang ist das
  Sample kurz vor dem Ende. Liest der Kernel dort Nullen — was jeder
  Allzweck-Resampler tut, weil er es nicht wissen kann — verliert genau die
  Naht Energie, die dieses Tool nahtlos machen soll. Gemessen: gegen die
  analytische Lösung ist unser Fehler an der Naht < 1e-4, rubatos > 0.01.
  Im Innenraum stimmen beide auf < 1e-3 überein, also ist der Kernel richtig.
  `rubato` bleibt dev-dependency-Gegenprobe, wie `hound`.
- **Die Ausgabelänge ist ein *Eingabe*-Parameter des Resamplers.** Sie kommt aus
  `Grid::resampled_length()`, das die *exakte* Bar-Arithmetik teilt, nicht die
  schon gerundete Schnittlänge. Bei 103 BPM, 8 Bars, halber Geschwindigkeit
  unterscheiden die beiden sich: **1 644 117 gegen 1 644 116.** Ein Sample zu
  kurz ist ein driftender Loop. Das effektive Verhältnis ergibt sich dann aus
  den zwei Ganzzahl-Längen — Tonhöhenfehler weit unter einem ppm, dafür eine
  Länge, die exakt stimmt.
- **Zwei der drei Antriebsarten sind exakt.** 103 → 90 BPM ist das Verhältnis
  90/103 und landet auf glatten **940 800 Samples, Residual null**. Prozent sind
  exakt, ganze Oktaven auch. Nur ein Halbton ist irrational — `Ratio` hält den
  Unterschied als Enum fest, statt alles in f64 zu werfen.
- **`--snap` allein ist ein eigenes Feature:** es zieht 103 → 105 BPM (+33 Cent)
  und macht damit einen Loop sample-exakt, der es nicht war.
- **Der Dither-Seed ist fest.** Invariante 4 verlangt, dass gleiche Eingabe und
  gleiche Parameter für immer byte-identisch ausgeben — ein zufällig geseedeter
  Dither bricht das bei jedem Lauf. Also eine deterministische Folge, die nur
  wie Rauschen aussieht. `--dither-seed` für einen anderen Zug.

### v0.3-Entscheidungen (30.07.2026)

- **Wow/Flutter moduliert die *Position*, nicht die Rate.** §3f hatte das Problem
  richtig benannt (moduliertes Ratio → Länge stimmt nicht mehr) und als Lösung
  quantisierte LFO-Raten plus Zero-Mean vorgesehen. Umgesetzt ist die stärkere
  Form: die Modulation ist eine **Verschiebung** der Leseposition,
  `p(j) = j + D(j)` mit loop-periodischem `D`. Damit folgt beides ohne
  Näherung — der Ausgabeindex läuft weiter in Einerschritten, also gibt es
  nichts zu runden; und `D` ist loop-periodisch, also ist die Naht stetig. Die
  Ratenabweichung ist `D'(j)`, und das Integral der Ableitung einer periodischen
  Funktion über ihre Periode ist **exakt** null. Zero-mean ist hier ein Satz,
  keine Kalibrierung.
- **`D(0)` wird absichtlich *nicht* auf null gezwungen.** Ein erzwungener Knoten
  an der Naht wäre ein Fixpunkt der Modulation einmal pro Repeat — genau das
  Ticken, das der ganze Ansatz vermeidet. Ein `D(0) ≠ 0` verschiebt nur den
  ganzen Loop um einen Sample-Bruchteil.
- **Die Tiefe ist eine Geschwindigkeitsabweichung**, wie eine Bandmaschine
  spezifiziert wird. Die Verschiebung folgt daraus als `A = depth · N / (2π k)`,
  also verschiebt ein langsames Wobbeln bei gleicher Tiefe viel weiter als ein
  schnelles: 0,3 % Wow auf 21 s sind ±22 Frames, dieselben 0,3 % Flutter unter
  einem. Diese Asymmetrie ist physikalisch — sie ist der Grund, warum Wow als
  Tonhöhenbewegung und Flutter als Rauhigkeit gehört wird.
- **Kein Zufall im Charakter.** Echtes Wow driftet zufällig; das hier nicht, weil
  eine Zufallskomponente entweder die Byte-Reproduzierbarkeit bricht oder einen
  Seed braucht — und ein geseedeter Pseudo-Zufall ist auf einem Zwei-Sekunden-Loop
  sowieso ein festes Muster. Zwei Sinusse auf unverwandten Raten klingen über
  einen Loop schon unregelmässig genug.
- **Filter nach dem Wobbeln, nie umgekehrt.** `run_periodic` kann den
  Einschwingvorgang nur für ein Filter mit *festen* Koeffizienten wegheben (Loop
  zweimal durchlaufen, Zustand mitnehmen, zweiten Durchgang behalten). Ein
  moduliertes Filter hat keinen stationären Zustand, in den es einschwingen
  könnte. Die Restabweichung ist `|pol|^frames` — bei 40 Hz und 800 000 Frames
  unterläuft das auf null, also exakt und nicht bloss nah dran.
- **Ein einziger Bypass, geprüft an den geschriebenen Bytes.** Der Unit-Test
  zeigt, dass `apply` den Buffer nicht anfasst; der Regressionstest über die echte
  Referenzdatei zeigt, dass die *Datei* Byte für Byte dieselbe ist — samt der
  Gegenprobe, dass sie sich mit eingeschaltetem Charakter unterscheidet, sonst
  würde der Test nur beweisen, dass `apply` nie erreicht wurde.
- **Charakter-Flags implizieren `--tape`.** `--wow 0.5` ohne `--tape` wäre ein
  stiller No-Op, und das ist schlimmer als eine Implikation.

### v0.4-Entscheidungen (30.07.2026)

- **Das Preset ist eine Flag-Datei, kein TOML.** Geplant war `loopslcr.toml`; das
  wäre eine **zweite Quelle der Wahrheit für die Flag-Liste** gewesen — jeder Flag
  braucht einen Key, jeder Key einen Typ, und beide Listen driften beim ersten
  Flag, das ohne den Parser dazukommt. `loopslcr.args` enthält die Flags, die man
  sowieso getippt hätte, und clap parst sie: eine Grammatik, nichts
  synchronzuhalten, und ein Flag von morgen funktioniert in einem Preset von
  heute. Ein Flag pro Zeile, alles nach dem ersten Leerzeichen ist der Wert
  wörtlich — Pfade mit Leerzeichen brauchen keine Quotes, und dieses Archiv liegt
  unter genau so einem Pfad.
- **Preset wird *vor* das Getippte gespleisst**, damit die Kommandozeile gewinnt.
  Das braucht `args_override_self`, sonst lehnt clap die Wiederholung ab und ein
  Preset kann nur hinzufügen, nie überschreiben. Das Preset steht im Report: eine
  Datei, die das Verhalten ändert, darf das nicht unsichtbar tun. `--no-preset`
  ignoriert sie ganz — der ehrliche Ausweg aus einem Schalter, den ein Preset
  eingeschaltet hat, statt für jeden Boolean einen `--no-`-Zwilling zu erfinden.
- **Übersprungen ≠ fehlgeschlagen.** Eine Zip-Datei im Loop-Ordner ist kein
  Fehler, sie ist kein Loop. Entschieden an den ersten zwölf Bytes, nicht an der
  Endung — zwei der Archiv-WAVEs haben gar keine.
- **Exit-Code 1, wenn irgendetwas fehlschlug**, auch wenn der Batch weiterläuft.
  Ein halb fertiger Batch, der Erfolg meldet, ist eine Falle für das aufrufende
  Skript.
- **Determinismus über Parallelität.** Gearbeitet wird auf allen Kernen, der
  Report aber in sortierter Pfadreihenfolge zusammengesetzt, und das Audio hängt
  nicht am Thread: `--jobs 1` und `--jobs 16` geben identische Bytes aus, zwei
  Läufe schreiben 261 byte-identische Dateien. Invariante 4 gilt auch quer über
  Threads.
- **Ein Flag-Satz für `cut` und `batch`** (`CutFlags` als geflatteter clap-`Args`),
  damit ein neuer Flag beide erreicht und sie nicht auseinanderdriften können. Die
  Validierung passiert einmal in `CutFlags::resolve()`, bevor die erste Datei
  angefasst wird — ein kaputtes `--pitch` scheitert einmal, nicht 279-mal.
- **Vom Archiv gefunden:** `Path::file_stem` schneidet alles nach dem letzten
  Punkt ab. `78-SMPL.BRN-21OCT23-01` und `-02` haben *keine* Endung, also wurden
  beide zu `78-SMPL` und die zweite Ausgabe weigerte sich, die erste zu
  überschreiben — 260 von 261. `naming::output_stem` streicht eine Endung jetzt
  nur, wenn es sie als Audio-Endung kennt.

### Noise-Shaping nachgereicht (30.07.2026)

- **`--dither shaped` quantisiert mit.** Shaping braucht den Quantisierungsfehler
  als Rückkopplung, und den gibt es nicht, bevor quantisiert wurde. Also liegen
  die Samples danach exakt auf dem Zielraster, das Runden im Writer ist ein
  No-Op, und `wav::write` braucht keinen Sonderfall — das ist die Naht, die
  verhindert, dass das ins Encoding durchsickert.
- **`(1 - z⁻¹)²`, zweiter Ordnung.** Koeffizienten 1, −2, 1 → Leistungsgewinn
  1 + 4 + 1 = 6 = **7,78 dB mehr Rauschen insgesamt**, dafür >8 dB weniger unter
  5 kHz und >6 dB mehr über 15 kHz. Der Test prüft die *Vorhersage* aus dem
  Filter, nicht die Beobachtung — driftet die Implementierung vom Filter weg,
  sagt er es.
- **Fehler-Rückkopplung auf ±2 LSB begrenzt.** Ohne das klippt ein Vollpegel-
  Passus den Quantisierer, der Klipping-Fehler geht in die Rückkopplung, und der
  Shaper klingelt darauf — ein Rauschstoss genau dort, wo die Musik am lautesten
  ist.
  `auto` dithert nur, wenn die Bittiefe **sinkt**; bei gleicher oder steigender
  wäre das Rauschen reiner Verlust.

### Noch offen ❓
1. ~~**Micro-Fades:** default an (0.5 ms) oder default aus?~~ → **entschieden:
    pfadabhängig.** An bei Pfad A (0.5 ms, Raised Cosine), aus bei Pfad B.
    Grund: ein Foldback-Loop ist *konstruktionsbedingt* zirkulär stetig — ihn an
    beiden Enden auf Null zu fahren würde genau die Stetigkeit zerstören, die
    gerade errechnet wurde. Ein gerader Schnitt hat diese Garantie nicht, seine
    Grenzen fallen dorthin, wo das Bar-Grid sie setzt, mitten in die Wellenform
    oder nicht. `--fade <ms>` / `--no-fade` überschreiben.
    **Der ehrliche Preis des Fades:** er ersetzt den Klick durch eine kurze
    Pegelsenke von doppelter Fade-Länge, einmal pro Wiederholung. Bei 0.5 ms auf
    perkussivem Material unhörbar, auf Flächen ein leiser Puls. Ohne Senke geht
    nur ein zirkulärer Crossfade mit Material *vor* dem Loop-Start — das ändert
    den Loop-Kopf und ist eine andere Operation.
2. **Peak-Buckets:** in Rust berechnen und über JNI reichen, oder in Kotlin?
    (Rust = konsistent mit CLI-`info`, Kotlin = weniger JNI-Verkehr)
3. ~~**WAV-Read:** `hound` behalten oder auch selbst?~~ → **selbst, entschieden
    und gebaut.** Reader und Writer teilen die Chunk-Ebene, `hound` bleibt als
    *dev*-dependency für die Gegenprobe in beide Richtungen. Null Runtime-Deps.
    Hat sich sofort bezahlt: das zu kurze RIFF-Size-Feld der Caustic-Exports
    fiel nur auf, weil das Parsen in eigener Hand lag.
4. ~~Wow/Flutter-Defaults~~ → **0,3 % Wow / 0,15 % Flutter**, je zwei Sinusse;
   Presetbarkeit noch offen (kommt mit `loopslcr.toml` in v0.4)
5. **iOS** — lohnt sich das, oder decken CLI + Android den echten Workflow ab?

## 9. Name — entschieden

**LOOP_SLCR.** Crates `loopslcr-core` / `loopslcr-cli` / `loopslcr-jni`,
Binary `loopslcr` — kleingeschrieben, wie Cargo es erwartet.

Die Kandidaten, gegen die entschieden wurde:

| Name | Kommentar |
|---|---|
| **TAKTSCHNITT** | Deutsch, passt zur HexaTakt/OktoTakt-Familie |
| **LOOPKLIPP** | kurz, DE/EN-Hybrid |
| **BARCUT** | international, langweilig-solide |
| **TAILFOLD** | benennt das Killer-Feature |
| **SCHNITTTAKT** | dreifach-T, sehr deutsch 😄 |
| **CSTC-CUT** | zu Caustic-spezifisch, schränkt ein |

---

## 10. Projektfamilie (Kontext)

| Projekt | Was | Status |
|---|---|---|
| **HexaTakt** | 16-Track JUCE Groovebox, VST3 + Standalone | existiert, Modul-Donor |
| **OktoTakt** | 8-Voice Rytm-style Drum Machine, JUCE | Architektur + Roadmap fertig |
| **DRUMOID** | simple Android Drum-App | Brainstorming, Stack offen |
| **LOOP_SLCR** | ← dieses Projekt, Utility statt Instrument | Timing-Core steht, M1 läuft |

LOOP_SLCR ist bewusst das **kleinste** Projekt der Familie — ein Tool, kein Instrument.
Guter Kandidat, um endlich mal etwas **fertig** zu veröffentlichen.

---

## 11. Nächster Schritt

1. ✅ Docs generiert: `BRAINSTORMING.md`, `ROADMAP.md`, `ARCHITECTURE.md`
2. ✅ GitHub-Repo angelegt (`LOOP_SLCR`), Docs unter `docs/` eingecheckt
3. ✅ Name festgelegt: **LOOP_SLCR**, Crates `loopslcr-*`, Binary `loopslcr`
4. ✅ Cargo-Workspace scaffoldet, Rust 1.97.1 via rustup
5. ✅ `rational.rs` + `timing/` mit Golden-Value-Tests — 28 Tests grün,
   bevor irgendein Audio angefasst wurde
6. ✅ CLI `loopslcr grid` — der audiofreie Teil von `--dry-run`
7. **Offen: WAV-Read entscheiden** (§8 Punkt 3) — blockiert den nächsten Schritt
8. Dann RIFF-Reader → `AudioBuffer` → `ops::cut` → `ops::foldback`
9. `--dry-run` über das Archiv → Annahmen validieren (M1-Exit)

### Was das Archiv über die echten Dateien verrät

Ein Sweep mit `loopslcr info` über alle 279 Einträge (Details siehe
`tests/archive_sweep.rs`, aktivierbar über `LOOPSLCR_ARCHIVE`):

| Befund | Zahl |
|---|---|
| WAVE-Dateien | **277** — zwei davon **ohne `.wav`-Endung** (`78-SMPL.BRN-…`) |
| Keine Audio-Dateien | 2 zip |
| Parse-Fehler | **0** |
| PCM 16-bit stereo | 141 |
| PCM 24-bit stereo | 75 |
| PCM 16-bit mono | 46 |
| PCM 32-bit stereo / mono | 8 / 5 |
| 44.1 kHz / 48 kHz | 239 / 36 |
| `WAVE_FORMAT_EXTENSIBLE` | **1** (`120-lilDRM-APRL09-02-Mstr 01.wav`) |
| Dateien mit echten Chunks | 9 — nur `LIST` und `smpl` |
| **`acid`-Chunks** | **0** |

**Konsequenzen:**

- **Kein `acid`-Chunk im ganzen Archiv.** Die Hoffnung, das Tempo aus der Datei
  zu lesen statt aus dem Namen, trägt für den Bestand nicht — `--bpm-from-name`
  bleibt der Hauptweg. Für die *Ausgabe* schreiben wir ihn natürlich trotzdem.
- **Nach Endung filtern wäre falsch:** zwei echte WAVs haben keine. Der Sweep
  prüft stattdessen die Magic Bytes `RIFF`/`WAVE`.
- **`WAVE_FORMAT_EXTENSIBLE` kommt real vor** — der Pfad ist kein Theoriefall.
- **Bar-Verteilung** über die 261 Dateien mit Tempo im Namen: 4 Bars (177),
  8 Bars (35), 16 Bars (15), 2 Bars (7) landen innerhalb 2 % auf einer ganzen
  Bar-Zahl. **Die 4-Bar-Loops dominieren**, nicht die 8-Bar-Loops — der
  Default `--bars 8` passt zum Referenzfall, aber nicht zur Mehrheit des
  Archivs. → **Konsequenz gezogen: es gibt keinen festen Default mehr.**
  `analysis::guess_loop_bars()` liest die Loop-Länge aus der Dauer der Datei
  selbst (Regel 1 „die Datei *ist* der Loop", Regel 2 „zwei Loops plus Tail",
  Regel 3 „ein Loop plus Tail"). Über das Archiv ergibt das 4 Bars (191),
  8 (40), 16 (15), 2 (13), 32 (1), 1 (1) — deckt sich mit der oben gemessenen
  Verteilung. `--bars` überschreibt weiterhin.
- **Der Tail wird beim Ableiten der Loop-Länge bewusst *nicht* herangezogen.**
  Würde Regel 1 verlangen, dass Audio bis ans Dateiende reicht, fiele ein
  sparsamer 8-Bar-Loop mit stiller letzter Bar auf „4-Bar-Warmup-Render" —
  und die Hälfte der Phrase wäre weg. Zwischen einem unnötigen „nichts zu tun"
  und einem still halbierten Loop ist der erste der Fehler, den man macht.
- **106 Archivdateien sind zu kurz für einen exakten Loop** — im Median um
  50 Frames, im schlimmsten Fall um 19 518. Ein früheres Tool hat abwärts
  gerundet. 50 Frames pro Wiederholung sind trotzdem Drift, also verweigert ein
  echter Lauf und benennt das Defizit; `--dry-run` berichtet es stattdessen,
  weil ein Archiv-Überblick genau dann gebraucht wird. `--allow-short` nimmt
  es bewusst in Kauf.
- **`smpl`-Loop-Endpunkt ist mehrdeutig** — ✅ **entschieden.** Die Spec sagt
  inklusiv, aber `58.5 DL_4BAR_Lumiko Imai 01.wav` hat 787 199 Frames und
  `end = 787199` — inklusiv gelesen zeigte der Loop ein Frame über das
  Dateiende hinaus. Der Encoder meint hier exklusiv.
  **Geschrieben wird die Spec** (`end = start + len - 1`), weil das die
  Konvention ist, die Sampler erwarten. **Gelesen wird tolerant:**
  `SampleLoop::frame_count()` liest inklusiv, nimmt aber ein `end`, das genau
  auf die Frame-Anzahl fällt, als exklusiv — der Off-by-one ist dann der des
  Encoders, und in die andere Richtung geraten hieße ein Klick im Loop-Punkt.
  Das rohe `end`-Feld bleibt unverändert lesbar; nur die Längenfrage ist
  entschieden.

### Was beim Bauen auffiel

- **Der Archivpfad ist bestätigt:** `IT'S_ME!/ALL STUFF OF ME/AUDIO/DRUMLOOPS/`
  enthält **exakt 279** WAVs.
- **`--bpm-from-name` braucht mehr als `^(\d{2,3})\b`.** Im Archiv liegen
  `102-MTRX-01.wav` (kein Wortende nach der Zahl, `\b` greift nicht wie gedacht),
  `105CSTC-APRL02-…`, `00005 136BPM E01…` (Marker statt führend) und
  **`58.5 DL_4BAR_…` mit Dezimal-Tempo** — auf 58 gekürzt liegt die Datei
  daneben, mit 58.5 stimmt sie. 14 Dateien tragen gar kein Tempo im Namen.
  → **Gebaut als `naming::tempo_from_name()`**, ohne Regex (null Dependencies).
  Was die Fälle auseinanderhält, ist **Plausibilität**, nicht Position: eine Zahl
  gilt nur als Tempo, wenn sie in 40..=300 liegt. Damit fallen Datum
  (`29Jul26`), Take-Nummer (`01`), Bar-Zahl (`4BRS`) und Caustics `00005` von
  selbst heraus. Ein explizites `BPM` schlägt die Position. Über das Archiv:
  **264 mit Tempo, 15 ohne** — und alle 15 tragen wirklich keins (eines ist
  eine `.zip`). Dazu `naming::bars_from_name()` für `4BRS` / `2BARS` / `8 BRS`.
- **§3c Restfehler-Angabe präzisiert:** beim 103-BPM-Fall ist das Residual exakt
  **−26/103 Samples ≈ −5.724 µs ≈ −0.307 ppm**. Die Doku sagte „0.25 Samples
  ≈ 5.7 µs ≈ 0.3 ppm" — richtig gerundet, aber das Vorzeichen fehlte
  (es wird abgerundet) und der exakte Bruch ist 26/103, nicht 1/4.
- **6/8 mit BPM-Einheit 1/4 ist bar-identisch mit 3/4** (beides 3 Viertel pro
  Bar). Erst `--bpm-unit 3/8` macht daraus 2 punktierte Viertel — und der Bar
  wird dabei **kürzer**, Faktor 2/3, nicht länger. Steht als Test drin.
- **Overflow panict auch im Release**, nicht nur in Debug wie in ARCHITECTURE
  §3.1 vorgesehen: gegen `i128` kosten die Checks nichts messbar, ein stiller
  Wrap würde dagegen einen Cut-Punkt unbemerkt verfälschen.
- **`#![deny(clippy::float_arithmetic)]`** auf dem Core macht Invariante 2
  („Floats nur in der Sample-Domäne") build-geprüft. Drei einzeln begründete
  Ausnahmen in der Timing-Domäne, alle reine Anzeige: `Rational::to_f64`,
  `Residual::micros`, `Residual::ppm`. `buffer.rs` und `wav/read.rs` erlauben
  Floats modulweit — sie *sind* die Sample-Domäne, und das Attribut markiert
  die Grenze sichtbar.
- **CLI panickte bei `| head`.** Rust ignoriert SIGPIPE per Default, also
  knallt jedes `println!` in eine geschlossene Pipe. Behoben: Ausgabe geht
  über einen Puffer, `BrokenPipe` beendet sauber mit Exit-Code 0.

---

## 12. Android — was beim Bauen entschieden wurde

- **Der Emulator hat die zweite ABI erzwungen.** Geplant war `arm64-v8a` allein.
  Ohne `x86_64` läuft die App aber auf keinem Emulator, und damit lässt sich das
  APK nur von Hand auf einem Gerät prüfen — also praktisch gar nicht. Ein halbes
  MB für automatisch prüfbare Builds ist keine Abwägung. `/dev/kvm` ist auf
  dieser Maschine für alle schreibbar, der Emulator läuft also beschleunigt,
  ohne jemanden in die `kvm`-Gruppe zu nehmen.
- **`Native.java` liegt in der App, nicht im Crate.** Sie ist die Deklaration,
  auf die die Symbolnamen in `lib.rs` passen müssen. Eine zweite Kopie im
  `tests/java` des Crates wäre eine zweite Sache, die stimmen muss — und die
  driftende wäre die, die kein Test kompiliert. Der JVM-Brückentest übersetzt
  jetzt die Datei aus `android/`.
- **AGP 8.13.2 und AndroidX bewusst eine Generation zurück.** Die neuesten
  AndroidX-Releases verlangen AGP 9 und `compileSdk` 37. Das hieße neuer
  Gradle-Major, neue Plattform, neue Plugin-APIs — für nichts, was diese App
  benutzt. Die gepinnten Versionen sind die letzten, die gegen `compileSdk` 36
  bauen.
- **`ndkVersion` im `android {}`-Block ist nicht kosmetisch.** Ohne sie findet
  AGP `llvm-strip` nicht, gibt das Strippen auf und sagt es nur in einer
  Warnung. Die Bibliothek war dadurch 758 KB statt 534 KB.
- **Kein Default wird nach Kotlin geschrieben.** `Settings.toJson()` lässt jedes
  Feld weg, das auf seinem Default steht. Die native Seite weiß bereits, was ein
  fehlendes Feld bedeutet; es mitzusenden würde die Kotlin-Klasse zu einem
  zweiten Ort machen, an dem Defaults stehen.
- **`plan` läuft die ganze Pipeline.** Es resampelt die Datei, um zu sagen, was
  herauskäme — also darf es nicht pro Pixel eines Sliders laufen. 250 ms Ruhe
  vor dem Neurechnen sind der Unterschied zwischen einer UI, die antwortet, und
  einer, die stockt.
- **Detent bei 0 im Varispeed.** Ohne ihn hinterlässt jede Fingerbewegung ein
  Verhältnis von 1.003: kostet ein Resampling und bringt nichts.
- **Der Panik-Test wurde auf Android wiederholt.** Auf dem Host war er schon
  bewiesen, aber Android hat eine andere Runtime und einen anderen Unwinder —
  und dort wird die Zusage tatsächlich gebraucht.
- **Der Instrumented-Test prüft auch Invariante 4 auf dem Gerät:** zweimal
  dieselben Einstellungen, bitgleiche Bytes. Reproduzierbarkeit, die nur auf der
  Maschine gilt, die sie gemessen hat, ist keine.

---

## 13. Preview-Engine — die Entscheidungen

- **Der Varispeed wird bewusst *nicht* eingebacken.** `preview::create` setzt
  `ratio`, `target_bpm` und `snap` auf neutral, bevor die Pipeline läuft. Sonst
  müsste bei jeder Fingerbewegung der ganze Loop neu geschnitten und resampelt
  werden — das genaue Gegenteil dessen, wofür eine Vorschau da ist.
- **Tape-Character läuft dagegen mit, bei Unity.** Der Export legt ihn bei der
  Endgeschwindigkeit an, wo die Filter tiefer sitzen. Eine stark transponierte
  Vorschau ist also etwas heller als das Rendering. Das zu benennen ist besser,
  als es zu verschweigen oder bei jedem Zug neu zu bauen.
- **Zwei Kanäle zwischen den Threads, absichtlich asymmetrisch.** UI → Audio ist
  ein Atomic (`set_ratio` speichert `f64`-Bits und kehrt zurück, nie blockierend).
  Audio → UI sind zwei weitere Atomics für Position und gespielte Frames, damit
  die UI einen Zeiger zeichnen kann, ohne den Audio-Thread nach irgendetwas zu
  fragen. `Relaxed` reicht überall: die Werte stehen für sich, es gibt nichts,
  wogegen sie geordnet werden müssten.
- **Der Mutex um `Preview` ist unumstritten by construction.** Nur der lesende
  Thread nimmt ihn. Er steht da, damit der Typ sicheres Rust ist statt einer
  `UnsafeCell` mit Kommentar — Kosten: ein Atomic-Exchange pro Block, nicht pro
  Sample.
- **Die Reihenfolge beim Stoppen ist die kritische Stelle.** `stop()` joint den
  Audio-Thread, *bevor* es `previewDestroy` ruft. Andersherum wäre es ein
  Use-after-free mit genau einem Block Fensterbreite — also selten, auf einem
  Gerät, und sähe aus wie ein Zufallscrash. Der Join ist auf 2 s begrenzt: ein
  unbegrenzter würde den UI-Thread aufhängen, wenn der Audiopfad klemmt.
- **`WRITE_BLOCKING` taktet die Schleife selbst.** `AudioTrack.write` kehrt
  zurück, wenn das Gerät Platz hat — der Thread läuft also exakt im Tempo der
  Hardware und braucht keine eigene Uhr.
- **Position wird pro Sample gewrappt, nicht pro Block.** Bei 48 kHz würde eine
  über eine Stunde akkumulierende Float-Position Nachkommastellen verlieren, und
  ausgerechnet an der Naht wäre das hörbar.
- **Acht Sinc-Taps statt zweiunddreißig.** Der Stoppband fällt von etwa −90 dB
  auf etwa −60, was unter einem Drumloop nicht hörbar ist, und kostet ein
  Viertel der Arbeit pro Sample. Der Offline-Pfad hat keine Deadline, dieser hier
  schon.
- **Die Glide-Zeit von 120 ms ist ein Gefühl, keine Messung.** Bandmaschinen
  streuen weit stärker. Sie ist Parameter, damit man mit ihr streiten kann.

---

## 14. Calculator-Tab — warum nativ

- **Kein Rechenschritt liegt in Kotlin.** Der Tab fragt Rust und stellt die
  Antwort dar. Eine Nachimplementierung hätte eine Weile übereingestimmt und
  dann, bei irgendeinem Tempo, das niemand getestet hat, still nicht mehr — und
  genau darin richtig zu sein ist die ganze Aufgabe.
- **`Grid::seconds_per_whole_notes` ist der gemeinsame Ausdruck.** Ein Takt ist
  `whole_notes_per_bar` davon, ein Notenwert sein eigener Bruchteil einer ganzen
  Note. Punktierung ist ×3/2, Triole ×2/3, beides exakt. So kommen beide Tabs
  über dieselbe Formel zu ihren Zahlen statt über zwei, die sich ähneln.
- **Ein Test hat einen echten Unterschied aufgedeckt:** 16 Takte ab null runden
  auf 1 644 117, der 8-Takt-Loop des Cutters ab Takt 8 endet auf 1 644 116.
  Beides stimmt — `Align::Loop` macht den Loop `round(8 · spb)` lang ab einem
  separat gerundeten Start, statt die Differenz zweier gerundeter Taktlinien zu
  nehmen. Das eine Sample Unterschied *ist* die Alignment-Entscheidung.
- **Die Sample-Genauigkeit pro Zeile ist der einzige Eintrag, der keine
  Dekoration ist.** Eine Delayzeit, die auf einem ganzen Sample landet, bleibt
  beliebig lange im Raster; eine andere driftet heraus. Bei 103 BPM ist keine
  einzige Zeile exakt, und das zu sehen ist der Zweck.
- **Der JSON-*Writer* kann jetzt Arrays, der *Parser* nicht.** Eine Tabelle ist
  nun mal eine Tabelle; sie in `note.16.ms`-Schlüssel zu flachklopfen hieße,
  Struktur in Namen zu kodieren — wovor ein selbstbeschreibendes Format gerade
  schützen soll. Die Eingaberichtung bleibt flach, der Parser bleibt eine
  Grammatik, die man vollständig implementieren kann. Rust liest die eigene
  Ausgabe nie zurück; der Empfänger ist `org.json`.
- **`System.loadLibrary` gehört in `Native`, nicht in einen Kotlin-Wrapper.**
  Der Rechner erreicht `Native`, ohne je `Engine` zu berühren — und fand die
  Lücke sofort, als `UnsatisfiedLinkError` beim ersten Aufruf. Jetzt lädt der
  statische Initialisierer der Klasse, die die Methoden deklariert; damit gibt
  es keinen Pfad, der es vergessen kann.

---

## 15. Ziehbare Marker und der Release-Build

- **Die Marker rasten auf Taktlinien ein.** Das ist keine Bequemlichkeit,
  sondern die einzige Art, wie ein Finger überhaupt an einen Schnittpunkt darf.
  Die Schnittpunkte kommen aus exakter Bruchrechnung über einen Takt-*Index*;
  ließe man eine Fingerspitze ein beliebiges Frame benennen, bekäme der Loop
  eine Länge, die kein Tempo teilt — genau die Drift, gegen die das Tool
  gebaut ist. Ein Zug wählt also einen Takt, den Rest macht das Raster.
- **Die Taktlänge zum Umrechnen Pixel → Takt kommt aus dem Plan und ist
  gerundet.** Das ist in Ordnung: sie zeigt, sie schneidet nicht. Egal auf
  welchem Takt der Finger landet, der Schnitt selbst kommt aus dem exakten Grid.
- **Welcher Marker gegriffen wurde, wird einmal beim Drag-Start entschieden.**
  Pro Bewegung neu zu entscheiden hieße, dass ein schneller Zug die Geste
  mittendrin an den anderen Marker übergibt und die Loop-Enden vertauscht.
- **R8 ist die Stelle, an der eine JNI-App normalerweise bricht.** Nichts im
  Kotlin ruft `Java_org_loopslcr_Native_analyze` — das tut der Linker zur
  Laufzeit, indem er ein Symbol gegen Klassen- und Methodennamen matcht. Ohne
  Keep-Regeln schrumpft der Build tadellos, installiert tadellos und wirft bei
  der ersten Datei. Zwei Regeln, weil sie verschiedene Fragen beantworten: die
  Klasse behalten (Name bleibt) und die Member behalten (Methodennamen bleiben).
- **Das Test-APK wird separat geschrumpft.** `proguardFiles` gilt der App,
  die Instrumentierung bekommt ihren eigenen Durchlauf über `testProguardFiles`.
  Das hat mich überrascht, deshalb steht es in einer eigenen Regel-Datei.
- **`-PtestRelease` schaltet `testBuildType` um**, damit die ganze Suite gegen
  den geschrumpften Build laufen kann. Eine falsche Keep-Regel fällt genau dort
  auf und sonst nirgends.
- **Der Signaturschlüssel liegt nicht im Repo und wird es nie.** `build.gradle.kts`
  liest `keystore.properties`, falls vorhanden; fehlt sie, ist der Release-Build
  eben unsigniert und beweist die Shrinker-Regeln trotzdem.

- **Die Marker-Arithmetik ist nach `Markers` herausgezogen und getestet.** Sie
  ist das einzige Rechnen in der ganzen App, das nicht in Rust passiert. Sie
  produziert nie einen *Schnittpunkt*, nur `skip` und `bars` — schlimmstenfalls
  also den falschen Takt, nie eine driftende Länge. Trotzdem getestet, weil
  „schlimmstenfalls" die Sorte Behauptung ist, die still aufhört zu stimmen.

---

## 16. Was auf einem echten Telefon schiefgeht

- **`OutOfMemoryError` ist ein `Error`, kein `Exception`.** Das `catch (e:
  Exception)` im ViewModel ließ ihn durch, also starb die App statt eine
  Meldung zu zeigen. Genau das Muster von „manchmal ein Fehler": es hängt an
  der Dateilänge, nicht an der Bedienung.
- **Die Größenordnung:** Die Datei wird ganz gehalten und in 64-Bit-Samples
  dekodiert. Fünf Minuten Stereo bei 48 kHz sind ~230 MB, *bevor* die Pipeline
  eine einzige Kopie macht. Die Referenzdatei (11 MB) wird zu 30 MB — harmlos;
  eine lange Aufnahme nicht.
- **Drei Gegenmaßnahmen, keine davon die eigentliche Lösung:** `largeHeap`,
  Datei direkt in einen Direct-Buffer lesen (der Java-Heap hält jetzt gar keine
  Kopie mehr), und `Throwable` fangen mit einer Meldung, die sagt *warum*.
  Die eigentliche Lösung wäre streamen statt ganz dekodieren — das ist ein
  eigenes Stück Arbeit und steht in der Roadmap.
- **Ein `OutOfMemoryError` zu fangen ist normalerweise falsch**, weil der
  Zustand danach unbekannt ist. Hier vertretbar aus einem Grund: alles, wozu
  die Allokation gehörte, wird auf demselben Pfad fallengelassen. Datei,
  dekodierter Puffer und Plan sind weg, übrig bleibt eine UI ohne offene
  Datei — genau der Zustand, in dem die App startet.
- **Die Kapazität des Direct-Buffers ist, was die native Seite liest**, nicht
  Position oder Limit. Ein Puffer mit Reserve würde Rust angehängte Nullen als
  Audio unterschieben — deshalb wird die Größe vorher beim Provider erfragt
  und exakt so viel alloziert, und ein zu kurz gelesener Puffer ist ein Fehler
  statt stiller Stille.
- **`-PtestRelease` brauchte eine eigene Regeldatei, und die kostet
  Aussagekraft.** Das Test-Framework löst Klassen über den Classloader der App
  auf, und R8 hatte alles entfernt, was die App selbst nicht benutzt:
  `androidx.tracing.Trace`, `kotlin.LazyKt`, `MonotonicFrameClock$DefaultImpls`,
  `mutableIntObjectMapOf`. Dazu zieht R8 kleine Kotlin-Objekte inline und
  löscht die Klasse — `Engine`, `Calculator` und `Markers` verschwanden so.
  Konsequenz sauber benannt: der Lauf beweist die JNI-Regeln, das gepackte
  `.so` und das signierte APK — nicht das geschrumpfte Kotlin.

---

## 17. Der Samsung-Fehler, aufgeklärt

Zwei getrennte Sachen, beide bestätigt durch den Screenshot vom Gerät.

- **`h1 was cancelled`** ist eine `CancellationException` aus Kotlin-Coroutines,
  im Release-Build zu `h1` obfuskiert. Mein `catch (e: Throwable)` fing den
  Abbruch mit und zeigte ihn als Fehler. Jede Einstellungsänderung bricht die
  laufende Neuberechnung ab — daher „manchmal". Zwei Dinge waren falsch: den
  Abbruch als Fehler zu melden, und ihn zu schlucken statt weiterzuwerfen. Wer
  eine `CancellationException` schluckt, lässt die Coroutine-Maschinerie
  glauben, ein abgebrochener Job lebe noch.
- **Die Latenz war ein Designfehler.** `plan` ließ die *ganze* Pipeline laufen,
  inklusive Resampling, nur um Zahlen zu melden. Gemessen auf der Referenzdatei:
  **6,6 s** mit Varispeed gegen **0,065 s** ohne. Auf einem Telefon leicht das
  Zehnfache — pro Reglerbewegung.
- **`pipeline::Stage::Plan`** hält nach den Entscheidungen und den billigen
  Operationen an: Cut und Fade sind Kopien, Resampling, Tape und Dither
  entfallen. Gemessen: **33 ms gegen 6,26 s**, Faktor 189.
- **Was dabei exakt bleibt:** alles Zeitliche. Die Ausgabelänge kommt aus
  `grid.resampled_length` statt aus `buffer.frames()`, ist also in beiden Stufen
  dieselbe Zahl. Ein Test vergleicht Feld für Feld.
- **Was es kostet, und wo es steht:** der Peak wird vor dem Varispeed gemessen.
  Der Resampler überschwingt um Bruchteile eines dB — die fehlen. Steht als
  `peakBeforeVarispeed` im JSON und als „(before varispeed)" auf dem Schirm.
  Der Normalisierungsfaktor dagegen ist keine Schätzung: `gain::normalize`
  rechnet genau diese Division, bevor es ein Sample anfasst.
- **Der CLI-Dry-Run bleibt vollständig.** Dort ist der exakte Peak vor dem
  Schreiben sechs Sekunden wert; auf dem Telefon bewegt sich ein Finger.

---

## 18. Die Lücke, die beim Nachsehen auffiel

- **Die App konnte kein Quell-Tempo setzen.** `Settings` hatte kein `bpm`,
  `sig`, `bpmUnit` oder `align` — obwohl die JNI-Fläche sie längst annahm.
  Folge: eine Datei ohne Tempo im Namen und ohne `acid`-Chunk war auf dem
  Telefon *unbenutzbar*. Die Pipeline verweigert sie zu Recht, und es gab
  nichts, womit man hätte antworten können. Im Archiv betrifft das 14 von 279.
- **Das Feld bleibt leer, solange die Datei es selbst weiß.** Ein vorgefülltes
  Feld lädt dazu ein, etwas zu ändern, das schon stimmte. Was gerade gilt,
  steht als `supportingText` darunter — nicht als Placeholder, denn Material
  zeigt den erst bei Fokus, und „was sagt die Datei" will man wissen, *bevor*
  man tippt.
- **Die Varispeed-Anzeige kommt aus dem Plan, nicht aus dem Regler.** Das
  Verhältnis ist oft ein Bruch, den der Slider nur annähert; 90/103 ist die
  Zahl, die zählt. Drei Einheiten nebeneinander, weil es ein Wert unter drei
  Namen ist: Halbtöne für Musiker, Prozent für die Bandmaschine, BPM für den
  Sequencer.
- **Ein Test war falsch, nicht der Code:** bei halbem Tempo ist der Takt doppelt
  so lang, dieselbe Datei liest sich als 4 statt 8 Takte — gleiche Frames,
  anderes Raster darunter. Dass die *Anzahl* sich ändert, ist der Beweis, dass
  die Überschreibung im Grid ankam.

---

## 19. Warum sich die Marker nicht ziehen ließen

Vom Gerät gemeldet, und es waren zwei Fehler in einem.

- **Der Gesten-Handler brach durch seine eigene Wirkung ab.**
  `Modifier.pointerInput(frames, region.first, region.last)` — auf die Region
  geschlüsselt. `pointerInput` startet neu, sobald ein Key sich ändert, und ein
  Neustart bricht die laufende Geste ab. Der Zug verschiebt die Region, also
  riss er sich selbst ab: der Finger bewegte sich weiter, es folgte nichts.
  Jetzt auf `frames` geschlüsselt (ändert sich nur bei einer anderen Datei),
  die aktuelle Region kommt über `rememberUpdatedState` herein.
- **Die Linie folgte der Pipeline statt dem Finger.** Gezeichnet wurde aus dem
  Plan — also 250 ms Debounce plus nativer Aufruf hinter der Berührung, mit
  Gummiband-Effekt, wenn die Antwort ankam. Solange gezogen wird, sitzt der
  Marker jetzt am Finger, gerastet auf den Takt, auf dem er landen wird. Beide
  Richtungen gehen durch `Markers.barAt` / `fractionOfBar`, damit die Linie
  keine Position verspricht, die das Commit anders rundet.
- **Griffe statt Linien.** Zwei Pixel sind zum Anschauen, nicht zum Anfassen.
  Der gehaltene Marker wird grün und breiter, damit der Finger weiß, welchen er
  erwischt hat.
- **Der erste Regressionstest fing den Fehler nicht** — und das war die
  lehrreichste Stelle. `performTouchInput` schickt eine ganze Gestenfolge in
  einem Block ab, ohne dass Compose dazwischen neu zeichnet; der Handler sah den
  Zustand also nie, an dem er zerbrach. Erst aufgeteilt in einen Block pro
  Bewegung mit `waitForIdle()` dazwischen — wie es in echt passiert — schlug er
  an: **„only 1 move(s) arrived"** von vier. Gegen die reparierte Fassung grün.
  Ein Regressionstest, den man nicht gegen den Fehler laufen gesehen hat, ist
  eine Vermutung.

---

## 20. Was ein Bildschirmvideo vom Gerät zeigte

Ein GIF von der echten Datei, und darin drei Dinge, die kein Emulator-Test
hergegeben hätte.

- **Die Abdunklung außerhalb des Schnitts fehlte — seit der ersten Fassung.**
  Sie wurde *vor* der Waveform gezeichnet, also malten die Balken sie zu. Auf
  dem Emulator fiel es nie auf, weil dort die Region immer die ganze Datei war:
  es gab nichts außerhalb, das hätte dunkel sein müssen. Erst eine echte Datei
  mit einem Teilschnitt zeigt es.
- **Der Test dafür prüft jetzt Pixel**, nicht Struktur: die mittlere Helligkeit
  einer Spalte außerhalb muss unter 80 % der Spalte innerhalb liegen. Gegen den
  alten Zeichenfehler laufen gelassen: **145,22 gegen 145,23** — identisch, also
  gar keine Abdunklung. Genau das, was ein Screenshot-Vergleich durchgehen ließ.
- **Der gehaltene Marker trug die Farbe des Playheads.** Beide können
  gleichzeitig zu sehen sein; zwei Dinge, die Verschiedenes bedeuten, dürfen
  nicht gleich aussehen. Gehalten ist jetzt hellblau.
- **Die Griffe waren in Pixeln bemessen.** Sechzehn Pixel sind auf einem Telefon
  anderthalb Millimeter — eine Markierung, kein Ziel. Jetzt in dp.
- **`peak 1.4998 — clips`** beim Foldback: erwartet und richtig gemeldet, denn
  der Foldback addiert den Tail auf den Kopf. Nur stand daneben kein nächster
  Schritt. Jetzt bietet die Plan-Karte in dem Moment „Normalize" als Knopf an.
- **Der Drag funktioniert:** im Video stehen Taktzahlen wie 5, 7, 10 und 12, die
  auf keinem Chip liegen — die können nur aus einem Zug stammen.

## 21. Warum sich die Vorschau bei Tape überschlug

Gemeldet als: „wenn ich Tape einstelle, fängt er an sich zu überschlagen/
überlappen". Es war wörtlich zu nehmen — die Schleife lief zweimal
gleichzeitig, phasenverschoben.

**Die Kette.** Ein Neuaufbau der Vorschau lässt die ganze Pipeline laufen und
dauert auf dem Telefon länger als die 250 ms Debounce des Replans. Also startete
der zweite Neuaufbau, während der erste noch lief. `PreviewPlayer.start` begann
mit `stop()`, und dieses Lesen-dann-Ersetzen über vier Felder war ungeschützt:
der zweite Aufrufer fand `pump` und `track` noch `null`, weil der erste sie noch
nicht zugewiesen hatte, räumte also nichts weg und baute einen **zweiten
AudioTrack mit zweitem Pump-Thread** neben den ersten. Die Felder merkten sich
danach nur den späteren — der frühere war nie wieder abschaltbar und las
außerdem ein Handle, das das `stop()` des Überlebenden freigeben würde.

**Warum nur Tape.** Wow und Flutter sind die einzigen *stufenlosen* Regler, die
die Vorschau ungültig machen. Jede andere Einstellung, die das tut, ist ein Chip
oder ein Schalter — ein Ereignis, kein Strom. Und Varispeed macht die Vorschau
gar nicht ungültig, weil sie absichtlich nicht eingebacken ist (Abschnitt 13).
Der Fehler konnte also gar nichts anderem gehören.

**Der Fix, in drei Teilen.**

- **Monitor auf `start` und `stop`.** Damit ist die Invariante lokal in der
  Klasse und hängt nicht daran, dass jeder Aufrufer sich benimmt.
- **Bauen außerhalb des Monitors.** `previewCreate` läuft so lange wie ein
  Schnitt; hielte man das Schloss darüber, müsste ein Tippen auf Pause sekunden-
  lang auf eine Vorschau warten, die es gerade wegwerfen will. Gebaut wird frei,
  eingehängt unter dem Schloss — Mikrosekunden.
- **Eine Warteschlange im View-Modell.** Ein `Mutex` plus abbrechbarer
  `rebuild`-Job. Abbruch allein genügt nicht: eine abgebrochene Coroutine hält
  einen bereits laufenden nativen Aufruf nicht an, sie kehrt zurück und wirft
  erst auf dem Rückweg. Aufreihen heißt, der spätere Aufbau hängt zuletzt ein —
  und das ist der, den der Finger gemeint hat.

**Dazu die Position.** Ein Neuaufbau begann bei null, also triggerte jede
Reglerstufe den Klang neu — als Stottern gehört, nicht als Regelung. Jetzt wird
die Spielposition über den Neuaufbau hinweg gehalten; `seek` wickelt, also ist
es immer eine Position, die die neue Vorschau auch hat.

**Der Test zählt Threads, nicht Töne.** Die Audioausgabe eines Emulators ist
keine Behauptung wert, der Thread, der sie füttert, aber sehr wohl: vier Threads
starten gleichzeitig, danach muss genau **ein** `loopslcr-preview` leben. Gegen
den Fehler laufen gelassen sagte er `expected:<1> but was:<4>`.

## 22. Der Build hing am System-JDK

Am 21.07.2026 ging das System-JDK auf 26.0.2, und der Build hörte auf zu
übersetzen — mit `java.lang.IllegalArgumentException: 26.0.2` und sonst nichts.
Es ist Gradles *eigener* Kotlin-DSL-Compiler, der die Versionsnummer nicht lesen
kann, und zwar beim Übersetzen der `.gradle.kts`-Dateien selbst. Deshalb hilft
keine Einstellung im Projekt: sie käme zu spät.

Zwei Ebenen, getrennt gehalten:

- **Der Daemon** läuft über `org.gradle.java.home` auf einem JDK 21. Das steht in
  der *benutzereigenen* `~/.gradle/gradle.properties`, nicht im Repository —
  ein absoluter Pfad einer Maschine gehört nicht in ein geteiltes Projekt.
- **Die Compiler** laufen über `jvmToolchain(17)` auf einem JDK, das Gradle sich
  selbst holt (foojay-Resolver in `settings.gradle.kts`). Das *gehört* ins
  Repository: ein Build, der nur mit den Systempaketen einer bestimmten Woche
  funktioniert, ist kein Build.

## 23. Der Tail-Modus, sichtbar gemacht

Der Kern hat immer richtig zwischen den beiden Pfaden gewählt und immer
gemeldet, was er gewählt hat. Was er nicht konnte: die Wahl *lesbar* machen. Auf
dem Bildschirm stand `workflow: unclear (detected unclear)` — eine Feststellung,
gegen die man nicht argumentieren kann.

- **Die Knöpfe heißen jetzt nach der Handlung, nicht nach der Datei.** `warmup`
  und `foldback` beschreiben, wie die Datei gerendert wurde; `cut (A)` und
  `fold (B)` beschreiben, was das Werkzeug tun wird — und wer einen Knopf
  drückt, wählt eine Handlung.
- **Die Begründung steht daneben.** `audibleBars` geteilt durch die Loop-Länge
  ist genau das Verhältnis, gegen das `WorkflowGuess::detect` seine Schwellen
  schreibt: etwa zwei für einen Warmup-Render, etwa eins für eine Datei, die
  einen Foldback braucht. Diese Zahl wird jetzt mitgeliefert und angezeigt, denn
  ohne sie ist die Erkennung ein Befehl statt eines Arguments.
- **Ein Rückfall wird ausgesprochen.** Bei `unclear` nimmt der Kern absichtlich
  den geraden Schnitt — ein falscher Schnitt ist hörbar und wiederholbar, ein
  falscher Foldback verdoppelt still die Tails. Das ist eine Entscheidung
  stellvertretend für den Benutzer, also darf sie nicht stumm sein.
- **Ein Widerspruch auch.** Wer von Hand gegen die Erkennung wählt, darf das —
  vielleicht weiß er etwas, das die Datei nicht sagt. Die wahrscheinlichere
  Ursache ist aber ein Chip, der von der vorigen Datei stehen geblieben ist.

Die Regel dahinter, in `Paths.kt` festgehalten: *die App darf für den Benutzer
entscheiden, aber nicht so, dass er es nicht sieht, nicht versteht und nicht
überstimmen kann.*

## 24. Drei Dinge aus einem zweiten Screenshot

- **Der Pitch-Regler antwortete erst beim Loslassen.** Das Verhältnis kam
  ausschließlich aus dem Plan, also hinter 250 ms Debounce plus nativem Aufruf.
  Dabei ist es lokal trivial: `2^(st/12)`, oder Zieltempo durch Quelltempo. Das
  geht jetzt sofort an den Handle, der Plan überschreibt es später mit dem
  exakten rationalen Wert, und die 120-ms-Glide macht die Korrektur unhörbar —
  gemessen liegt sie unter 1e-9 relativ, ein Cent sind 0,0578 %.
  **Diese Zahl darf nie in eine Datei.** Sie existiert zwischen einem bewegten
  Finger und der antwortenden Pipeline, sonst nirgends.
- **Das Raster braucht keinen BPM-Detektor.** `samplesPerBar` liegt im Plan
  bereits vor, also stehen die Taktlinien exakt dort, wo ein Marker einrasten
  würde — dieselbe Arithmetik, nicht eine zweite. Jede vierte Linie ist heller,
  weil vier Takte die Phrase sind, auf der praktisch das ganze Archiv steht.
  Unter vier Pixeln Abstand wird gar nichts gezeichnet: ein Raster, das zum
  Schleier wird, liest sich als Teil des Signals, und eine leise Stelle sähe
  voller aus, als sie ist. Nichts schlägt einen Farbton, der falsch informiert.
- **Die Dateizahlen lagen im Weg.** Sie standen zwischen Waveform und jedem
  Bedienelement, also war das Erste, was man ändern konnte, einen Scroll weit
  weg. Jetzt liegen sie hinter dem Dateinamen, einen Tipp entfernt. Auf dem
  Bildschirm bleibt, woraus eine *Entscheidung* gefällt wird.

## 25. Der Bildschirm nach dem, was man wirklich drückt

Zwei Screenshots mit eingezeichneten Pfeilen, und der Punkt war beide Male
derselbe: die Ecke gehört dem, was am häufigsten gedrückt wird.

- **Play sitzt jetzt oben rechts, wo Open war.** Eine Datei wird einmal gewählt
  und dann minutenlang gehört. Play ist außerdem das, wonach man greift,
  *während* man die Waveform ansieht — also gehört es neben sie, nicht darunter.
- **Open ist in die Info-Klappbox gewandert.** Es wirft jede Einstellung auf dem
  Bildschirm weg und lag vorher unter dem Daumen.
- **Die Abschnitte klappen zu.** Der Bildschirm ist eine einzige Spalte, und die
  Varispeed lag fünf Gruppen tief: einen Regler erreichen hieß, die Waveform
  oben rausschieben — genau das, was man beim Schieben ansehen will. Quelle und
  Ausgabe sind jetzt zugeklappt vorbelegt (beides wird einmal gesetzt), die
  Regler offen. `rememberSaveable`, damit eine Drehung nicht aufreißt, was
  jemand zugeklappt hat.

**Die Regel, die das Zuklappen überhaupt zulässig macht: Detail darf
verschwinden, Ärger nicht.** Die zugeklappte Plan-Karte trägt weiter `clips`
beziehungsweise `short by N`. Eine Karte, die „clips" verschlucken kann, wäre
schlechter als eine, die sich nicht zuklappen lässt — denn die verborgene Zahl
war der Grund hinzusehen. Gegen eine blind gemachte Karte laufen gelassen:
`AssertionError: Failed: assertExists.`

## 26. Ein Release-Build, der die falsche Wahrheit meldete

Gemeldet als „ist immer noch wie vorher". Es war auch so: der Release-Build hatte
`BUILD SUCCESSFUL` gemeldet und das APK des *vorherigen* Commits ausgeliefert.

Das verräterische Zeichen war da und wurde übersehen — **exakt dieselbe
Byte-Zahl** wie beim Build davor, obwohl neue Zeichenketten hinzugekommen waren.
Zwei gleich große Artefakte aus zwei verschiedenen Commits sind kein Zufall,
sondern eine Meldung.

Nachgewiesen wurde es, indem das APK selbst gefragt wurde statt der Erfolgs-
meldung. Wichtig dabei: `strings` auf das APK anzuwenden findet **nichts**, weil
das DEX im Zip komprimiert liegt — erst entpacken, dann suchen:

```console
$ unzip -q -o app-release.apk 'classes*.dex'
$ strings classes*.dex | grep -F "Open another file"
```

Vorher fehlte die Zeichenkette, nach einem `clean` war sie da.

Daraus zwei bleibende Änderungen:

- **`versionName` trägt den Commit.** `0.1.0+339ab7a`, aus `git rev-parse` beim
  Bauen, sichtbar in der Info-Klappbox neben der Engine-Version. Wenn „ist das
  der neue Build?" eine Frage ist, kostet Raten mehr Zeit als Hinschreiben.
- **Die Regel für mich: einer Erfolgsmeldung nicht glauben, wenn das Ergebnis
  prüfbar ist.** Bei einem APK ist es prüfbar.

### 26a. Derselbe Fehler noch einmal — und was der Stempel nicht kann

Zwei Runden später wieder „hat sich wie nichts verändert", und wieder zu Recht:
das APK trug `0.1.0+38bb3c4`, während HEAD auf `aebd2e8` stand. Berichtet worden
war `0.1.0+aebd2e8`. Der Stempel hat den Fehler diesmal *gefunden* — geglaubt
worden war trotzdem der Bericht statt dem Artefakt.

Zwei Dinge daraus:

- **Die Byte-Zahl taugt nicht als Zeuge.** Die beiden Builds waren auf das Byte
  gleich groß (2 417 311) — bei zwei *verschiedenen* Commits. Was zählt, ist eine
  Zeichenkette, die es vorher nicht gab.
- **Ein Commit-Hash allein sagt nichts über die Quellen.** Wird gebaut, bevor
  committet wird, nennt der Stempel den Vor-Commit und sieht dabei sauber aus.
  Deshalb trägt `versionName` jetzt ein **`+dirty`**, sobald `app/src` von dem
  abweicht, was der genannte Commit enthält — genau der Zustand, in dem der
  Stempel lügt. Und die Reihenfolge ist: **erst committen, dann bauen, dann
  ausliefern.**

## 27. Der Pitch-Regler wandert unter die Waveform

Er ist das einzige Bedienelement, das man *während des Hörens* festhält, und lag
fünf faltbare Gruppen tief in einer scrollenden Spalte — ihn zu benutzen schob
also genau das Bild aus dem Blick, auf das er wirkt. Jetzt steht er direkt unter
der Waveform, mit seinem Wert und dem Zieltempo daneben.

Der Rest der Varispeed bleibt in seinem Abschnitt: Modus, die drei Einheiten,
das Zieltempo-Feld. Das sind Dinge, die man *liest*, nicht festhält. Und der
Streifen erscheint nur im Halbton-Modus — ein Zieltempo wird getippt, nicht
gewischt, und ein Regler, der unter der Waveform auftaucht und wieder
verschwindet, wäre schlimmer als einer, der an seinem Platz bleibt.

Der Test dazu prüft **Sichtbarkeit ohne Scrollen**, nicht rohe Koordinaten: ein
aus dem Sichtfeld gescrollter Knoten meldet Null, und ein Vergleich gegen Null
wäre aus dem falschen Grund durchgegangen — was er beim ersten Versuch auch tat.

## 28. Alles zugeklappt, und ein Regler für das Zieltempo

- **Zugeklappt ist jetzt die Regel**, nicht die Ausnahme. `Section` hat
  `initiallyOpen = false` als Vorgabe, die Plan-Karte startet gefaltet. Der
  Bildschirm öffnet auf dem, was man ansieht — Waveform und Varispeed — und
  alles andere ist einen Tipp entfernt statt einen Scroll.
- **Die ganze Varispeed steht unter der Waveform.** Modus-Chips, Regler und das
  BPM-Feld. Der eigene Abschnitt ist damit überflüssig geworden und wurde
  entfernt, samt `SpeedControls` — zwei Orte für eine Sache sind ein Ort zu
  viel.
- **Das Zieltempo hat einen Regler**, mit dem Bereich `Quelle/2 .. Quelle×2`.
  Nicht aus absoluten Zahlen gegriffen, sondern aus der Quelle abgeleitet: das
  ist genau die musikalische Spanne des Halbton-Reglers (±12 Halbtöne sind
  Faktor 0,5 bis 2), also decken beide Bedienelemente dasselbe Feld ab, und der
  Regler kann kein Verhältnis erreichen, das die Vorschau anschließend
  beschneiden müsste. Rastung beim Quelltempo, aus demselben Grund, aus dem der
  Halbton-Regler eine bei null hat. Das Textfeld bleibt daneben: ein Regler
  trifft keine glatte 90, und eine glatte 90 ist meistens der Punkt.

**Der Fehler, der beim Umbau auffiel.** „Wenn man im Calculator die BPM schickt,
soll er sich einfach anpassen" — tat er nicht. Das Textfeld hielt seinen Text in
einem `remember`, das auf den *Modus* verschlüsselt war. Ein Tempo, das ankam,
während „target BPM" schon gewählt war, ließ die alte Zahl stehen — und das
sieht genau so aus, als wäre nichts passiert. Jetzt übernimmt ein
`LaunchedEffect(s.targetBpm)` jeden Wert, der von außen kommt, ohne beim Tippen
dazwischenzufunken. Gegen die alte Fassung laufen gelassen: `AssertionError:
Failed: assertExists.`

**Und eine Lektion, die ich schon einmal gelernt hatte.** Der Halbton-Wert im
Streifen kam zuerst aus dem Plan statt aus der Einstellung — also lag zwischen
dem Ziehen und der eigenen Anzeige eine Debounce. Dieselbe Regel wie bei den
Markern: *ein lebendes Bedienelement folgt dem Finger, nicht der Pipeline.* Im
Zieltempo-Modus ist es umgekehrt richtig, weil die Halbtöne dort abgeleitet sind
und es gar keine Einstellung gibt, die man anzeigen könnte.

## 29. Der Kopf bleibt stehen

Gemeldet als Einfall: „die Waveform und der Pitch/BPM-Regler sollten fix im Bild
bleiben, ab dort ein scrollbares Menü — damit man immer sieht, was sich gerade
ändert."

Genau richtig, und es war der eigentliche Rest des Problems. Der ganze Bildschirm
scrollte als ein Stück, also schob jedes Bedienelement, das man erreichen wollte,
das Bild weg, auf das es wirkt: man konnte die Sache ändern **oder** ansehen, nie
beides. Das Falten der Gruppen hat geholfen und es nicht behoben — eine lange
genug Liste scrollt den Kopf trotzdem hinaus.

Jetzt zwei Teile: oben festgesetzt Kopfzeile, Waveform und Varispeed; darunter
genau ein Scroll-Container mit allem, was den Schnitt nur *beschreibt*.

**Und der verschwundene BPM-Regler.** Er hing an `plan?.tempo`, und der Plan ist
null, bevor der erste ankommt und wieder, sobald die Einstellungen keinen Schnitt
beschreiben — also war das Bedienelement in genau diesen Momenten weg, ohne ein
Wort dazu. Das Tempo der Datei stand die ganze Zeit daneben und ist ein völlig
brauchbarer Rückfall. Gibt es wirklich keins (14 der 279 Archivdateien), steht
jetzt dort, warum: „half as fast" braucht etwas, wovon es die Hälfte sein kann.

**Zwei Lektionen aus den Tests dieser Runde**, beide unangenehm:

- Mein erster Sticky-Test war **wertlos**. Er verglich zwei getrennte
  Scroll-Container, was ohnehin nie fehlschlagen konnte. Der ehrliche
  Gegenversuch ist die *eine* scrollende Spalte von vorher — und dagegen lief er
  trotzdem durch, weil bei zugeklappten Gruppen der Inhalt aufs Display passt und
  es schlicht nichts zu scrollen gibt. Erst nachdem der Test alle Gruppen selbst
  aufklappt, fängt er die Regression: `'speed' is not displayed`.
- Ein Test, der gegen den Fehler nicht scheitert, ist kein Test, sondern eine
  Behauptung mit Zeremonie. Zweimal in einer Runde beinahe eine abgeliefert.

## 30. Zoom, und wem eine Geste gehört

- **Ziehen nur noch am Griff.** Vorher nahm ein Zug irgendwo auf der Waveform
  das jeweils nähere Ende — und das ist genau, womit ein Zwei-Finger-Zoom
  beginnt: ein Finger, der irgendwo aufsetzt. Ohne diese Regel hätte jede Kneif-
  Geste den Schnitt quer über die Datei geschleudert, bevor der zweite Finger
  überhaupt gelandet wäre. Der Körper der Waveform gehört jetzt dem Zoom.
- **Zoom ohne Nachladen.** Statt beim Zoomen in Rust neu zu messen, wird einmal
  beim Öffnen feiner gemessen — 4096 Buckets statt 512 — und der Zoom gibt sie
  aus. Bei 8× hält das Fenster ein Achtel davon, also dieselbe Dichte, mit der
  die ganze Datei ungezoomt gezeichnet wird. Weiter hinein zu gehen zeigte nicht
  mehr, sondern dasselbe breiter: eine vergrößerte Behauptung statt eines
  näheren Blicks. Deshalb ist `MAX_ZOOM` an die Messung gebunden, nicht an
  Geschmack. Kostet 128 KB und ist sofort.
- **Gezoomt wird um den Schwerpunkt der Finger**, damit das Audio unter ihnen
  liegen bleibt. Am linken Rand verankert fühlt sich eine Kneif-Geste an, als
  würde sie sich wehren.

**Zwei Fehler auf dem Weg, beide lehrreich.**

*Die Reihenfolge der Gesten.* Zeigerereignisse erreichen den **innersten**
Handler zuerst, und `detectTransformGestures` verbraucht alles jenseits des
Touch-Slop. Mit dem Zoom als innerem Handler fraß er jeden Marker-Zug: „only 0
move(s) arrived". Der Marker-Handler steht jetzt zuletzt in der Kette.

*Der Treffertest lief auf der falschen Stelle.* `detectDragGestures` meldet in
`onDragStart` die Position **nach** dem Überschreiten des Touch-Slop — Dutzende
Pixel entfernt vom Aufsetzpunkt. Ein Griff wurde also gegen einen Punkt geprüft,
den der Finger nie berührt hat, und prompt kam beides falsch heraus: der Griff
griff nicht, und die Mitte griff. Ausgeschrieben mit `awaitEachGesture` und
`awaitFirstDown` liegt der Treffertest dort, wo der Finger wirklich aufsetzte.

## 31. Bildschirmpflege

- **Die Regler sind dünn.** Materials Standard ist ein fetter runder Balken —
  er liest sich als Fortschrittsanzeige, als etwas, das einem berichtet wird,
  statt als etwas, das man hält. Ein Ort für alle vier Regler (Tonhöhe, Tempo,
  Wow, Flutter), damit sie nicht zu vier Arten von Bedienelement auseinander-
  driften.
- **Die Falt-Dreiecke sind größer** und kommen aus einer einzigen Funktion. In
  Fließtextgröße waren sie Satzzeichen, keine Bedienelemente.
- **Jede Faltbox hat eine Umrandung**, offen etwas heller als zu. Eine Spalte
  kastenloser Zeilen gibt dem Auge nichts, um eine Gruppe von der nächsten zu
  trennen — die Überschriften lasen sich als Etiketten statt als Deckel.
- **Ein Zahnrad-Reiter** neben CUTTER und CALCULATOR. Absichtlich fast leer:
  Erscheinungsbild, Bedienung und Haptik sind dort *benannt*, nicht gezeigt.
  Ein Schalter, der nichts tut, ist eine Lüge mit gutem Finish. Was heute darin
  steht, ist das, was schon einmal echte Zeit gekostet hat: welcher Build läuft.

## 32. Das Feld neben der Zahl, die es erzeugt

Das Ziel-Tempo war ein bildschirmbreites Material-Feld: schwebendes Etikett,
56 dp Mindesthöhe, ein Daumenbreit Luft um vier Zeichen — und es stand *unter*
dem Regler, eine halbe Seite von der Anzeige entfernt, die es erzeugt. Die Zahl,
die man tippt, und die Zahl, die dabei herauskommt, sind eine einzige Tatsache.
Also teilen sie sich jetzt eine Zeile: das Feld liegt im rechten Viertel des
Varispeed-Streifens, auf Höhe von `speed`.

Handgebaut statt `OutlinedTextField`, dessen Mindesthöhe genau das Problem war.
Die Einheit steht im Kasten, nicht darüber, und das Quell-Tempo erscheint grau
als Platzhalter — ein leeres Feld sagt so immer noch, was Leerlassen bedeutet.

**Der Test misst gegen den Streifen, nicht gegen den Bildschirm.** Ein Feld,
dessen Breite aus dem Rest besteht, den die Anzeige übrig lässt, ändert seine
Größe bei jedem Zug am Regler.

Der erste Versuch maß das falsche Kästchen: `BasicTextField` reicht seinen
`modifier` an den Editor *innerhalb* der Dekoration weiter, also kam die
Textbreite minus Innenabstand zurück (80.76 dp statt 96.86 dp) — die Zelle war
längst richtig, die Messung nicht.

## 33. Der Loop, der sich selbst umstellt

Gewünscht als „ein LFO, der den Playhead moduliert, aber immer an die richtige
Position, dass es ein Loop bleibt".

Der Loop wird in gleiche Stücke geteilt — Takt, Schlag, halber Schlag — und auf
jeder Stückgrenze wird der Lesekopf um eine *ganze* Anzahl Stücke versetzt.
Schläge werden zu Schlägen umgestellt, nie zu Bruchteilen davon.

Zwei Eigenschaften tragen die Sache, und beide sind tragend:

- **Das Raster ist der Loop, geteilt.** Keine Dauer in Sekunden, keine Frequenz
  in Hertz. Ein Stück ist exakt `frames / steps`, das Raster schließt an der
  Naht ohne Rest.
- **Die Versetzung ist eine reine Funktion des Stück-Index**, der an der Naht
  auf null zurückspringt. Damit ist der zweite Durchlauf sample-identisch zum
  ersten. Ein LFO mit eigener Periode würde gegen den Loop schweben und etwas
  erzeugen, das sich nie wiederholt — ein hübscher Effekt, aber nicht dieser.

Vier Formen: `rise`, `fall`, `swing`, `scatter`. Auch das Streuen ist ein *Hash*
des Index, kein Zufallsgenerator — gleicher Index, gleiche Antwort, für immer.
Ein Generator würde beim zweiten Durchlauf etwas anderes ausspucken, und genau
das ist der Unterschied zwischen „umgestellt" und „kaputt".

**Übergeblendet wird trotzdem.** Ein Sprung ist auch auf dem Raster ein Bruch in
der Wellenform: das Sample davor und das danach haben nichts miteinander zu tun.
4 ms *gleicher Leistung* (nicht linear) — zwischen unkorrelierten Signalen
sackt eine lineare Blende in der Mitte ab, und das Loch säße genau dort, wo das
Ohr auf den Transienten wartet.

### Was der erste Test *nicht* geprüft hat

Der Wiederholungs-Test bestand auch gegen ein Raster von festen 700 Frames —
weil der Stück-Index von der Position abgelesen wird, die mit dem Loop umläuft.
Das Muster startet also bei jedem Durchlauf neu, ganz gleich wie die Stücke
geschnitten sind. Wiederholung und Rasterschluss sind **zwei** Behauptungen;
die zweite braucht einen eigenen Test (`the_grid_closes_at_the_seam`), der
nachrechnet, dass die Stücke den Loop restlos kacheln.

Gegen einen freilaufenden Zähler fällt der Wiederholungs-Test dagegen sofort —
das ist genau der falsche Bau, den er verhindern soll.

### Wo es bewusst nicht hinkommt

**Nichts davon erreicht die Datei.** Die Bewegung ist eine Art, den fertigen
Schnitt zu *hören*, keine Stufe seiner Herstellung; Export schreibt dieselben
Bytes, ob sie an ist oder aus. Deshalb steht sie auch nicht in `Settings`: ein
Feld dort würde als Parameter mitgeschickt, würde bei jedem Dreh die Vorschau
verwerfen und wäre still Teil des Schnitts geworden. Auf dem Bedienfeld steht
es als Satz, statt es entdecken zu lassen.

Der Playhead folgt jetzt der **klingenden** Position, nicht der Uhr. Beide
werden getrennt veröffentlicht: zurückgesprungen wird an die Uhr, denn nach
einem Neuaufbau dort weiterzumachen, wo der Loop gerade hingesprungen war, ist
nicht die Stelle, an der der Hörer war.

### Zweimal die eigene Arbeit gelöscht

Ein `cp` aus einem älteren Backup und ein `git checkout` haben je einen frisch
geschriebenen Test wieder entfernt — beide Male, um einen Gegenbeweis
zurückzunehmen. Die Regel daraus: **vor dem Gegenbeweis committen**, dann ist
Zurücksetzen billig und trifft nur das, was es treffen soll.

### 26b. Und einmal das Falsche ausgeliefert

Gemeldet als „wo ist der LFO? ist es überhaupt die richtige Version?" — und der
Stempel gab die Antwort sofort: `0.1.0+3f17f5f`, ein Commit vor dem LFO.

Diesmal lag es nicht am Build, sondern an der **Reihenfolge**: erst
`assembleRelease`, dann geprüft, dann `connectedAndroidTest -PtestRelease` —
was dieselbe `app-release.apk` überschreibt, weil `testBuildType` dort auf
`release` steht und zusätzliche R8-Regeln alles behalten. Erst danach wurde
kopiert. Ausgeliefert wurde also der „unter Test"-Build: 8 MB statt 2,4, mit
17,9 MB dex.

Zwei Regeln daraus:

- **Prüfen, was tatsächlich rausgeht** — nicht den Pfad, aus dem es einmal kam.
  Aufgefallen ist es nur, weil die Dateigröße nicht passte.
- **`versionCode` zählt jetzt die Commits.** Er stand für immer auf 1, also sah
  jeder Build für Android wie die schon installierte Version aus. Ein Installer
  darf das als „nichts zu tun" behandeln, und dann ist „installiert, nichts
  geändert" nicht mehr von einem alten APK zu unterscheiden. Er steht neben dem
  Namen im Zahnrad-Reiter.

## 34. Zwei Einheiten auf einer Zeile

Der Varispeed war eine Knopfreihe *und* eine Anzeigereihe — zwei Zeilen eines
Bildschirms, dessen ganzes Layout dafür da ist, die Waveform im Blick zu halten,
und die getippte Zahl lag einen Regler von der Zahl entfernt, die sie erzeugt.

Jetzt trägt jeder Modusknopf seinen eigenen Wert direkt hinter sich, beide auf
einer Zeile: links `semitones` mit den Halbtönen, rechts `target BPM` mit dem
Tempo. **Beide Anzeigen stehen immer da** — die gedimmte ist abgeleitet, die
helle ist die, die man gerade fährt. Die andere Einheit beim Ziehen mitlaufen
zu sehen ist der halbe Grund, überhaupt zwei Einheiten zu haben.

Die alte Regel „das Feld ist ein Viertel des Streifens" fällt mit dem Layout,
das sie beschrieb: dieses Feld hatte eine eigene Zeile, das neue teilt sie sich
mit zwei Knöpfen und der anderen Einheit.

## 35. Die Rate ist eine Einheit, keine Einstellung

Raster und Rate waren ein Regler. Damit hieß „Landungen auf halben Schlägen"
zwangsläufig auch „Stottern im Halbschlag-Takt" — die feine Auflösung war bei
keinem musikalischen Tempo brauchbar.

Getrennt: das **Raster** sagt, *wo* ein Sprung landen darf, die **Rate** sagt,
*wie oft* einer passiert. Beide werden pro Takt gezählt, und genau das ist das
ganze „beatsync" — keine Einstellung, sondern eine Einheit. Es gibt in diesem
Modell keine Zahl, die zwischen zwei Schläge fallen könnte. Die Rate ist nie
feiner als das Raster; ein gröberes Raster zieht sie mit.

Dazu eine fünfte Form: **`walk`**, ein Random Walk statt eines unabhängigen
`scatter`. Benachbarte Züge bleiben nahe beieinander, eine Phrase überlebt das
Verschieben. Er wird bei jedem Zug von vorn durchgelaufen statt in einem Feld
mitgeführt — ein Feld würde ihn von der Vorgeschichte abhängig machen, und
Vorgeschichte ist genau das, was den Loop kaputtmacht. Kosten: `index`
Schritte, höchstens einmal pro Zug, auf einem Index, der jede Runde neu bei
null anfängt.

## 36. Eine Spur statt zwei

Neben Play sitzt `2ch` / `1ch`. Es ist eine **Ansicht**, keine Einstellung —
weder Datei noch Schnitt ändern sich, nur 84 dp statt 150.

Gefaltet zeigt die Spur die **Hüllkurve** beider Kanäle, nicht ihre Summe: eine
Summe löscht sich überall dort aus, wo die Kanäle sich widersprechen, und würde
eine leise Stelle über Material zeichnen, das bloß breit ist.

## 37. Zwei Loops auf einem Lesekopf

Gewünscht: ein zweiter Drumloop, zwischen dessen Chops ein LFO hin- und
herschaltet — „aber immer an einer richtig gesyncten Position".

Gebaut als **geteilte Phase, nicht als Synchronisation**, und darin steckt der
ganze Entwurf. Es gibt nur einen Lesekopf; der Wechsel ändert allein, *aus
welchem Puffer* gelesen wird. Takt drei des einen wird von Takt vier des anderen
gefolgt, im Takt, ohne dass irgendetwas neu getriggert würde.

Zwei Uhren, die man im Gleichschritt hält, sind etwas, das auseinanderlaufen
kann — und ein Versatz von ein paar Samples pro Durchlauf ist genau das
Artefakt, dessen Beseitigung der Sinn dieses Werkzeugs ist. **Eine Uhr kann
nicht von sich selbst abweichen.**

### Warum die Länge nicht verhandelbar ist

Ein Partner anderer Länge hat keine geteilte Phase, an der er gelesen werden
könnte. Also wird er **abgelehnt statt gedehnt** — Dehnen an dieser Stelle würde
still die Exaktheit aufheben, auf der alles andere steht.

Möglich ist die Anpassung überhaupt nur, weil beide Tempi exakt bekannt sind:
der zweite Loop wird auf die Taktzahl des ersten geschnitten und mit derselben
exakten Rationalarithmetik auf dessen Tempo gezogen wie jeder andere Schnitt.
Wo das nicht geht — kein deklariertes Tempo, andere Samplerate —, erscheint die
Ablehnung in CUTTER 2, **mit beiden Frame-Zahlen darin**. „Anderer Länge" allein
lässt den Benutzer ohne etwas, das er ändern könnte.

### CUTTER 2 ist absichtlich kein zweiter Cutter

Kein Varispeed, kein Export — er hat keine eigene Länge. Ihm ein Tempo zu geben
hieße anzubieten, genau die Eigenschaft zu brechen, die das Paar trägt. Was er
hat, sind die beiden Entscheidungen, die wirklich seine sind: welcher Teil der
Datei, und wie er gelesen wird.

### Eine Blende für zwei Ereignisse

Fallen ein Sprung (Motion) und ein Wechsel (Swap) auf dieselbe Rasterkante, gibt
es dort **eine** Unstetigkeit. Zwei überlappende Blenden würden jede nur die
Hälfte davon verdecken, deshalb merkt sich `begin_fade` einmal, was verlassen
wird — Seite *und* Versatz.

### Ein ungerader Zyklus wird gesagt, nicht verhindert

Teilt `holdA + holdB` den Loop nicht, kommt die letzte Wendung vor der Naht kurz
heraus. Der Loop wiederholt sich trotzdem exakt, weil die Zählung mit ihm neu
beginnt — es ist eine musikalische Wahl, kein Fehler. Also steht es da.

## 38. Ein Tempo, zwei Bezugspunkte

Gewünscht: das Master-Tempo auch auf CUTTER 2, und wählen können, von welchem
Loop die Geschwindigkeit genommen wird.

**Es gibt nur eine Geschwindigkeit, weil es nur einen Lesekopf gibt.** Was der
`MSTR`-Schalter wählt, ist, *woran* sie gemessen wird: eingeschaltet läuft das
Paar auf dem Eigentempo dieses Loops, der andere wird also dorthin gezogen.

**Ein Schalter neben dem Regler, keine zwei Buttons.** Zuerst waren es zwei
Chips, die beide Loops benannten, auf einer eigenen Zeile. Das kostete eine
Zeile für eine Auskunft, die ein einzelner Schalter gibt, und ließ die Wahl
aussehen, als gehöre sie dem Paar statt der Seite, auf der man steht. Jetzt
trägt jeder Cutter ein `[MSTR]` an seinem eigenen Geschwindigkeitsregler.

**Höchstens einer kann an sein.** Hier an heißt dort aus — zwei Bezugspunkte
sind kein Zustand, der etwas bedeutet, also ist es kein Zustand, der erreichbar
ist. Ohne zweiten Loop fehlt der Schalter ganz, statt tot dazustehen: mit einem
Loop gibt es nichts, wovon man Master wäre.

**Jetzt ein Modus — und einer, der von selbst loslässt.** Als reine Aktion
konnte er ein Tempo, das sich später ändert, nicht mitnehmen. Als Modus, der
festhält, läge er im Streit mit dem nächsten Zug am Regler. Beides zusammen geht
nur mit einer Regel: *eine Hand am Tempo schlägt den Schalter.* Sobald die
Geschwindigkeit nicht mehr das Tempo dieses Loops ist — Regler gezogen, Tempo
getippt, Einheit gewechselt — geht `MSTR` aus, statt die Zahl zurückzuschreiben.

Beide Hälften stellen dieselbe Frage, und die steht deshalb als eine Funktion da
(`holdsTempo`): das Anwenden fragt sie, um nicht neu zu setzen, was schon da ist
— sonst löst ein fertiger Plan ein Update aus, das einen Plan auslöst —, das
Loslassen fragt sie, um die Hand zu bemerken. Verglichen wird auf Milli-BPM, die
Auflösung, auf die der Tempo-Regler einrastet; enger verglichen ginge der
Bezugspunkt bei einem Rundungsschritt verloren, den niemand gemacht hat.

Der Varispeed-Streifen steht jetzt auf beiden Cuttern. Dieselbe Einstellung,
zwei Orte — weil man auf CUTTER 2 steht, wenn man hören will, wie der zweite
Loop sitzt, und jemanden für einen Regler auf einen anderen Reiter zu schicken
ist, wie ein Bedienelement aufhört, benutzt zu werden.

## 39. Der Mixer ist ein Pult

Gewünscht: ein richtiges Mischpult-Layout, mit Kanälen und dem Master.

**Ein Pult liest man quer, nicht untereinander.** Drei Züge nebeneinander lassen
einen Blick zwei Pegel vergleichen; drei Zeilen mit liegenden Reglern machen
daraus Scrollen und eine Gedächtnisübung. Die Fader stehen senkrecht aus
demselben Grund, aus dem sie es auf jedem Pult tun — das Auge vergleicht Höhen.

Die Fader sind von Hand gezeichnet statt gedrehte `Slider`. Ein gekippter
`Slider` misst und meldet sich weiter in seiner alten Ausrichtung; er wäre ein
Bedienelement, das dem Layout und den Tests etwas anderes erzählt, als es zeigt
— und dieser hier muss exakt so hoch sein wie die Anzeige daneben, damit man das
eine gegen das andere lesen kann.

**Oben ist +6 dB, Unity sitzt knapp darunter** und ist markiert. Ein Fader, der
nur absenken kann, lässt „dieser Loop ist zu leise" ohne Bedienelement auf der
Seite. Auf Unity rastet er ein, aus demselben Grund wie der Varispeed auf Null:
genau 0 dB muss mit dem Finger erreichbar sein.

### Der Master bewegt, was rausgeht — nicht, was die Kanäle beitragen

Die Kanalanzeigen sind **post-Fader und pre-Master**, die Master-Anzeige ist
post-Master. Das ist die Pult-Anordnung und die nützliche: die Kanäle sagen
weiter, welcher Loop laut ist, während der Master sagt, ob das, was rausgeht, zu
laut ist. Zöge man den Master und alle drei Anzeigen gingen mit, stünde dreimal
dieselbe Auskunft da und die Balance wäre weg.

Der Master-Gain bleibt bis ganz unten von den beiden Kanal-Gains getrennt. Ein
Master-Zug, der beide Kanalfader umschreibt, ist ein Pult, das über die eigenen
Pegel lügt. Er hat deshalb ein eigenes Wort in der Handle: drei `f32` sind
sechsundneunzig Bit und passen nicht in eins — verloren geht dabei nichts, denn
ein Block, der einen neuen Master neben einer alten Balance sieht, ist ein Pult
mit zwei Händen darauf und kein Fehler.

## 39a. Der Mixer misst pro Loop, nicht pro Ausgang

Mit laufendem Swap klingt immer nur *einer* der beiden. Eine Anzeige, die die
Summe zeigt, könnte die Frage, für die diese Seite existiert, nicht beantworten
— „welcher der beiden ist zu laut". Also zeigt jeder Balken den eigenen Beitrag
seines Loops, und der stumme steht auf null. Während der Swap-Blende bewegen
sich beide, was genau das ist, was gerade passiert.

**Zwei Skalen, absichtlich gegenläufig:**

- Die **Anzeige ist in Dezibel**. Linear säße alles, was einzustellen sich lohnt,
  im linken Zehntel des Balkens, und die Anzeige wäre Dekoration.
- Der **Fader ist linear in der Lautheit** (Kubikgesetz). Auf halber Strecke soll
  es etwa halb so laut klingen, nicht 6 dB leiser. Andersherum hätte man ein
  Bedienelement, das über den größten Teil seines Wegs fast nichts tut.

Beides hängt an einem Test, weil beides leicht falsch herum ist und keines davon
auf einem Screenshot zu sehen wäre.

**Anstieg sofort, Abfall langsam.** Eine Anzeige, die so schnell fällt wie das
Audio, flackert bei jedem Schlag und liest sich als Rauschen.

Die beiden Gains reisen in **einem** Wort, aus demselben Grund wie die Motion:
einzeln gesetzt könnte ein Block mit dem neuen ersten und dem alten zweiten Gain
gelesen werden — eine Balance, die niemand wollte, genau dort, wo ein Swap sie
hörbar macht.

Ein negativer Gain ist ausgeschlossen: das wäre eine Polaritätsumkehr im Gewand
eines Lautstärkereglers, und beim Swap würde sie als „ein Loop klingt hohl"
gehört.

## 40. Zwei Ansichten, und wer sie wählt

Gefragt: ob man die senkrechte und die liegende Ansicht in den Settings
einstellen kann. Ja — und beide Fragen dahinter sind dieselbe Frage.

### Pult oder Zeilen

Ein Pult braucht Breite, und ein hochkant gehaltenes Telefon hat wenig davon:
drei Züge auf einem schmalen Schirm sind drei schmale Züge. Zeilen geben jedem
Kanal die volle Breite und dem Dateinamen Platz, zum Preis genau des Vergleichs,
für den das Pult da ist. Was davon wichtiger ist, kann der Code nicht wissen —
also ist es eine Einstellung und keine Vermutung.

**Beide Ansichten teilen sich jede Zahl.** Dieselbe Anzeigenskala, derselbe
Faderweg, dieselbe Rastung auf Unity — nur die Richtung ändert sich. Ein
Bedienelement, das sich je nach Zeichenrichtung anders verhält, sind zwei
Bedienelemente mit einem Namen.

### Quer: derselbe Schnitt, um neunzig Grad gedreht

Das Bild nimmt die linke Hälfte, die Panels stehen daneben statt darunter.
Entschieden wird das **an den eigenen Constraints des Layouts**
(`maxWidth > maxHeight`), nicht an der Gerätemeldung: ein hochkant gehaltenes
Tablet mit Platz will dieselbe Anordnung, und „Landscape" hätte das falsch
beantwortet.

Quer ist die obere Hälfte **scrollbar**. Hochkant ist sie festgepinnt, das ist
der ganze Sinn des Schnitts; bei halber Höhe hieße „festgepinnt" aber
„abgeschnitten", und ein Regler, den man nicht erreicht, ist schlimmer als
einer, den man ins Bild schieben muss.

Zwei Dinge, die dabei kaputtgingen und geflickt wurden — beide erst auf einem
Screenshot sichtbar, nicht in einem Test:

- Der Titel brach um („LOOP_ / SLCR") und schob den Varispeed aus seiner Spalte.
  Jetzt eine Zeile, Name mit Auslassungspunkten.
- Der Varispeed-Streifen brach bei halber Breite so weit ein, dass das
  Chip-Label senkrecht stand, ein Buchstabe pro Zeile. Statt ein Bedienelement
  bis zur Unlesbarkeit zu schrumpfen, bricht die Zeile jetzt an ihrer einzigen
  Naht in zwei.

### Gemerkt, nicht geraten

`SharedPreferences`, zwei Enums, beim Start gelesen und beim Tippen geschrieben.
Eine Einstellung, die sich bei jedem Start vergisst, wäre schlimmer als keine:
man träfe die Wahl jedes Mal neu, was das Gegenteil dessen ist, wofür eine
Einstellung da ist. Ein unbekannter Wert in der Datei fällt auf den Standard
zurück, statt beim Start zu werfen.

Die Ausrichtung wird **vor dem ersten Frame** gesetzt und beim Tippen sofort —
eine App, die aufrecht startet und dann herumschwenkt, wäre schlechter als eine,
die die Wahl nie angeboten hätte.

Und: `Display` ist bewusst von `Settings` getrennt. Die einen beschreiben einen
Schnitt und gehören zu einer Datei, die anderen beschreiben einen Bildschirm und
gehören zu einem Menschen.

## 41. Sechs Frames, und warum ein Tempo sie nicht findet

Vom Telefon gemeldet: „CUTTER 2 läuft nicht mit dem Swap", dazu rot
`the second loop is a different length — 769745 frames against 769739`. Sechs
Frames, bei einem Paar, dessen Tempi beide exakt bekannt waren.

**Der Weg über das Tempo rundet zweimal.** Einmal auf ein Taktraster, einmal auf
einen Frame. Der erste Loop war über einen anderen Weg zu seiner Länge gekommen
als der zweite — also waren sich die beiden über das *Tempo* einig und über den
*Sample* nicht. Ein gemeinsamer Lesekopf hat für diese Uneinigkeit keinen Platz;
er ist der Grund, warum das Paar überhaupt ohne Synchronisation funktioniert.

Die Anfrage ist deshalb jetzt **die Länge selbst**: `targetFrames` in den
Parametern, `Ratio::to_fit(frames, wanted)` im Kern. Kein Zwischenschritt, keine
Rundung — die Länge *ist* die Bitte, und das Verhältnis ist, was sie wahr macht.
Gesetzt schlägt sie Halbtöne und Ziel-BPM; das Tempo fährt weiter mit, weil es
das ist, was der Plan meldet.

Genommen wird die Zahl **vom laufenden Preview**, wenn es eins gibt: dieser
Puffer *ist* das, wozu der Partner dazukommt, und ihn zu fragen schlägt jedes
Nachrechnen. Ohne Wiedergabe kommt sie aus dem Plan, damit der Trockenlauf auf
CUTTER 2 dieselben Zahlen zeigt wie später das Audio.

Dazu ein zweiter Fehler in derselben Anzeige: das Fit-Feld verglich gegen die
*gedehnte* Länge des ersten Loops und meldete „13745 frames out", während das
Paar sechs Frames vom Passen entfernt war. Der Preview spielt den ersten Loop
ungedehnt, der Varispeed bewegt danach beide zusammen — verglichen wird jetzt
gegen die Eigenlänge.

Der Gegenbeweis steht als Test: ohne `targetFrames` fällt er mit genau der
Fehlermeldung, die auf dem Telefon stand.


## 42. Ein Insert, und warum die Reihenfolge ein Regler ist

Gewünscht: „evtl sogar noch einen Filter und Overdrive, wo man den Filter vor
oder nach dem Overdrive routen kann". Genau das ist gebaut — und die Reihenfolge
ist kein Detail, sondern der interessanteste Regler von beiden.

**Ein Tiefpass *vor* dem Overdrive** nimmt die Höhen weg, bevor irgendetwas sie
verzerren kann: der Overdrive hört nur, was durchgekommen ist, und das Ergebnis
bleibt so dunkel wie der Filter. **Ein Tiefpass *hinter* dem Overdrive** nimmt
die Höhen weg, die der Overdrive selbst gemacht hat: verzerrt wird alles, und der
Filter entscheidet danach, wie viel davon bei dir ankommt — das ist der Sweep,
der etwas zum Durchfahren hat. Beides macht man mit Absicht, also ist keins fest
verdrahtet.

**Warum das nicht `ops::filter` ist.** Das Modul filtert einen *Loop* und muss
sich Mühe geben: ein Filter, der bei Stille anfängt, setzt einen Einschwinger auf
Frame 0 — genau die Naht, für die es dieses Programm gibt. Deshalb läuft das
Material dort zweimal durch und nur der zweite Durchgang wird behalten. Dieser
Filter hier filtert einen *Lesekopf*, und der hält an der Naht nicht an. Er läuft
vom Start der Wiedergabe bis zum Stopp, über jede Loop-Grenze, jeden Sprung und
jeden Swap, und trägt seinen Zustand die ganze Zeit mit — wie das Kabel, das er
vertritt. Es gibt nichts aufzuwärmen, weil nichts neu anfängt.

**Der Overdrive hält oben still und holt alles darunter hoch.** `tanh`, hinein
gedrückt und normiert auf das, womit ein Vollausschlag herauskäme. Das ist, was
Aufdrehen an einem Overdrive tut: es fügt keinen Pegel hinzu, es nimmt den
Abstand zwischen leise und laut weg. Nebenbei fällt eine Garantie ab, die in
einer Kiste vor dem Master-Fader etwas wert ist: aus einem Signal, das keine
Übersteuerung hatte, kann er in keiner Stellung eine machen.

**Aus ist aus, bis aufs Bit.** Nicht ein Filter auf harmloser Einstellung — der
Preview fragt `is_wire()` und nimmt den alten Pfad. Eine Sitzung, die das
FX-Feld nie öffnet, bekommt die Samples, die sie bekommen hätte, bevor es das
Feld gab. „Aus" auf zwei Nachkommastellen wäre eine andere Behauptung.

**Warum es auf der MIX-Seite steht.** Ein Insert ist ein Platz auf dem Pult,
kein eigenes Zimmer. Er sitzt zwischen den Kanalfadern und dem Master, und das
Master-Meter darüber ist das, was sagt, was das Aufdrehen angerichtet hat. Auf
einem eigenen Tab wäre es ein Regler, den man dreht, während man ein Bild von
einem anderen Signal ansieht.

**Sechs Regler kreuzen in zwei Wörtern.** Sie passen nicht in vierundsechzig Bit,
und der Mutex gehört für die Länge eines Blocks dem Audio-Thread. Anders als bei
der Motion ist ein zerrissener Lesevorgang hier harmlos: jedes Feld ist ein
stufenloser Regler mit für sich gültigen Werten, also ist „neuer Cutoff, alte
Resonanz" eine Stellung, an der die Hand vorbeigekommen ist — keine Einstellung,
die niemand gewählt hat. Der Cutoff behält alle zweiunddreißig Bit, weil er der
ist, den eine Hand sweept, und eine grobe Stufe darin hört man als Treppe.

**Nichts davon wird exportiert**, aus demselben Grund wie Varispeed und Motion:
der Schnitt ist, was die Datei *ist*, das Insert ist, was deine Hände damit
gemacht haben. Ein eingebackener Filtersweep ist kein Loop mehr.
