# LOOP_SLCR — Kontextfile (Session-Übergabe)

> **Zweck:** Diese Datei in einen neuen Chat ziehen → Claude Van Damme ist sofort auf Stand.
> **Status:** TIMING-CORE + WAV-READER STEHEN. Cargo-Workspace, `rational.rs`,
> `timing/`, `buffer.rs`, `wav/{chunks,read}.rs`, CLI `grid` + `info`.
> 60 Tests grün, Clippy sauber, null Runtime-Dependencies im Core.
> Der 103-BPM-Referenzfall reproduziert die Docs exakt
> (in 822058 = 0:18.641, out 1644116 = 0:37.282).
> **Reader gegen das echte Archiv verifiziert:** 277 WAVE-Dateien,
> 177 790 491 Frames, sample-für-sample identisch mit `hound`.
> Nächster Schritt: RIFF-Writer → `ops::cut` → `ops::foldback`.
> **Letztes Update:** Session 2 — 29.07.2026

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
- [ ] Batch-Modus: Preset auf ganzen Ordner anwenden
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
loopslcr batch ./drumloops --bpm-from-name --bars 8 -o ./cut/
```

Flags: `--sig 7/8` · `--bpm-unit 1/4` · `--align loop|grid` · `--fade 1ms`
· `--normalize` · `--dry-run` · `--bits 16|24|32f` · `--dither tpdf|none`
· `--pitch -2.34st` **oder** `--target-bpm 90` · `--tag acid,smpl,info`
· `--tape off|on` · `--wow 0.3` · `--flutter 0.15` · `--hf-rolloff auto`

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

### Noch offen ❓
1. **Micro-Fades:** default an (0.5 ms) oder default aus?
2. **Peak-Buckets:** in Rust berechnen und über JNI reichen, oder in Kotlin?
    (Rust = konsistent mit CLI-`info`, Kotlin = weniger JNI-Verkehr)
3. ~~**WAV-Read:** `hound` behalten oder auch selbst?~~ → **selbst, entschieden
    und gebaut.** Reader und Writer teilen die Chunk-Ebene, `hound` bleibt als
    *dev*-dependency für die Gegenprobe in beide Richtungen. Null Runtime-Deps.
    Hat sich sofort bezahlt: das zu kurze RIFF-Size-Feld der Caustic-Exports
    fiel nur auf, weil das Parsen in eigener Hand lag.
4. **Wow/Flutter-Defaults** und ob Charakter-Settings presetbar sind
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
  Archivs.
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
