//! LOOP_SLCR command line interface.

mod batch;
mod preset;

use std::error::Error;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use batch::BatchArgs;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use loopslcr_core::analysis::{Peaks, Tail, Workflow, WorkflowGuess};
use loopslcr_core::naming;
use loopslcr_core::ops::{dither, tape, TapeParams};
use loopslcr_core::pipeline;
use loopslcr_core::timing::{Align, BpmUnit, Grid, Ratio, Tempo, TimeSignature};
use loopslcr_core::wav::{chunks::Chunks, write, BitDepth, Metadata, SampleFormat, Wav, WriteSpec};

#[derive(Parser)]
#[command(
    name = "loopslcr",
    version,
    about = "Sample-exact loop trimming",
    // A preset is spliced in ahead of what was typed, so every flag has to be
    // allowed to appear twice with the later one winning. Without this clap
    // refuses the repetition and the preset can only ever add, never override.
    args_override_self = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The variants differ a lot in size, and deliberately so: `Cut` carries every
/// flag of the pipeline. Boxing it to even the sizes out would add an
/// indirection to a value that is built once at startup and destructured on the
/// next line.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    /// Show what a WAVE file contains: format, duration, declared tempo, tags.
    Info {
        /// The file to inspect.
        file: PathBuf,

        /// Tempo to measure the file against. Defaults to what the file
        /// declares in its `acid` chunk, if anything.
        #[arg(long)]
        bpm: Option<Tempo>,

        /// Time signature, N/D.
        #[arg(long, default_value = "4/4")]
        sig: TimeSignature,

        /// Note value the BPM counts, as a fraction of a whole note.
        #[arg(long = "bpm-unit", default_value = "1/4")]
        bpm_unit: BpmUnit,

        /// Loop length to measure the file against, in bars.
        #[arg(long, default_value_t = 8)]
        bars: u64,

        /// Skip decoding: no peak, no tail measurement, no workflow guess.
        #[arg(long)]
        no_peak: bool,

        /// Draw the waveform this many columns wide.
        #[arg(long, num_args = 0..=1, default_missing_value = "72")]
        waveform: Option<usize>,
    },

    /// Trim a rendered loop to a sample-exact N-bar loop.
    Cut {
        /// The file to cut.
        file: PathBuf,

        /// Where to write. Defaults to `{name}_{bpm}bpm_{bars}bars.wav` beside
        /// the source.
        #[arg(long, short)]
        out: Option<PathBuf>,

        #[command(flatten)]
        flags: CutFlags,
    },

    /// Cut every loop in a directory, carrying on past the ones that cannot be.
    Batch {
        /// The directory to walk.
        dir: PathBuf,

        /// Where to write. Defaults to beside each source. The tree below `dir`
        /// is recreated underneath, so two loops of the same name in different
        /// folders stay two files.
        #[arg(long = "out-dir")]
        out_dir: Option<PathBuf>,

        /// Descend into subdirectories.
        #[arg(long, short)]
        recursive: bool,

        /// Worker threads. Defaults to one per core.
        #[arg(long, short)]
        jobs: Option<usize>,

        /// Print each file's full report instead of one line per file.
        #[arg(long, short)]
        verbose: bool,

        #[command(flatten)]
        flags: CutFlags,
    },

    /// Print a shell completion script.
    Completions {
        /// The shell to generate for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Show where the bar grid puts the cut points, and how exact they are.
    Grid {
        /// Tempo: 103, 103.5, or an exact fraction like 207/2.
        #[arg(long)]
        bpm: Tempo,

        /// Bars to keep.
        #[arg(long, default_value_t = 8)]
        bars: u64,

        /// Warmup bars to skip. Defaults to `--bars`: skip exactly one loop
        /// length of warmup, which is right for any meter or loop length.
        #[arg(long)]
        skip: Option<u64>,

        /// Time signature, N/D.
        #[arg(long, default_value = "4/4")]
        sig: TimeSignature,

        /// Note value the BPM counts, as a fraction of a whole note.
        #[arg(long = "bpm-unit", default_value = "1/4")]
        bpm_unit: BpmUnit,

        /// Sample rate in Hz.
        #[arg(long, default_value_t = 44_100)]
        sr: u32,

        /// Which constraint wins when a bar is not a whole number of samples.
        #[arg(long, value_enum, default_value_t = AlignArg::Loop)]
        align: AlignArg,

        /// Search radius for the sample-exact tempo suggestions.
        #[arg(long, default_value_t = 15)]
        window: u32,
    },
}

/// Every flag of the cutting pipeline, shared by `cut` and `batch`.
///
/// One struct rather than two lists, so a flag added here reaches both
/// subcommands and they cannot drift apart.
#[derive(clap::Args, Clone, Debug)]
struct CutFlags {
    /// Tempo. Defaults to the file's `acid` chunk, then to its filename.
    #[arg(long)]
    bpm: Option<Tempo>,

    /// Read the tempo from the filename in preference to the `acid` chunk.
    #[arg(long = "bpm-from-name")]
    bpm_from_name: bool,

    /// Bars to keep. Derived from the file's own length when omitted.
    #[arg(long)]
    bars: Option<u64>,

    /// Prefer a loop length stated in the filename (`4BRS`, `2BARS`) over
    /// the one derived from the file's length.
    #[arg(long = "bars-from-name")]
    bars_from_name: bool,

    /// Warmup bars to skip. Defaults to what the file's shape implies.
    #[arg(long)]
    skip: Option<u64>,

