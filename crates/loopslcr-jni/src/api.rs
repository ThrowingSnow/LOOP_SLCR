//! The call surface, in plain Rust.
//!
//! Nothing here knows about the JVM. That is the point: JNI code is the hardest
//! kind to test, so as little of it as possible should contain anything worth
//! testing. `lib.rs` converts types and catches panics; this decides things, and
//! it runs under `cargo test` like any other module.
//!
//! Every function takes bytes and a JSON string and returns bytes or a JSON
//! string, so the whole surface can be exercised without a `JNIEnv` — and the
//! JVM test then only has to prove that the bridge passes them through intact.

use loopslcr_core::analysis::Peaks;
use loopslcr_core::ops::tape::TapeParams;
use loopslcr_core::pipeline::{self, BarsSource, Params, TempoSource};
use loopslcr_core::rational::Rational;
use loopslcr_core::timing::{note, Align, BpmUnit, Grid, Ratio, Tempo, TimeSignature};
use loopslcr_core::wav::{write, BitDepth, Metadata, Wav, WriteSpec};
use loopslcr_core::{naming, Tail, Workflow};

use crate::json::{self, Object, Value};

/// What a caller needs to know before it can offer any choices.
///
/// Returned as JSON. Fields a caller does not recognise are ignored rather than
/// misread, which is the whole reason for a self-describing format here.
pub fn analyze(bytes: &[u8], name: &str) -> Result<String, String> {
    let wav = Wav::parse(bytes).map_err(|e| e.to_string())?;
    let format = wav.format();
    let buffer = wav.decode().map_err(|e| e.to_string())?;
    let tail = Tail::measure_default(&buffer);

    let mut out = Object::new();
    out.integer("channels", format.channels as i64)
        .integer("sampleRate", format.sample_rate as i64)
        .integer("bitsPerSample", format.bits_per_sample as i64)
        .integer("frames", buffer.frames() as i64)
        .number("durationSeconds", buffer.duration_seconds())
        .number("peak", buffer.peak())
        .integer("audibleEnd", tail.audible_end as i64)
        .integer("tailFrames", tail.trailing_frames as i64)
        .number("tailThresholdDbfs", tail.threshold_dbfs)
        .maybe_number(
            "declaredTempo",
            wav.tags().declared_tempo().map(f64::from),
        )
        .maybe_number(
            "nameTempo",
            naming::tempo_from_name(name).map(|t| t.value().to_f64()),
        )
        .maybe_integer("nameBars", naming::bars_from_name(name).map(|b| b as i64));

    // The loop length and shape need a grid, so they need a tempo. Reported when
    // one can be had and left null when not, rather than guessed at 120 — a UI
    // that shows a wrong default is worse than one that asks.
    let tempo = wav
        .tags()
        .declared_tempo()
        .and_then(Tempo::from_f32)
        .or_else(|| naming::tempo_from_name(name));
    match tempo {
        Some(tempo) => {
            let grid = loopslcr_core::timing::Grid::new(
                tempo.with_unit(BpmUnit::quarter()),
                TimeSignature::default(),
                format.sample_rate,
            );
            let guess = loopslcr_core::analysis::guess_loop_bars(&grid, buffer.frames());
            out.number("tempo", tempo.value().to_f64())
                .maybe_integer("loopBars", guess.map(|g| g.bars as i64))
                .string(
                    "workflow",
                    workflow_name(guess.map_or(Workflow::Unclear, |g| g.workflow)),
                );
        }
        None => {
            out.maybe_number("tempo", None)
                .maybe_integer("loopBars", None)
                .string("workflow", workflow_name(Workflow::Unclear));
        }
    }
    Ok(out.render())
}

