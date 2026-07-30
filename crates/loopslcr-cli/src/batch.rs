//! `loopslcr batch` — a directory at a time.
//!
//! # Carrying on is the whole point
//!
//! The archive this was built against holds 279 entries, and 18 of them cannot
//! be cut: 14 declare no tempo anywhere, 2 are zip files, 2 are shorter than a
//! bar. A batch that stopped at the first of those would never reach the other
//! 261. So every file is attempted, every failure is kept with its own reason,
//! and the reasons are printed together at the end where they can be acted on.
//!
//! The exit code is still non-zero when anything failed. A half-finished batch
//! that reports success is a trap for whatever script called it.
//!
//! # Skipped is not failed
//!
//! A zip file in a folder of drum loops is not an error, it is not a drum loop.
//! Anything that is not a RIFF/WAVE file at all is skipped and counted
//! separately, so the failure list stays a list of things worth fixing. The test
//! is the file's first twelve bytes, not its extension — two of the archive's
//! WAVE files have no `.wav` on them.
//!
//! # Determinism
//!
//! Work runs on all cores, but the report is assembled in sorted path order, so
//! two runs over the same directory print the same bytes in the same sequence.
//! Progress goes to stderr as files complete, in whatever order they finish;
//! that is the one thing here that is allowed to be unordered, because it is
//! ephemeral rather than a result.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use crate::{run_cut, Out, Report, Settings};

pub struct BatchArgs {
    pub dir: PathBuf,
    pub out_dir: Option<PathBuf>,
    pub recursive: bool,
    pub jobs: Option<usize>,
    pub verbose: bool,
    pub settings: Settings,
}

/// One file's outcome.
enum Outcome {
    Cut(crate::Cut),
    /// Not a WAVE file. Carries no reason because there is nothing to fix.
    Skipped,
    Failed(String),
}

pub fn run(args: BatchArgs) -> Result<Report, Box<dyn Error>> {
    let files = collect(&args.dir, args.recursive)?;
    if files.is_empty() {
        return Err(format!("{}: no files found", args.dir.display()).into());
    }

    let done = AtomicUsize::new(0);
    let total = files.len();
    let results: Vec<(PathBuf, Outcome)> = pool(args.jobs)?.install(|| {
        files
            .par_iter()
            .map(|file| {
                let outcome = process(file, &args);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                eprintln!("[{n:>4}/{total}] {}", name_of(file));
                (file.clone(), outcome)
            })
            .collect()
    });

    Ok(render(&args, results, total))
}

/// A thread pool of the requested size, or the default one per core.
///
/// A local pool rather than the global one: `--jobs` belongs to this batch, and
/// configuring the global pool would make the first call win for the process.
fn pool(jobs: Option<usize>) -> Result<rayon::ThreadPool, Box<dyn Error>> {
    let mut builder = rayon::ThreadPoolBuilder::new();
    if let Some(jobs) = jobs {
        builder = builder.num_threads(jobs);
    }
    Ok(builder.build()?)
}

/// Cuts one file, or says why it could not be.
fn process(file: &Path, args: &BatchArgs) -> Outcome {
    if !is_wave(file) {
        return Outcome::Skipped;
    }
    // The tree below the source directory is recreated under `--out-dir`, so two
    // loops of the same name in different folders stay two files. Flattening
    // would silently drop one of them, or refuse the second for existing.
    let target = match &args.out_dir {
        Some(root) => match mirror(root, &args.dir, file, !args.settings.dry_run) {
            Ok(dir) => Some(dir),
            Err(e) => return Outcome::Failed(e),
        },
        None => None,
    };
    let out = match &target {
        Some(dir) => Out::Dir(dir),
        None => Out::BesideSource,
    };
    match run_cut(file, out, &args.settings) {
        Ok(report) => Outcome::Cut(report),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// The directory under `root` that mirrors where `file` sits under `dir`.
///
/// Created if it does not exist. A dry run creates nothing, because a run that
/// writes no audio should not leave a tree of empty folders behind either.
fn mirror(root: &Path, dir: &Path, file: &Path, create: bool) -> Result<PathBuf, String> {
    let relative = file
        .parent()
        .and_then(|p| p.strip_prefix(dir).ok())
        .unwrap_or(Path::new(""));
    let target = root.join(relative);
    if create && !target.is_dir() {
        std::fs::create_dir_all(&target)
            .map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(target)
}

/// True when the file starts `RIFF....WAVE`.
///
/// Twelve bytes rather than an extension check: two of the archive's WAVE files
/// carry no extension, and a `.wav` on something that is not a WAVE file would
/// be a failure worth reporting rather than a silent skip.
fn is_wave(file: &Path) -> bool {
    use std::io::Read;
    let Ok(mut handle) = std::fs::File::open(file) else {
        // Unreadable is a real failure, so let the cut report it properly.
        return true;
    };
    let mut head = [0u8; 12];
    match handle.read_exact(&mut head) {
        Ok(()) => &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE",
        // Shorter than a header: a broken WAVE file, not a foreign one.
        Err(_) => true,
    }
}

/// Every regular file under `dir`, sorted.
///
/// Sorted here rather than at print time so the work order matches the report
/// order too — it makes the progress lines readable on a single-threaded run.
fn collect(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|e| format!("{}: {e}", current.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| format!("{}: {e}", current.display()))?.path();
            if path.is_dir() {
                if recursive {
                    stack.push(path);
                }
            } else if !is_preset(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The preset file is configuration, not material.
fn is_preset(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == crate::preset::FILE_NAME)
}

fn name_of(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy()
}

/// Assembles the report in sorted order.
fn render(args: &BatchArgs, results: Vec<(PathBuf, Outcome)>, total: usize) -> Report {
    use std::fmt::Write as _;

    let mut text = String::new();
    let mut cut = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(PathBuf, String)> = Vec::new();

    let _ = writeln!(
        text,
        "{} — {total} files{}",
        args.dir.display(),
        if args.settings.dry_run { ", dry run" } else { "" }
    );

    for (path, outcome) in results {
        match outcome {
            Outcome::Cut(report) => {
                cut += 1;
                if args.verbose {
                    let _ = writeln!(text, "\n{}", report.text);
                } else {
                    let _ = writeln!(text, "  ok      {} — {}", name_of(&path), report.summary);
                }
            }
            Outcome::Skipped => skipped += 1,
            Outcome::Failed(reason) => failures.push((path, reason)),
        }
    }

    let _ = writeln!(
        text,
        "\n  {cut} cut, {} failed, {skipped} not WAVE files",
        failures.len()
    );

    if !failures.is_empty() {
        let _ = writeln!(text, "\nfailed:");
        for (_, reason) in &failures {
            // The reason already begins with the path, as every error from the
            // cut does, so print it as it stands rather than naming the file
            // twice.
            let _ = writeln!(text, "  {reason}");
        }
    }

    Report {
        text,
        failed: !failures.is_empty(),
    }
}