    /// Time signature, N/D.
    #[arg(long, default_value = "4/4")]
    sig: TimeSignature,

    /// Note value the BPM counts, as a fraction of a whole note.
    #[arg(long = "bpm-unit", default_value = "1/4")]
    bpm_unit: BpmUnit,

    /// Which constraint wins when a bar is not a whole number of samples.
    #[arg(long, value_enum, default_value_t = AlignArg::Loop)]
    align: AlignArg,

    /// Which workflow the source was rendered for.
    #[arg(long = "path", value_enum, default_value_t = PathArg::Auto)]
    workflow: PathArg,

    /// Varispeed by an interval: `-2.34` semitones, or `-234c` in cents.
    /// Pitch and tempo move together, tape style.
    #[arg(long, allow_hyphen_values = true)]
    pitch: Option<String>,

    /// Varispeed to land on this tempo. Exact: the ratio is a fraction.
    #[arg(long = "target-bpm", conflicts_with = "pitch")]
    target_bpm: Option<Tempo>,

    /// Nudge the resulting tempo to the nearest one where the loop is a
    /// whole number of samples. On its own, makes the loop sample-exact.
    #[arg(long)]
    snap: bool,

    /// Search radius in BPM for --snap.
    #[arg(long = "snap-window", default_value_t = 15)]
    snap_window: u32,

    /// Output bit depth: 16, 24, 32, or 32f.
    #[arg(long, default_value = "24")]
    depth: BitDepth,

    /// Scale the loop so its peak sits at full scale. Off by default: a
    /// level change is a decision about the material.
    #[arg(long)]
    normalize: bool,

    /// Dither: `auto` applies flat TPDF only when the bit depth drops,
    /// `shaped` adds noise shaping, `on` forces flat, `off` refuses.
    #[arg(long, default_value = "auto")]
    dither: DitherArg,

    /// Dither seed. Fixed by default so runs reproduce byte for byte.
    #[arg(long = "dither-seed", default_value_t = dither::DEFAULT_SEED)]
    dither_seed: u64,

    /// Tape character: wow, flutter, head-gap HF loss, head bump. One
    /// switch, so the clean path stays byte-identical when it is off.
    #[arg(long)]
    tape: bool,

    /// Wow depth as peak speed deviation in percent. Implies --tape.
    #[arg(long)]
    wow: Option<f64>,

    /// Flutter depth as peak speed deviation in percent. Implies --tape.
    #[arg(long)]
    flutter: Option<f64>,

    /// Head-gap loss corner: `auto`, `off`, or a frequency in Hz at nominal
    /// speed — it is scaled by the varispeed ratio. Implies --tape.
    #[arg(long = "hf-rolloff")]
    hf_rolloff: Option<String>,

    /// Head bump height in dB, or `off`. Implies --tape.
    #[arg(long = "head-bump")]
    head_bump: Option<String>,

    /// Micro-fade length in milliseconds. Defaults to 0.5 ms on a straight
    /// cut and to none on a foldback, which is seamless already.
    #[arg(long)]
    fade: Option<f64>,

    /// Never fade, whichever path is taken.
    #[arg(long = "no-fade", conflicts_with = "fade")]
    no_fade: bool,

    /// Report what would happen and write nothing.
    #[arg(long = "dry-run")]
    dry_run: bool,

    /// Write the loop even when the source is too short to fill it. The
    /// result will drift against a sequencer — re-rendering is the fix.
    #[arg(long = "allow-short")]
    allow_short: bool,

    /// Overwrite an existing output file.
    #[arg(long)]
    force: bool,

    /// Ignore any `loopslcr.args` preset in the directory.
    #[arg(long = "no-preset")]
    no_preset: bool,
}

/// When to dither.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum DitherArg {
    /// Flat TPDF, but only when the bit depth drops — the only time it helps.
    Auto,
    /// Flat TPDF, always, for integer output.
    On,
    /// TPDF plus second-order noise shaping: the added noise is pushed above
    /// 15 kHz, costing total noise power to buy about 10 dB where it is heard.
    Shaped,
    Off,
}

/// Parses `--pitch`: bare digits are semitones, a trailing `c` means cents.
///
/// Two units on one flag rather than two flags, because they are one law — and
/// `-234c` is how the interval reads on a tape machine's own scale.
fn parse_pitch(text: &str) -> Result<Ratio, Box<dyn Error>> {
    let trimmed = text.trim();
    let (number, in_cents) = match trimmed.strip_suffix(['c', 'C']) {
        Some(rest) => (rest, true),
        // `st`, `s` and a bare number all mean semitones.
        None => (trimmed.trim_end_matches(['s', 'S', 't', 'T']), false),
    };
    let value: f64 = number
        .trim()
        .parse()
        .map_err(|_| format!("invalid --pitch {text:?}: expected -2.34 or -234c"))?;
    let ratio = if in_cents {
        Ratio::from_cents(value)
    } else {
        Ratio::from_semitones(value)
    };
    Ok(ratio?)
}

