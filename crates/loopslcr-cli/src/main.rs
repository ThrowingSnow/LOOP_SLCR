//! LOOP_SLCR command line interface.

use std::error::Error;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use loopslcr_core::timing::{Align, BpmUnit, Grid, Tempo, TimeSignature};
use loopslcr_core::analysis::{Tail, Workflow, WorkflowGuess};
use loopslcr_core::wav::{chunks::Chunks, SampleFormat, Wav};

#[derive(Parser)]
#[command(name = "loopslcr", version, about = "Sample-exact loop trimming")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

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

#[derive(Copy, Clone, Debug, ValueEnum)]
enum AlignArg {
    /// Cut-in on the grid, length exactly `bars` long.
    Loop,
    /// Both markers on the grid; length may differ by a sample.
    Grid,
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
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
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

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let text = match cli.command {
        Command::Info {
            file,
            bpm,
            sig,
            bpm_unit,
            bars,
            no_peak,
        } => {
            let bytes = std::fs::read(&file)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let wav = Wav::parse(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            let size_error = Chunks::size_field_error(&bytes).unwrap_or(0);
            render_info(&file, bytes.len(), size_error, &wav, bpm, sig, bpm_unit, bars, !no_peak)?
        }
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
            render_grid(&grid, skip, bars, align.into(), window)
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(text.as_bytes())?;
    out.flush()?;
    Ok(())
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
            // The smpl end point is inclusive, so the length is end - start + 1.
            writeln!(
                s,
                "               [{}, {}] = {} frames",
                l.start,
                l.end,
                l.end.saturating_sub(l.start) + 1
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