/// Min/max buckets for a waveform, as `[c0min, c0max, c1min, c1max, …]`.
///
/// Interleaved per bucket and then per channel, so the drawing side walks it
/// once in the order it draws. Flat `f32` rather than a nested structure because
/// this is the one call that is on a hot path — a resize redraws it — and
/// because a Java `float[]` maps onto a `FloatBuffer` with no per-element work.
pub fn peaks(bytes: &[u8], buckets: usize) -> Result<Vec<f32>, String> {
    let wav = Wav::parse(bytes).map_err(|e| e.to_string())?;
    let buffer = wav.decode().map_err(|e| e.to_string())?;
    let peaks = Peaks::measure(&buffer, buckets);

    let channels = peaks.channel_count();
    let mut out = Vec::with_capacity(peaks.len() * channels * 2);
    for i in 0..peaks.len() {
        for channel in &peaks.channels {
            out.push(channel[i].min as f32);
            out.push(channel[i].max as f32);
        }
    }
    Ok(out)
}

/// Runs the whole pipeline and encodes the result.
///
/// Returns the finished WAVE file. `params` is the JSON described in
/// [`params_from_json`]; every field is optional and an absent one means the
/// same as it does on the command line.
pub fn process(bytes: &[u8], name: &str, params: &str) -> Result<Vec<u8>, String> {
    let wav = Wav::parse(bytes).map_err(|e| e.to_string())?;
    let (params, allow_short) = params_from_json(params)?;

    let outcome = pipeline::run(&wav, name, &params).map_err(|e| e.to_string())?;
    if outcome.short_by > 0 && !allow_short {
        return Err(format!(
            "{} frames short of a {}-bar loop — the result would drift; \
             set allowShort to accept it",
            outcome.short_by, outcome.bars
        ));
    }

    let comment = format!(
        "LOOP_SLCR: {}, {} bars, {}, {} frames",
        outcome.final_tempo,
        outcome.bars,
        outcome.sig(),
        outcome.buffer.frames()
    );
    let spec = WriteSpec::new(params.depth).with_metadata(
        Metadata::for_loop(
            outcome.final_tempo.value().to_f64() as f32,
            u32::try_from(outcome.beats()).unwrap_or(u32::MAX),
            outcome.sig().num as u16,
            outcome.sig().den as u16,
            u32::try_from(outcome.buffer.frames()).unwrap_or(u32::MAX),
        )
        .with_comment(comment),
    );
    write::write(&outcome.buffer, &spec).map_err(|e| e.to_string())
}

/// The same run, reported rather than encoded.
///
/// The dry run, for a UI that wants to show what would happen before it happens.
/// Returns the same shape of JSON as [`analyze`] plus what the pipeline decided.
pub fn plan(bytes: &[u8], name: &str, params: &str) -> Result<String, String> {
    let wav = Wav::parse(bytes).map_err(|e| e.to_string())?;
    let (params, _) = params_from_json(params)?;
    // The cheap stage. A UI calls this while a finger is still moving, and the
    // full run resamples: 6.6 seconds against 0.065 on the reference file.
    let outcome = pipeline::run_staged(&wav, name, &params, pipeline::Stage::Plan)
        .map_err(|e| e.to_string())?;

    let mut out = Object::new();
    out.number("tempo", outcome.tempo.value().to_f64())
        .string("tempoSource", tempo_source_name(outcome.tempo_source))
        .integer("bars", outcome.bars as i64)
        .string("barsSource", bars_source_name(outcome.bars_source))
        .string("workflowDetected", workflow_name(outcome.detected))
        .string("workflowChosen", workflow_name(outcome.chosen))
        // The evidence the detection ran on, so a UI can show *why* rather than
        // only *what*. `audibleBars` is the one that decides it: divided by the
        // loop length it gives the ratio the thresholds in
        // `WorkflowGuess::detect` are written against — about two for a warmup
        // render, about one for a file needing a foldback.
        .number("audibleBars", outcome.shape.audible_bars)
        .number("barsInFile", outcome.shape.bars_in_file)
        .integer("skipBars", outcome.skip_bars as i64)
        .integer("regionStart", outcome.region.start as i64)
        .integer("regionEnd", outcome.region.end as i64)
        .integer("loopFrames", outcome.loop_frames as i64)
        .integer("shortBy", outcome.short_by as i64)
        .integer("fadeFrames", outcome.fade.frames as i64)
        .number("ratio", outcome.ratio.to_f64())
        .bool("ratioExact", outcome.ratio.is_exact())
        .number("semitones", outcome.ratio.semitones())
        .number("resultingTempo", outcome.final_tempo.value().to_f64())
        .integer("outputFrames", outcome.output_frames as i64)
        .number("peak", outcome.peak.value)
        .bool("clips", outcome.peak.clips())
        // In the planning stage the character is not applied, so what can be
        // reported is whether it *would* be — a decision about parameters.
        .bool("tape", params.tape.enabled)
        // The peak was measured before the varispeed, so a resampler's
        // overshoot is not in it. Named rather than implied.
        .bool("peakBeforeVarispeed", outcome.stage == pipeline::Stage::Plan)
        .maybe_number("normalizeGain", outcome.normalized.map(|(g, _)| g))
        .bool("dithered", outcome.dither.is_some());
    Ok(out.render())
}