/// Collects the character flags into one [`TapeParams`].
///
/// Naming any amount switches the character on. The alternative — requiring
/// `--tape` alongside `--wow 0.5` — would make a flag that was clearly asked for
/// do nothing at all, and a silent no-op is worse than an implication.
fn resolve_tape(
    tape: bool,
    wow: Option<f64>,
    flutter: Option<f64>,
    hf_rolloff: &Option<String>,
    head_bump: &Option<String>,
) -> Result<TapeParams, Box<dyn Error>> {
    let named = tape
        || wow.is_some()
        || flutter.is_some()
        || hf_rolloff.is_some()
        || head_bump.is_some();
    let mut params = TapeParams {
        enabled: named,
        ..TapeParams::default()
    };

    for (name, value, target) in [
        ("--wow", wow, &mut params.wow_percent),
        ("--flutter", flutter, &mut params.flutter_percent),
    ] {
        if let Some(value) = value {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("invalid {name} {value}: expected a percentage from 0 up").into());
            }
            *target = value;
        }
    }

    if let Some(text) = hf_rolloff {
        params.hf_rolloff_hz = parse_amount(text, "--hf-rolloff", tape::DEFAULT_HF_ROLLOFF_HZ)?;
    }
    if let Some(text) = head_bump {
        params.head_bump_db = parse_amount(text, "--head-bump", tape::DEFAULT_HEAD_BUMP_DB)?;
    }
    Ok(params)
}

/// Parses `auto`, `off`, or a number, for the flags where zero means off.
fn parse_amount(text: &str, flag: &str, default: f64) -> Result<f64, Box<dyn Error>> {
    match text.trim() {
        "auto" => Ok(default),
        "off" | "none" | "no" => Ok(0.0),
        number => number
            .parse()
            .map_err(|_| format!("invalid {flag} {text:?}: expected auto, off, or a number").into()),
    }
}

/// Which of the two render shapes the source has.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum PathArg {
    /// Decide from the file's own length and tail.
    Auto,
    /// Warmup render: skip one loop of warmup, keep the next, discard the tail.
    A,
    /// One loop plus tail: keep from the start and fold the tail back onto it.
    B,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum AlignArg {
    /// Cut-in on the grid, length exactly `bars` long.
    Loop,
    /// Both markers on the grid; length may differ by a sample.
    Grid,
}

impl From<PathArg> for Option<Workflow> {
    fn from(p: PathArg) -> Option<Workflow> {
        match p {
            PathArg::Auto => None,
            PathArg::A => Some(Workflow::WarmupRender),
            PathArg::B => Some(Workflow::TailFoldback),
        }
    }
}

impl From<DitherArg> for pipeline::DitherPolicy {
    fn from(d: DitherArg) -> pipeline::DitherPolicy {
        match d {
            DitherArg::Auto => pipeline::DitherPolicy::Auto,
            DitherArg::On => pipeline::DitherPolicy::Flat,
            DitherArg::Shaped => pipeline::DitherPolicy::Shaped,
            DitherArg::Off => pipeline::DitherPolicy::Off,
        }
    }
}

impl From<AlignArg> for Align {
    fn from(a: AlignArg) -> Align {
        match a {
            AlignArg::Loop => Align::Loop,
            AlignArg::Grid => Align::Grid,
        }
    }
}

