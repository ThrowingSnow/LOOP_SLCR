//! LOOP_SLCR command line interface.
//!
//! Only the timing math exists so far, so only `grid` does. It is the part of
//! `--dry-run` that needs no audio: given a tempo and a meter, where exactly
//! does the cut land, and how far off the grid is it.

use clap::{Parser, Subcommand, ValueEnum};
use loopslcr_core::timing::{Align, BpmUnit, Grid, Tempo, TimeSignature};

#[derive(Parser)]
#[command(name = "loopslcr", version, about = "Sample-exact loop trimming")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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

        /// Sample rate in Hz. Read from the file header once WAV I/O exists.
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

fn main() {
    let cli = Cli::parse();
    match cli.command {
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
            print_grid(&grid, skip, bars, align.into(), window);
        }
    }
}

fn print_grid(grid: &Grid, skip: u64, bars: u64, align: Align, window: u32) {
    let region = grid.region(skip, bars, align);
    let spb = grid.samples_per_bar();

    println!("{}  {}  {} Hz", grid.tempo, grid.sig, grid.sample_rate);
    println!(
        "  bar length   {:.6} s   {:.4} samples ({})",
        grid.seconds_per_bar().to_f64(),
        spb.to_f64(),
        spb
    );
    println!("  align        {align}");
    println!();
    for (label, samples) in [
        (format!("skip {skip} bar(s)  in"), region.start),
        (format!("keep {bars} bar(s)  out"), region.end),
        ("length".to_string(), region.len()),
    ] {
        println!(
            "  {label:<20}{samples:>10}  {}",
            timecode(samples, grid.sample_rate)
        );
    }
    println!();
    println!("  cut-in residual   {}", grid.residual(skip));
    println!(
        "  length residual   {}",
        grid.length_residual(skip, bars, align)
    );

    if grid.is_sample_exact(bars) {
        println!("\n  {bars} bars are sample-exact at this tempo.");
    } else {
        let exact = grid.sample_exact_bpms(bars, window);
        let bpm = grid.tempo.value();
        let below = exact.iter().rev().find(|&&b| Tempo::bpm(b).is_ok_and(|t| t.value() < bpm));
        let above = exact.iter().find(|&&b| Tempo::bpm(b).is_ok_and(|t| t.value() > bpm));
        print!("\n  not sample-exact for {bars} bars.");
        match (below, above) {
            (Some(lo), Some(hi)) => println!(" Nearest exact tempos: {lo} / {hi}"),
            (Some(lo), None) => println!(" Nearest exact tempo below: {lo}"),
            (None, Some(hi)) => println!(" Nearest exact tempo above: {hi}"),
            (None, None) => println!(" None within ±{window} BPM."),
        }
    }
}

fn timecode(samples: u64, sample_rate: u32) -> String {
    let total_ms = samples as f64 / sample_rate as f64 * 1000.0;
    let minutes = (total_ms / 60_000.0).floor();
    let seconds = total_ms / 1000.0 - minutes * 60.0;
    format!("{minutes:.0}:{seconds:06.3}")
}