/// The calculator screen, computed rather than looked up.
///
/// Takes a tempo, a signature, a BPM unit, a sample rate and a bar count, and
/// returns everything that follows from them — beat and bar lengths, the total
/// duration, and the note-value table.
///
/// **This exists so the calculator is not a second source of truth.** Every
/// number here comes out of the same [`Grid`](loopslcr_core::timing::Grid) the
/// cutter uses, through the same expression. A tab that did its own arithmetic
/// in Kotlin would eventually disagree with the tool it sits next to, and the
/// one thing this program is for is being right about exactly this.
pub fn calculate(params: &str) -> Result<String, String> {
    let fields = json::parse(params)?;

    let mut tempo: Option<Tempo> = None;
    let mut sig = TimeSignature::default();
    let mut unit = BpmUnit::default();
    let mut sample_rate: u32 = 48_000;
    let mut bars: u64 = 4;

    for (key, value) in &fields {
        if *value == Value::Null {
            continue;
        }
        match key.as_str() {
            "bpm" => {
                let number = value.as_f64().ok_or_else(|| "bpm: expected a number".to_string())?;
                tempo = Some(self::tempo(number, "bpm")?);
            }
            "sig" => {
                sig = value
                    .as_str()
                    .ok_or_else(|| "sig: expected a string".to_string())?
                    .parse()
                    .map_err(|e| format!("sig: {e}"))?
            }
            "bpmUnit" => {
                unit = value
                    .as_str()
                    .ok_or_else(|| "bpmUnit: expected a string".to_string())?
                    .parse()
                    .map_err(|e| format!("bpmUnit: {e}"))?
            }
            "sampleRate" => {
                let rate = value
                    .as_u64()
                    .ok_or_else(|| "sampleRate: expected a count".to_string())?;
                sample_rate = u32::try_from(rate)
                    .ok()
                    .filter(|r| *r > 0)
                    .ok_or_else(|| format!("sampleRate: {rate} is not a sample rate"))?;
            }
            "bars" => {
                bars = value
                    .as_u64()
                    .ok_or_else(|| "bars: expected a count".to_string())?
            }
            other => return Err(format!("unknown parameter {other:?}")),
        }
    }

    let tempo = tempo.ok_or_else(|| "bpm is required".to_string())?.with_unit(unit);
    let grid = Grid::new(tempo, sig, sample_rate);

    let per_bar = grid.samples_per_bar();
    let per_beat = grid.samples_per_beat();
    // Beats per bar is the bar measured in BPM units — four in 4/4 counted in
    // quarters, two in 6/8 counted in dotted quarters.
    let beats_per_bar = sig.whole_notes_per_bar() / unit.whole_notes();
    let total = per_bar * Rational::from(bars);

    let mut out = Object::new();
    out.number("tempo", tempo.value().to_f64())
        .string("sig", &format!("{}/{}", sig.num, sig.den))
        .string("bpmUnit", &format!("{}/{}", unit.whole_notes().num(), unit.whole_notes().den()))
        .integer("sampleRate", sample_rate as i64)
        .integer("bars", bars as i64)
        .number("beatsPerBar", beats_per_bar.to_f64())
        .number("barsPerMinute", (Rational::from_int(60) / grid.seconds_per_bar()).to_f64())
        .number("secondsPerBeat", (per_beat / Rational::from(sample_rate)).to_f64())
        .number("samplesPerBeat", per_beat.to_f64())
        .bool("beatSampleExact", per_beat.den() == 1)
        .number("secondsPerBar", grid.seconds_per_bar().to_f64())
        .number("samplesPerBar", per_bar.to_f64())
        .bool("barSampleExact", per_bar.den() == 1)
        .number("barHz", (Rational::from_int(1) / grid.seconds_per_bar()).to_f64())
        .number("totalSeconds", (grid.seconds_per_bar() * Rational::from(bars)).to_f64())
        .number("totalSamples", total.to_f64())
        .integer("totalSamplesRounded", total.round_half_up() as i64)
        .bool("totalSampleExact", total.den() == 1);

    let rows: Vec<Object> = note::table(&grid)
        .iter()
        .map(|row| {
            let mut o = Object::new();
            o.string("label", &row.value.label())
                .integer("denominator", row.value.denominator as i64)
                .string("flavour", flavour_name(row.value.flavour))
                .number("ms", row.millis())
                .number("hz", row.hertz())
                .number("samples", row.samples.to_f64())
                .bool("sampleExact", row.is_sample_exact())
                .number("perBar", row.per_bar.to_f64());
            o
        })
        .collect();
    out.rows("notes", &rows);

    Ok(out.render())
}