fn main() -> ExitCode {
    match parse_with_preset().and_then(|(cli, note)| emit(run(cli, note)?)) {
        Ok(code) => code,
        // Downstream closed the pipe — `loopslcr info x.wav | head` is a
        // normal way to use this, not a failure to report.
        Err(e) if is_broken_pipe(e.as_ref()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn is_broken_pipe(e: &(dyn Error + 'static)) -> bool {
    let mut current = Some(e);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<io::Error>() {
            if io.kind() == io::ErrorKind::BrokenPipe {
                return true;
            }
        }
        current = e.source();
    }
    false
}

/// What a subcommand produced.
struct Report {
    text: String,
    /// True when at least one file in a batch could not be processed. A
    /// half-finished batch must not look like a success to whatever called it.
    failed: bool,
}

impl From<String> for Report {
    /// A single-file subcommand either produced its report or returned an error,
    /// so there is no partial success to represent.
    fn from(text: String) -> Self {
        Report { text, failed: false }
    }
}

/// One file's cut: the full report, and a line short enough for a batch listing.
struct Cut {
    text: String,
    summary: String,
}

/// Where a cut's output goes.
///
/// Three cases rather than an `Option<PathBuf>`, because naming a directory and
/// naming a file are different instructions: in a directory the filename still
/// comes from the template, and the template needs the tempo the loop ends up
/// at, which is not known until the varispeed has run.
#[derive(Copy, Clone, Debug)]
enum Out<'a> {
    /// Named explicitly, template not used.
    File(&'a Path),
    /// Named by the template, in this directory.
    Dir(&'a Path),
    /// Named by the template, beside the source.
    BesideSource,
}

/// Writes a report to stdout and turns it into an exit code.
fn emit(report: Report) -> Result<ExitCode, Box<dyn Error>> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(report.text.as_bytes())?;
    out.flush()?;
    Ok(if report.failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Parses the command line, then re-parses it with the directory's preset
/// spliced in.
///
/// Two passes because the preset's location is itself an argument: the file to
/// cut names the directory to look in. The second parse is the one that counts,
/// and it is the same parser, so a preset cannot express anything the command
/// line could not.
fn parse_with_preset() -> Result<(Cli, Option<String>), Box<dyn Error>> {
    let argv: Vec<OsString> = std::env::args_os().collect();
    let first = Cli::parse_from(&argv);

    let dir = match &first.command {
        Command::Cut { file, flags, .. } if !flags.no_preset => file.parent().map(Path::to_path_buf),
        Command::Batch { dir, flags, .. } if !flags.no_preset => Some(dir.clone()),
        _ => None,
    };
    let Some(dir) = dir else {
        return Ok((first, None));
    };
    let Some(args) = preset::load(&dir)? else {
        return Ok((first, None));
    };

    let note = format!(
        "  preset       {} — {}\n",
        dir.join(preset::FILE_NAME).display(),
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok((Cli::parse_from(preset::splice(&argv, args)), Some(note)))
}

fn run(cli: Cli, preset_note: Option<String>) -> Result<Report, Box<dyn Error>> {
    let text = match cli.command {
        Command::Info {
            file,
            bpm,
            sig,
            bpm_unit,
            bars,
            no_peak,
            waveform,
        } => {
            let bytes = std::fs::read(&file)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let wav = Wav::parse(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            let size_error = Chunks::size_field_error(&bytes).unwrap_or(0);
            render_info(
                &file,
                bytes.len(),
                size_error,
                &wav,
                bpm,
                sig,
                bpm_unit,
                bars,
                !no_peak,
                waveform,
            )?
            .into()
        }
        Command::Cut { file, out, flags } => {
            let where_to = out.as_deref().map_or(Out::BesideSource, Out::File);
            run_cut(&file, where_to, &flags.resolve()?)?.text.into()
        }
        Command::Batch {
            dir,
            out_dir,
            recursive,
            jobs,
            verbose,
            flags,
        } => batch::run(BatchArgs {
            dir,
            out_dir,
            recursive,
            jobs,
            verbose,
            settings: flags.resolve()?,
        })?,
        Command::Grid {
            bpm,
            bars,
            skip,
            sig,
            bpm_unit,
            sr,
            align,
            window,
        } => {
            let skip = skip.unwrap_or(bars);
            let grid = Grid::new(bpm.with_unit(bpm_unit), sig, sr);
            render_grid(&grid, skip, bars, align.into(), window).into()
        }
        Command::Completions { shell } => {
            let mut buffer = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "loopslcr", &mut buffer);
            Report::from(String::from_utf8(buffer)?)
        }
    };

    // The preset goes at the top of the report, never silently: a file that
    // changes what the command does has to be visible in the command's output.
    Ok(match preset_note {
        Some(note) => Report {
            text: note + &text.text,
            ..text
        },
        None => text,
    })
}

/// The resolved flags: the pipeline's own parameters, plus the three decisions
/// that belong to a command line rather than to a cut.
///
/// Resolved once, before the first file is touched, so a bad `--pitch` fails
/// once rather than 279 times.
struct Settings {
    params: pipeline::Params,
    /// Report and write nothing.
    dry_run: bool,
    /// Accept a loop the source is too short to fill.
    allow_short: bool,
    /// Overwrite an existing output file.
    force: bool,
}

impl CutFlags {
    /// Validates and parses everything that can fail, once.
    fn resolve(&self) -> Result<Settings, Box<dyn Error>> {
        Ok(Settings {
            params: pipeline::Params {
                bpm: self.bpm,
                prefer_name_tempo: self.bpm_from_name,
                bars: self.bars,
                prefer_name_bars: self.bars_from_name,
                skip: self.skip,
                sig: self.sig,
                bpm_unit: self.bpm_unit,
                align: self.align.into(),
                workflow: self.workflow.into(),
                ratio: self.pitch.as_deref().map(parse_pitch).transpose()?,
                target_bpm: self.target_bpm,
                snap: self.snap,
                snap_window: self.snap_window,
                depth: self.depth,
                normalize: self.normalize,
                dither: self.dither.into(),
                dither_seed: self.dither_seed,
                tape: resolve_tape(
                    self.tape,
                    self.wow,
                    self.flutter,
                    &self.hf_rolloff,
                    &self.head_bump,
                )?,
                fade_ms: self.fade,
                no_fade: self.no_fade,
            },
            dry_run: self.dry_run,
            allow_short: self.allow_short,
            force: self.force,
        })
    }
}

/// Cuts a loop and writes it, or says what it would have written.
///
/// The cutting itself is [`pipeline::run`]; everything here is reading the file,
/// turning the outcome into lines, and deciding what to do about it. That split
/// is what lets a second front end exist without a second set of answers to
/// which tempo, which loop length, which shape.
///
/// Everything that could make the result silently wrong is a hard error here
/// rather than a warning: a loop that is short drifts, and an output that
/// overwrites a source is unrecoverable. Warnings are for things the ear can
/// judge.
fn run_cut(file: &Path, out: Out, args: &Settings) -> Result<Cut, Box<dyn Error>> {
    let named = |e: String| format!("{}: {e}", file.display());

    let bytes = std::fs::read(file).map_err(|e| named(e.to_string()))?;
    let wav = Wav::parse(&bytes).map_err(|e| named(e.to_string()))?;
    let name = file.file_name().unwrap_or_default().to_string_lossy();

    let outcome = pipeline::run(&wav, &name, &args.params).map_err(|e| named(e.to_string()))?;
    let rate = outcome.sample_rate;

    let mut s = String::new();
    writeln!(s, "{}", file.display())?;
    writeln!(
        s,
        "  source       {} frames, {}  at {} ({}), {}",
        outcome.source_frames,
        timecode(outcome.source_frames as u64, rate),
        outcome.tempo,
        outcome.tempo_source.label(),
        outcome.sig()
    )?;
    writeln!(
        s,
        "  shape        {:.4} bars, {:.4} audible — {}",
        outcome.shape.bars_in_file,
        outcome.shape.audible_bars,
        describe(outcome.detected)
    )?;
    writeln!(
        s,
        "  loop         {} bars{}, skip {}, align {}",
        outcome.bars,
        outcome.bars_source.label(),
        outcome.skip_bars,
        args.params.align
    )?;
    if args.params.workflow.is_some() && outcome.chosen != outcome.detected {
        writeln!(s, "  ! --path overrides the detected shape")?;
    }

    // Shortness is decided once, because both paths fail the same way: the
    // region names more frames than the file holds. Much of the archive is short
    // by a handful of frames — a previous tool rounded down — and 8 frames per
    // repeat is still drift, so this is refused rather than absorbed. A dry run
    // reports it instead: surveying an archive is exactly when you want to see
    // the problem rather than stop at it.
    if outcome.short_by > 0 {
        writeln!(
            s,
            "  ! short by {} frames — the file holds {} of the {} a {}-bar loop needs at this tempo",
            outcome.short_by, outcome.available, outcome.loop_frames, outcome.bars
        )?;
        if !args.dry_run && !args.allow_short {
            return Err(named(format!(
                "{} frames short of a {}-bar loop — re-render longer, lower --bars, \
                 or pass --allow-short to accept a loop that drifts",
                outcome.short_by, outcome.bars
            ))
            .into());
        }
    }

    match &outcome.taken {
        pipeline::Taken::Foldback(report) => writeln!(
            s,
            "  path B — folded {} tail frames back in {} wrap(s), peak {:.6} → {:.6}",
            report.tail_frames, report.wraps, report.peak_before, report.peak_after
        )?,
        taken => {
            if matches!(taken, pipeline::Taken::TooShortToFold) {
                writeln!(s, "  ! too short to fold — taking a straight cut instead")?;
            }
            writeln!(
                s,
                "  path A — cut {} .. {}, tail discarded",
                outcome.region.start, outcome.region.end
            )?;
            if outcome.short_by > 0 {
                writeln!(
                    s,
                    "  ! the result will drift: it is not a whole {} bars",
                    outcome.bars
                )?;
            }
        }
    }

    if outcome.fade.is_none() {
        writeln!(s, "  fade         none")?;
    } else {
        writeln!(
            s,
            "  fade         {} frames each end, {:?}",
            outcome.fade.frames, outcome.fade.shape
        )?;
    }

    if !outcome.ratio.is_unity() {
        writeln!(s, "  varispeed    {}", outcome.ratio)?;
        writeln!(
            s,
            "               {} → {}, {} frames{}",
            outcome.tempo,
            outcome.final_tempo,
            outcome.buffer.frames(),
            if outcome.ratio.is_exact() { ", exact ratio" } else { "" }
        )?;
    }

    if let Some(line) = describe_tape(&outcome.tape) {
        write!(s, "{line}")?;
    } else if args.params.tape.enabled {
        writeln!(s, "  tape         on, but every amount is zero — nothing applied")?;
    }

    if let Some((factor, before)) = outcome.normalized {
        writeln!(
            s,
            "  normalize    ×{factor:.6} ({:+.2} dB), peak {before:.6} → 1.000000",
            20.0 * factor.log10()
        )?;
    }

    match outcome.dither {
        Some((_, applied)) => writeln!(
            s,
            "  dither       {} at {} bit, seed {}{}",
            if applied.quantised {
                "TPDF ±1 LSB, 2nd-order shaped"
            } else {
                "TPDF ±1 LSB"
            },
            args.params.depth.bits(),
            args.params.dither_seed,
            // Worth saying, because it is the one place a stage downstream of
            // the dither would be a mistake: the samples are already on the
            // output grid, so anything touching them now would need dithering
            // all over again.
            if applied.quantised { ", already quantised" } else { "" }
        )?,
        None if args.params.dither == pipeline::DitherPolicy::Auto
            && !args.params.depth.is_float() =>
        {
            writeln!(
                s,
                "  dither       none ({} bit in, {} bit out — no depth reduction)",
                outcome.source_bits,
                args.params.depth.bits()
            )?
        }
        None => {}
    }

    writeln!(
        s,
        "  result       {} frames, {}  peak {:.6} {}",
        outcome.buffer.frames(),
        timecode(outcome.buffer.frames() as u64, rate),
        outcome.peak.value,
        dbfs(outcome.peak.value)
    )?;
    if outcome.peak.clips() && args.params.depth.clips() {
        writeln!(
            s,
            "  ! peak is past full scale — {}-bit output will clip. Use --normalize or --depth 32f",
            args.params.depth.bits()
        )?;
    }

    // The grid residual describes the *region*, so it would understate a short
    // output by orders of magnitude: 78 missing frames are 1.8 ms of drift per
    // repeat, not the 4 µs of rounding the region carries.
    if let Some((missing, seconds)) = outcome.drift() {
        writeln!(
            s,
            "  length       {missing} frames short — drifts {:.3} ms per repeat",
            seconds * 1000.0
        )?;
    } else if outcome.ratio.is_unity() {
        writeln!(
            s,
            "  length       {}",
            outcome
                .grid
                .length_residual(outcome.skip_bars, outcome.bars, args.params.align)
        )?;
    } else {
        // After a varispeed the residual to report is against the *new* grid:
        // the old one no longer describes this file.
        let scaled = outcome.grid.scaled(outcome.ratio).map_err(|e| named(e.to_string()))?;
        writeln!(
            s,
            "  length       {} vs {:.4} exact at {}{}",
            outcome.buffer.frames(),
            scaled.exact_sample(outcome.bars).to_f64(),
            outcome.final_tempo,
            if scaled.is_sample_exact(outcome.bars) {
                " — sample-exact"
            } else {
                ""
            }
        )?;
    }

    // The output is named and tagged with the tempo it actually plays at.
    let dest = match out {
        Out::File(path) => path.to_path_buf(),
        Out::Dir(dir) => dir.join(default_name(file, &outcome.final_tempo, outcome.bars)),
        Out::BesideSource => {
            file.with_file_name(default_name(file, &outcome.final_tempo, outcome.bars))
        }
    };
    writeln!(s, "  out          {}", dest.display())?;

    // The one-line summary is assembled from the facts, not scraped back out of
    // the report: a batch listing needs the tempo, length and destination, and
    // the last line of the report happens to be none of those.
    let summary = format!(
        "{} bars at {}, {} frames → {}",
        outcome.bars,
        outcome.final_tempo,
        outcome.buffer.frames(),
        dest.file_name().unwrap_or_default().to_string_lossy()
    );

    if args.dry_run {
        writeln!(s, "  dry run — nothing written")?;
        return Ok(Cut { text: s, summary: format!("{summary} (dry run)") });
    }

    // Refusing to overwrite the source is not the same check as refusing to
    // overwrite any existing file: --force must not be able to destroy the
    // input, since that is the one file that cannot be reproduced.
    if same_file(file, &dest) {
        return Err(format!("{}: output would overwrite the source", dest.display()).into());
    }
    if dest.exists() && !args.force {
        return Err(format!("{}: exists — pass --force to overwrite", dest.display()).into());
    }

    let comment = format!(
        "LOOP_SLCR: {}, {} bars, {}, {} frames{}",
        outcome.final_tempo,
        outcome.bars,
        outcome.sig(),
        outcome.buffer.frames(),
        if outcome.ratio.is_unity() {
            String::new()
        } else {
            format!(
                " (varispeed {:+.3} st from {})",
                outcome.ratio.semitones(),
                outcome.tempo
            )
        }
    );
    let spec = WriteSpec::new(args.params.depth).with_metadata(
        Metadata::for_loop(
            outcome.final_tempo.value().to_f64() as f32,
            u32::try_from(outcome.beats()).unwrap_or(u32::MAX),
            outcome.sig().num as u16,
            outcome.sig().den as u16,
            u32::try_from(outcome.buffer.frames()).unwrap_or(u32::MAX),
        )
        .with_comment(comment),
    );
    let encoded = write(&outcome.buffer, &spec)?;
    std::fs::write(&dest, &encoded).map_err(|e| format!("{}: {e}", dest.display()))?;
    writeln!(
        s,
        "  wrote        {} bytes, {}-bit",
        encoded.len(),
        args.params.depth.bits()
    )?;

    Ok(Cut {
        text: s,
        summary: format!("{summary}, {}-bit", args.params.depth.bits()),
    })
}

/// Reports the character actually imposed, or `None` if none was.
///
/// The rates printed are the quantised ones, not the ones requested: the loop
/// grid is what the modulation had to land on, and a report showing 0.7 Hz where
/// 0.68 Hz was used would hide the one mechanism that keeps the loop exact.
fn describe_tape(t: &tape::Tape) -> Option<String> {
    if t.is_noop() {
        return None;
    }
    let mut s = String::new();
    let mut label = "  tape         ";

    for (name, wobble) in [("wow", &t.wow), ("flutter", &t.flutter)] {
        if let Some(w) = wobble {
            let rates: Vec<String> = w.rates_hz.iter().map(|hz| format!("{hz:.3}")).collect();
            let _ = writeln!(
                s,
                "{label}{name} {:.3} % at {} Hz ({} cycles/loop), ±{:.2} frames",
                w.depth_percent,
                rates.join(" + "),
                w.cycles.iter().map(u64::to_string).collect::<Vec<_>>().join(" + "),
                w.peak_frames
            );
            label = "               ";
        }
    }
    if let Some(hz) = t.hf_rolloff_hz {
        let _ = writeln!(s, "{label}HF rolloff 6 dB/oct from {hz:.0} Hz");
        label = "               ";
    }
    if let Some((hz, db)) = t.head_bump {
        let _ = writeln!(s, "{label}head bump {db:+.1} dB at {hz:.0} Hz, Q {}", tape::HEAD_BUMP_Q);
    }
    Some(s)
}

fn describe(w: Workflow) -> &'static str {
    match w {
        Workflow::WarmupRender => "warmup render (path A)",
        Workflow::TailFoldback => "one loop plus tail (path B)",
        Workflow::AlreadyTrimmed => "already trimmed",
        Workflow::Unclear => "shape unclear, defaulting to a straight cut",
    }
}

/// `{name}_{bpm}bpm_{bars}bars.wav`, beside the source.
fn default_name(source: &Path, tempo: &Tempo, bars: u64) -> String {
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    let stem = naming::output_stem(&name);
    let bpm = tempo.value();
    // A fractional tempo has to survive into the name — `58.5` truncated to
    // `58` names a file that is not the file.
    let bpm = if bpm.is_integer() {
        bpm.num().to_string()
    } else {
        format!("{}", bpm.to_f64()).replace('.', "p")
    };
    format!("{stem}_{bpm}bpm_{bars}bars.wav")
}

/// Whether two paths name the same existing file.
///
/// Compares canonical paths, so `./x.wav` and `x.wav` are recognised as one.
/// A destination that does not exist yet cannot be the source.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_info(
    path: &Path,
    file_size: usize,
    size_field_error: i64,
    wav: &Wav,
    bpm: Option<Tempo>,
    sig: TimeSignature,
    bpm_unit: BpmUnit,
    loop_bars: u64,
    decode: bool,
    waveform: Option<usize>,
) -> Result<String, Box<dyn Error>> {
    let mut s = String::new();
    let format = wav.format();

    writeln!(s, "{}", path.display())?;
    writeln!(
        s,
        "  {} {}-bit {}  {} Hz  {} frames  {}",
        match format.format {
            SampleFormat::Int => "PCM int",
            SampleFormat::Float => "IEEE float",
        },
        format.bits_per_sample,
        match format.channels {
            1 => "mono".to_string(),
            2 => "stereo".to_string(),
            n => format!("{n} channels"),
        },
        format.sample_rate,
        wav.frames(),
        timecode(wav.frames() as u64, format.sample_rate)
    )?;
    writeln!(s, "  file size    {file_size} bytes")?;

    if wav.has_partial_frame() {
        writeln!(s, "  ! data chunk ends mid-frame — trailing bytes ignored")?;
    }
    if size_field_error != 0 {
        // Advisory field, ignored when walking chunks — but it says something
        // about which tool wrote the file, so it is worth showing.
        writeln!(
            s,
            "  ! RIFF size field is {} bytes {} the real file — ignored",
            size_field_error.abs(),
            if size_field_error > 0 { "beyond" } else { "short of" }
        )?;
    }
    if !format.block_align_is_consistent() {
        writeln!(
            s,
            "  ! block align {} disagrees with {} bytes per frame — computed value used",
            format.block_align,
            format.frame_size()
        )?;
    }

    // One decode serves the peak and the tail measurement.
    let decoded = if decode { Some(wav.decode()?) } else { None };
    if let Some(buf) = &decoded {
        let peak = buf.peak();
        writeln!(s, "  peak         {peak:.6}  {}", dbfs(peak))?;

        if let Some(columns) = waveform {
            writeln!(s)?;
            write!(s, "{}", draw_waveform(buf, columns.clamp(8, 400)))?;
        }
    }

    let tags = wav.tags();
    if tags.acid.is_none() && tags.smpl.is_none() && tags.info.is_empty() {
        writeln!(s, "  tags         none (no acid / smpl / INFO)")?;
    }
    if let Some(acid) = tags.acid {
        if acid.is_one_shot() {
            writeln!(s, "  acid         one-shot, no tempo")?;
        } else {
            writeln!(
                s,
                "  acid         {} BPM, {} beats, {}/{}",
                acid.tempo, acid.beats, acid.meter_numerator, acid.meter_denominator
            )?;
        }
    }
    if let Some(smpl) = &tags.smpl {
        writeln!(
            s,
            "  smpl         unity note {}, {} loop(s)",
            smpl.midi_unity_note,
            smpl.loops.len()
        )?;
        for l in &smpl.loops {
            // `frame_count` reads the end point inclusively per the spec but
            // tolerates encoders that meant it exclusively.
            writeln!(
                s,
                "               [{}, {}] = {} frames",
                l.start,
                l.end,
                l.frame_count(wav.frames() as u32)
            )?;
        }
    }
    for (id, text) in &tags.info {
        writeln!(s, "  {}         {}", String::from_utf8_lossy(id), text)?;
    }

    // Measure the file against a tempo, if there is one to measure against.
    let declared = tags
        .declared_tempo()
        .and_then(|t| format!("{t}").parse::<Tempo>().ok());
    match bpm.or(declared) {
        Some(tempo) => {
            let source = if bpm.is_some() {
                "given"
            } else {
                "from acid chunk"
            };
            let grid = Grid::new(tempo.with_unit(bpm_unit), sig, format.sample_rate);
            let spb = grid.samples_per_bar().to_f64();
            writeln!(s)?;
            writeln!(s, "  at {tempo}, {sig} ({source}):")?;
            writeln!(
                s,
                "    bar length   {:.6} s = {spb:.4} samples",
                grid.seconds_per_bar().to_f64()
            )?;
            writeln!(s, "    file spans   {:.4} bars", wav.frames() as f64 / spb)?;

            if let Some(buf) = &decoded {
                let tail = Tail::measure_default(buf);
                // `audible_end` is one past the last audible frame; the report
                // names the frame itself.
                writeln!(
                    s,
                    "    above -60dB  {:.4} bars, last at frame {} ({})",
                    tail.audible_end as f64 / spb,
                    tail.audible_end.saturating_sub(1),
                    timecode(tail.audible_end.saturating_sub(1) as u64, format.sample_rate)
                )?;
                writeln!(
                    s,
                    "    silent end   {:.4} bars = {} frames below {} dBFS",
                    tail.trailing_frames as f64 / spb,
                    tail.trailing_frames,
                    tail.threshold_dbfs
                )?;

                let guess = WorkflowGuess::detect(
                    &grid,
                    wav.frames(),
                    tail.audible_end,
                    loop_bars,
                );
                writeln!(s)?;
                writeln!(s, "  for a {loop_bars}-bar loop:")?;
                match guess.workflow {
                    Workflow::WarmupRender => writeln!(
                        s,
                        "    path A — warmup render. Skip {} bars, keep {}, discard the tail.",
                        guess.skip_bars, loop_bars
                    )?,
                    Workflow::TailFoldback => writeln!(
                        s,
                        "    path B — one loop plus tail. Skip 0 bars, keep {loop_bars}, fold the tail back."
                    )?,
                    Workflow::AlreadyTrimmed => writeln!(
                        s,
                        "    already trimmed — exactly {loop_bars} bars, no tail. Nothing to do."
                    )?,
                    Workflow::Unclear => writeln!(
                        s,
                        "    unclear — {:.3} loop lengths of audible material. Defaulting to a straight cut.",
                        guess.audible_bars / loop_bars as f64
                    )?,
                }
                let region = grid.region(guess.skip_bars, loop_bars, Align::Loop);
                writeln!(
                    s,
                    "    cut          {} .. {}  ({} .. {})",
                    region.start,
                    region.end,
                    timecode(region.start, format.sample_rate),
                    timecode(region.end, format.sample_rate)
                )?;
                if region.end as usize > wav.frames() {
                    writeln!(
                        s,
                        "    ! the region runs {} frames past the end of the file",
                        region.end as usize - wav.frames()
                    )?;
                }
            }
        }
        None => {
            writeln!(s)?;
            writeln!(
                s,
                "  no tempo known — pass --bpm to measure the file against a bar grid"
            )?;
        }
    }

    Ok(s)
}

fn render_grid(grid: &Grid, skip: u64, bars: u64, align: Align, window: u32) -> String {
    let mut s = String::new();
    let region = grid.region(skip, bars, align);
    let spb = grid.samples_per_bar();

    let _ = writeln!(s, "{}  {}  {} Hz", grid.tempo, grid.sig, grid.sample_rate);
    let _ = writeln!(
        s,
        "  bar length   {:.6} s   {:.4} samples ({})",
        grid.seconds_per_bar().to_f64(),
        spb.to_f64(),
        spb
    );
    let _ = writeln!(s, "  align        {align}");
    let _ = writeln!(s);

    for (label, samples) in [
        (format!("skip {skip} bar(s)  in"), region.start),
        (format!("keep {bars} bar(s)  out"), region.end),
        ("length".to_string(), region.len()),
    ] {
        let _ = writeln!(
            s,
            "  {label:<20}{samples:>10}  {}",
            timecode(samples, grid.sample_rate)
        );
    }

    let _ = writeln!(s);
    let _ = writeln!(s, "  cut-in residual   {}", grid.residual(skip));
    let _ = writeln!(
        s,
        "  length residual   {}",
        grid.length_residual(skip, bars, align)
    );

    if grid.is_sample_exact(bars) {
        let _ = writeln!(s, "\n  {bars} bars are sample-exact at this tempo.");
    } else {
        let exact = grid.sample_exact_bpms(bars, window);
        let bpm = grid.tempo.value();
        let below = exact
            .iter()
            .rev()
            .find(|&&b| Tempo::bpm(b).is_ok_and(|t| t.value() < bpm));
        let above = exact
            .iter()
            .find(|&&b| Tempo::bpm(b).is_ok_and(|t| t.value() > bpm));
        let _ = write!(s, "\n  not sample-exact for {bars} bars.");
        let _ = match (below, above) {
            (Some(lo), Some(hi)) => writeln!(s, " Nearest exact tempos: {lo} / {hi}"),
            (Some(lo), None) => writeln!(s, " Nearest exact tempo below: {lo}"),
            (None, Some(hi)) => writeln!(s, " Nearest exact tempo above: {hi}"),
            (None, None) => writeln!(s, " None within ±{window} BPM."),
        };
    }
    s
}

/// A waveform as text, one line per channel, min/max mapped to a ramp of marks.
///
/// Uses the same min/max buckets the Android display will: a one-frame transient
/// stays visible at any width, where averaging would hide it at some zooms and
/// not others.
fn draw_waveform(buffer: &loopslcr_core::AudioBuffer, columns: usize) -> String {
    // Coarse to fine. A bucket that holds any signal at all must not render as
    // a blank, or the display would claim silence where there is none.
    const MARKS: [char; 8] = [' ', '.', ':', '-', '=', '+', '*', '#'];

    let peaks = Peaks::measure(buffer, columns);
    let mut out = String::new();
    for (channel, buckets) in peaks.channels.iter().enumerate() {
        let line: String = buckets
            .iter()
            .map(|b| {
                let m = b.magnitude();
                if m <= 0.0 {
                    MARKS[0]
                } else {
                    // Log scale over 60 dB: linear would leave every tail
                    // looking like silence.
                    #[allow(clippy::float_arithmetic)]
                    let level = (1.0 + m.log10() / 3.0).clamp(0.0, 1.0);
                    #[allow(clippy::float_arithmetic)]
                    let index = 1 + (level * (MARKS.len() - 2) as f64).round() as usize;
                    MARKS[index.min(MARKS.len() - 1)]
                }
            })
            .collect();
        let _ = writeln!(out, "  {} |{line}|", ["L", "R"].get(channel).unwrap_or(&"·"));
    }
    let _ = writeln!(
        out,
        "     {} frames per column, log scale over 60 dB",
        peaks.frames_per_bucket
    );
    out
}

fn timecode(samples: u64, sample_rate: u32) -> String {
    let total_ms = samples as f64 / sample_rate as f64 * 1000.0;
    let minutes = (total_ms / 60_000.0).floor();
    let seconds = total_ms / 1000.0 - minutes * 60.0;
    format!("{minutes:.0}:{seconds:06.3}")
}

fn dbfs(peak: f64) -> String {
    if peak <= 0.0 {
        "(silent)".to_string()
    } else {
        format!("({:+.2} dBFS)", 20.0 * peak.log10())
    }
}