fn flavour_name(f: note::Flavour) -> &'static str {
    match f {
        note::Flavour::Straight => "straight",
        note::Flavour::Dotted => "dotted",
        note::Flavour::Triplet => "triplet",
    }
}

/// Reads a flat JSON object into [`Params`].
///
/// Every key is optional, and an unknown key is *refused* rather than ignored:
/// a UI that misspells `targetBpm` would otherwise silently produce a loop at
/// the wrong tempo, and finding that out on a device is much more expensive than
/// finding it out here.
///
/// Returns the parameters and whether a short loop is acceptable — the same
/// division as the CLI, where the pipeline records the shortfall and the caller
/// decides what it means.
pub fn params_from_json(text: &str) -> Result<(Params, bool), String> {
    let fields = json::parse(text)?;
    let mut params = Params::default();
    let mut allow_short = false;
    let mut tape = TapeParams::default();
    let mut tape_named = false;

    for (key, value) in &fields {
        let number = || value.as_f64().ok_or_else(|| format!("{key}: expected a number"));
        let count = || value.as_u64().ok_or_else(|| format!("{key}: expected a count"));
        let flag = || value.as_bool().ok_or_else(|| format!("{key}: expected true or false"));
        let text = || value.as_str().ok_or_else(|| format!("{key}: expected a string"));

        // A null means "not set", which is what `Params::default` already says.
        if *value == Value::Null {
            continue;
        }

        match key.as_str() {
            "bpm" => params.bpm = Some(tempo(number()?, key)?),
            "preferNameTempo" => params.prefer_name_tempo = flag()?,
            "bars" => params.bars = Some(count()?),
            "preferNameBars" => params.prefer_name_bars = flag()?,
            "skip" => params.skip = Some(count()?),
            "sig" => params.sig = text()?.parse().map_err(|e| format!("{key}: {e}"))?,
            "bpmUnit" => params.bpm_unit = text()?.parse().map_err(|e| format!("{key}: {e}"))?,
            "align" => {
                params.align = match text()? {
                    "loop" => Align::Loop,
                    "grid" => Align::Grid,
                    other => return Err(format!("{key}: {other:?} is not loop or grid")),
                }
            }
            "workflow" => {
                params.workflow = match text()? {
                    "auto" => None,
                    "warmup" | "a" => Some(Workflow::WarmupRender),
                    "foldback" | "b" => Some(Workflow::TailFoldback),
                    other => return Err(format!("{key}: {other:?} is not auto, warmup or foldback")),
                }
            }
            "semitones" => {
                params.ratio = Some(Ratio::from_semitones(number()?).map_err(|e| e.to_string())?)
            }
            "cents" => {
                params.ratio = Some(Ratio::from_cents(number()?).map_err(|e| e.to_string())?)
            }
            "targetBpm" => params.target_bpm = Some(tempo(number()?, key)?),
            // A length rather than a tempo — see `Params::target_frames`. The
            // caller that uses it is the second loop, which has to match the
            // first to the sample and cannot get there through a tempo.
            "targetFrames" => params.target_frames = Some(count()? as usize),
            "snap" => params.snap = flag()?,
            "snapWindow" => params.snap_window = count()? as u32,
            "depth" => {
                params.depth = text()?
                    .parse::<BitDepth>()
                    .map_err(|e| format!("{key}: {e}"))?
            }
            "normalize" => params.normalize = flag()?,
            "dither" => {
                params.dither = match text()? {
                    "auto" => pipeline::DitherPolicy::Auto,
                    "flat" | "on" => pipeline::DitherPolicy::Flat,
                    "shaped" => pipeline::DitherPolicy::Shaped,
                    "off" => pipeline::DitherPolicy::Off,
                    other => return Err(format!("{key}: {other:?} is not a dither mode")),
                }
            }
            "ditherSeed" => params.dither_seed = count()?,
            "fadeMs" => params.fade_ms = Some(number()?),
            "noFade" => params.no_fade = flag()?,
            "allowShort" => allow_short = flag()?,

            // Naming any amount switches the character on, exactly as on the
            // command line: a parameter that was clearly set must not be a
            // silent no-op.
            "tape" => {
                tape.enabled = flag()?;
                tape_named = true;
            }
            "wow" => {
                tape.wow_percent = number()?;
                tape_named = true;
            }
            "flutter" => {
                tape.flutter_percent = number()?;
                tape_named = true;
            }
            "hfRolloffHz" => {
                tape.hf_rolloff_hz = number()?;
                tape_named = true;
            }
            "headBumpDb" => {
                tape.head_bump_db = number()?;
                tape_named = true;
            }
            "headBumpHz" => {
                tape.head_bump_hz = number()?;
                tape_named = true;
            }

            other => return Err(format!("unknown parameter {other:?}")),
        }
    }

    if tape_named {
        // An explicit `"tape": false` still wins; anything else naming an amount
        // turns it on.
        if !fields.contains_key("tape") {
            tape.enabled = true;
        }
        params.tape = tape;
    }
    Ok((params, allow_short))
}

/// A tempo from a JSON number, via its decimal spelling.
///
/// The same route as [`Tempo::from_f32`] and for the same reason: 103.5 has to
/// become 207/2, because the exact fraction is what makes the loop length exact.
fn tempo(value: f64, key: &str) -> Result<Tempo, String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{key}: {value} is not a tempo"));
    }
    format!("{value}").parse().map_err(|e| format!("{key}: {e}"))
}

fn workflow_name(w: Workflow) -> &'static str {
    match w {
        Workflow::WarmupRender => "warmup",
        Workflow::TailFoldback => "foldback",
        Workflow::AlreadyTrimmed => "trimmed",
        Workflow::Unclear => "unclear",
    }
}

fn tempo_source_name(s: TempoSource) -> &'static str {
    match s {
        TempoSource::Given => "given",
        TempoSource::AcidChunk => "acid",
        TempoSource::Filename => "filename",
    }
}

fn bars_source_name(s: BarsSource) -> &'static str {
    match s {
        BarsSource::Given => "given",
        BarsSource::Filename => "filename",
        BarsSource::FileLength => "fileLength",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loopslcr_core::AudioBuffer;

    const RATE: u32 = 8_000;
    /// One bar of 4/4 at 200 BPM at [`RATE`]. Exact, deliberately.
    const BAR: usize = 9_600;

    fn file(bars: usize) -> Vec<u8> {
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
        .unwrap()
    }

    fn field(json: &str, key: &str) -> Value {
        json::parse(json).unwrap().get(key).cloned().unwrap()
    }

    #[test]
    fn analysis_reports_the_file_and_what_can_be_read_off_its_name() {
        let json = analyze(&file(8), "200 loop.wav").unwrap();
        assert_eq!(field(&json, "channels").as_u64(), Some(2));
        assert_eq!(field(&json, "sampleRate").as_u64(), Some(8_000));
        assert_eq!(field(&json, "bitsPerSample").as_u64(), Some(24));
        assert_eq!(field(&json, "frames").as_u64(), Some(8 * BAR as u64));
        assert_eq!(field(&json, "nameTempo").as_f64(), Some(200.0));
        assert_eq!(field(&json, "tempo").as_f64(), Some(200.0));
        assert_eq!(field(&json, "loopBars").as_u64(), Some(8));
        assert_eq!(field(&json, "declaredTempo"), Value::Null);
    }

    #[test]
    fn a_file_with_no_tempo_reports_null_rather_than_a_guess() {
        // A UI showing a confident wrong default is worse than one that asks.
        let json = analyze(&file(8), "nameless.wav").unwrap();
        assert_eq!(field(&json, "tempo"), Value::Null);
        assert_eq!(field(&json, "nameTempo"), Value::Null);
        assert_eq!(field(&json, "loopBars"), Value::Null);
        assert_eq!(field(&json, "workflow").as_str(), Some("unclear"));
        // The measurements that need no tempo are still there.
        assert_eq!(field(&json, "frames").as_u64(), Some(8 * BAR as u64));
    }

    #[test]
    fn peaks_come_back_interleaved_by_channel() {
        let peaks = peaks(&file(4), 100).unwrap();
        // 100 buckets × 2 channels × min and max.
        assert_eq!(peaks.len(), 100 * 2 * 2);
        // The two channels hold the same audio, so their extremes match.
        for chunk in peaks.chunks(4) {
            assert_eq!(chunk[0], chunk[2], "channel minima differ");
            assert_eq!(chunk[1], chunk[3], "channel maxima differ");
        }
        assert!(peaks.iter().any(|&v| v > 0.2), "no signal in the peaks");
    }

    #[test]
    fn processing_produces_a_readable_file_at_the_asked_for_depth() {
        let out = process(&file(8), "200 loop.wav", r#"{"depth":"16"}"#).unwrap();
        let wav = Wav::parse(&out).unwrap();
        assert_eq!(wav.format().bits_per_sample, 16);
        assert_eq!(wav.frames(), 8 * BAR);
        // The tempo is declared in the file, which is the point of the tool.
        assert_eq!(wav.tags().declared_tempo(), Some(200.0));
    }

    #[test]
    fn a_short_source_is_refused_unless_allowed() {
        let params = r#"{"bars":16}"#;
        let e = process(&file(8), "200 loop.wav", params).unwrap_err();
        assert!(e.contains("short"), "{e}");

        let allowed = r#"{"bars":16,"allowShort":true}"#;
        assert!(process(&file(8), "200 loop.wav", allowed).is_ok());
    }

    #[test]
    fn an_unknown_parameter_is_refused_rather_than_ignored() {
        // A misspelled `targetBpm` would otherwise silently produce a loop at
        // the wrong tempo, discovered on a device rather than here.
        let e = params_from_json(r#"{"targetBPM":90}"#).unwrap_err();
        assert!(e.contains("unknown parameter"), "{e}");
        assert!(e.contains("targetBPM"), "{e}");
    }

    #[test]
    fn a_parameter_of_the_wrong_type_is_refused() {
        for (bad, want) in [
            (r#"{"bars":"four"}"#, "expected a count"),
            (r#"{"normalize":1}"#, "expected true or false"),
            (r#"{"depth":24}"#, "expected a string"),
            (r#"{"bars":-4}"#, "expected a count"),
        ] {
            let e = params_from_json(bad).unwrap_err();
            assert!(e.contains(want), "{bad}: {e}");
        }
    }

    #[test]
    fn a_null_means_not_set() {
        let (params, _) = params_from_json(r#"{"bars":null,"bpm":null}"#).unwrap();
        assert_eq!(params.bars, None);
        assert_eq!(params.bpm, None);
    }

    #[test]
    fn a_fractional_tempo_arrives_as_an_exact_fraction() {
        let (params, _) = params_from_json(r#"{"bpm":103.5}"#).unwrap();
        assert_eq!(
            params.bpm.unwrap().value(),
            loopslcr_core::Rational::new(207, 2)
        );
    }

    #[test]
    fn naming_a_tape_amount_switches_the_character_on() {
        // The same rule as the command line, where `--wow 0.5` implies `--tape`.
        let (params, _) = params_from_json(r#"{"wow":0.5}"#).unwrap();
        assert!(params.tape.enabled);
        assert_eq!(params.tape.wow_percent, 0.5);

        // An explicit false still wins, so a UI can set amounts and keep them
        // bypassed.
        let (params, _) = params_from_json(r#"{"wow":0.5,"tape":false}"#).unwrap();
        assert!(!params.tape.enabled);
        assert_eq!(params.tape.wow_percent, 0.5);

        // And nothing named leaves the clean path untouched.
        let (params, _) = params_from_json("{}").unwrap();
        assert!(!params.tape.enabled);
    }

    #[test]
    fn the_plan_reports_what_the_run_would_do_without_encoding_anything() {
        let json = plan(&file(8), "200 loop.wav", r#"{"targetBpm":100}"#).unwrap();
        assert_eq!(field(&json, "bars").as_u64(), Some(8));
        assert_eq!(field(&json, "resultingTempo").as_f64(), Some(100.0));
        assert_eq!(field(&json, "ratioExact").as_bool(), Some(true));
        assert_eq!(field(&json, "shortBy").as_u64(), Some(0));
        assert_eq!(field(&json, "barsSource").as_str(), Some("fileLength"));
        assert_eq!(field(&json, "outputFrames").as_u64(), Some(8 * 4 * 60 * 8_000 / 100));
    }

    #[test]
    fn the_plan_carries_the_evidence_the_detection_ran_on() {
        // A UI that only shows *what* was detected states a conclusion the user
        // cannot argue with. `audibleBars` divided by the loop length is the
        // ratio `WorkflowGuess::detect` thresholds on, so publishing it is what
        // lets the choice be judged rather than only obeyed.
        let json = plan(&file(8), "200 loop.wav", "{}").unwrap();
        let bars = field(&json, "bars").as_u64().unwrap() as f64;
        let audible = field(&json, "audibleBars").as_f64().unwrap();
        let in_file = field(&json, "barsInFile").as_f64().unwrap();

        assert!(audible > 0.0, "no audible material reported");
        assert!(
            audible <= in_file + 1e-9,
            "audible {audible} exceeds the file's {in_file} bars"
        );

        // The synthetic file is one 8-bar loop with no tail at all, so it reads
        // as exactly one loop of audible material. That is `trimmed`, not
        // `foldback`: a file this tool has already produced, where folding an
        // empty tail back would be harmless but saying so would be a lie.
        let loops = audible / bars;
        assert!((loops - 1.0).abs() < 0.01, "{loops} loops of audible material");
        assert_eq!(field(&json, "workflowDetected").as_str(), Some("trimmed"));
    }

    #[test]
    fn the_plan_and_the_process_agree_on_the_length() {
        // Two entry points, one pipeline. If they could disagree, a UI would
        // show one number and write another.
        let params = r#"{"targetBpm":150,"tape":true}"#;
        let planned = plan(&file(8), "200 loop.wav", params).unwrap();
        let written = process(&file(8), "200 loop.wav", params).unwrap();
        assert_eq!(
            field(&planned, "outputFrames").as_u64(),
            Some(Wav::parse(&written).unwrap().frames() as u64)
        );
    }

    #[test]
    fn the_same_bytes_and_parameters_produce_the_same_file() {
        // Invariant 4, across the boundary a front end sees.
        let params = r#"{"depth":"16","dither":"shaped","tape":true}"#;
        let a = process(&file(8), "200 loop.wav", params).unwrap();
        let b = process(&file(8), "200 loop.wav", params).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rubbish_input_is_an_error_not_a_panic() {
        // Everything crossing this boundary comes from outside, so nothing here
        // may unwind: on the other side that is undefined behaviour.
        assert!(analyze(b"not a wave file", "x.wav").is_err());
        assert!(analyze(&[], "x.wav").is_err());
        assert!(peaks(b"nope", 100).is_err());
        assert!(process(b"nope", "x.wav", "{}").is_err());
        assert!(process(&file(4), "200 loop.wav", "not json").is_err());
        // Zero buckets is a degenerate request, not a crash.
        assert!(peaks(&file(4), 0).unwrap().is_empty());
    }

    #[test]
    fn the_calculator_agrees_with_the_cutter() {
        // The reason this call exists at all. 103 BPM in 4/4 at 44.1 kHz is the
        // reference case, and its bar length is the number the whole tool is
        // built around: 60·44100·4/103 = 102757.28…
        let json = calculate("{\"bpm\":103,\"sampleRate\":44100,\"bars\":16}").expect("no result");
        assert!(json.contains("\"samplesPerBar\":102757.28"), "{json}");
        assert!(json.contains("\"barSampleExact\":false"), "{json}");
        assert!(json.contains("\"beatsPerBar\":4.0"), "{json}");

        // 16 bars from zero rounds to 1 644 117, and the cutter's 8-bar loop
        // starting at bar 8 ends at 1 644 116. Both are right: `Align::Loop`
        // makes the loop `round(8 · spb)` long from a start that was rounded
        // separately, rather than the difference of two rounded bar lines. The
        // one-sample gap between the two answers *is* the alignment choice, and
        // a calculator that quietly reported the other number would be
        // contradicting the screen next to it.
        assert!(json.contains("\"totalSamplesRounded\":1644117"), "{json}");
        assert!(json.contains("\"totalSeconds\":37.28"), "{json}");
    }

    #[test]
    fn a_round_tempo_is_reported_as_sample_exact() {
        let json = calculate("{\"bpm\":120,\"sampleRate\":48000}").expect("no result");
        assert!(json.contains("\"samplesPerBar\":96000.0"), "{json}");
        assert!(json.contains("\"barSampleExact\":true"), "{json}");
        assert!(json.contains("\"beatSampleExact\":true"), "{json}");
    }

    #[test]
    fn the_note_table_comes_back_as_rows() {
        let json = calculate("{\"bpm\":120,\"sampleRate\":48000}").expect("no result");
        assert!(json.contains("\"notes\":["), "{json}");
        // A quarter at 120 is half a second; a dotted eighth is 375 ms.
        assert!(json.contains("\"label\":\"1/4\""), "{json}");
        assert!(json.contains("\"label\":\"1/8.\""), "{json}");
        assert!(json.contains("\"label\":\"1/16T\""), "{json}");
        assert!(json.contains("\"ms\":500.0"), "{json}");
        assert!(json.contains("\"ms\":375.0"), "{json}");
    }

    #[test]
    fn the_signature_and_the_bpm_unit_are_honoured() {
        // 6/8 counted in dotted quarters: two beats to the bar, not six.
        let json = calculate("{\"bpm\":120,\"sig\":\"6/8\",\"bpmUnit\":\"3/8\"}").expect("no result");
        assert!(json.contains("\"beatsPerBar\":2.0"), "{json}");
        assert!(json.contains("\"bpmUnit\":\"3/8\""), "{json}");
    }

    #[test]
    fn a_missing_tempo_is_refused_rather_than_guessed() {
        let e = calculate("{\"bars\":4}").expect_err("a tempo was invented");
        assert!(e.contains("bpm"), "{e}");
    }

    #[test]
    fn a_misspelled_calculator_parameter_is_refused() {
        assert!(calculate("{\"sampleRatte\":48000,\"bpm\":120}").is_err());
        assert!(calculate("{\"bpm\":120,\"sampleRate\":0}").is_err());
        assert!(calculate("{\"bpm\":0}").is_err());
    }

    #[test]
    fn the_calculator_output_is_byte_identical_twice() {
        let a = calculate("{\"bpm\":103.5,\"sampleRate\":44100}").expect("no result");
        let b = calculate("{\"bpm\":103.5,\"sampleRate\":44100}").expect("no result");
        assert_eq!(a, b);
    }
}
