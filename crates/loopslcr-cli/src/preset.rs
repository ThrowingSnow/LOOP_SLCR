//! Per-directory presets: `loopslcr.args`, holding the flags you would type.
//!
//! # Why not `loopslcr.toml`
//!
//! The roadmap said TOML, and TOML would have been a mistake. A `[cut]` table
//! with `bars = 8` is a **second source of truth for the flag set**: every flag
//! would need a key, every key a type, and the two lists would drift the first
//! time a flag was added without remembering the parser. Worse, the file would
//! quietly accept `bar = 8` or `bars = "8"` unless each key were validated
//! twice over.
//!
//! A file of flags has one grammar — clap's, the same one the command line
//! already uses. Nothing to keep in sync, no key that exists in one place and
//! not the other, and a flag added tomorrow works in a preset written today.
//!
//! # The format
//!
//! One flag per line. Everything after the first run of whitespace is the value,
//! taken verbatim to the end of the line, so a path with spaces in it needs no
//! quoting — which matters, since this archive is full of them. `#` at the start
//! of a line is a comment; blank lines are ignored.
//!
//! ```text
//! # 4-bar loops at 103, the way this folder was rendered
//! --bars 4
//! --bpm-from-name
//! --depth 16
//! --out-dir /mnt/loops/cut clean
//! ```
//!
//! # Precedence
//!
//! Preset flags are spliced in *before* the ones typed, so anything on the
//! command line wins. The one thing that cannot be overridden is a switch the
//! preset turns on — there is no `--no-tape`, and inventing negations for every
//! boolean to work around a config file would be the config file dictating the
//! interface. `--no-preset` ignores the file wholesale, which covers the case
//! honestly.

use std::error::Error;
use std::ffi::OsString;
use std::path::Path;

pub const FILE_NAME: &str = "loopslcr.args";

/// Reads the preset in `dir`, if there is one.
///
/// A missing file is not an error — most directories have none. A malformed one
/// is, because a preset that was written and then silently ignored would produce
/// files nobody asked for.
pub fn load(dir: &Path) -> Result<Option<Vec<OsString>>, Box<dyn Error>> {
    let path = dir.join(FILE_NAME);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let args = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(args))
}

/// Splits preset text into arguments.
fn parse(text: &str) -> Result<Vec<OsString>, String> {
    let mut args = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with('-') {
            return Err(format!(
                "line {}: {line:?} is not a flag — every line starts with - or --",
                number + 1
            ));
        }
        // Split once, on the first whitespace. The rest of the line is one
        // value, verbatim: no quoting rules to get wrong, and a path with
        // spaces just works.
        match line.split_once(char::is_whitespace) {
            Some((flag, value)) => {
                args.push(OsString::from(flag));
                args.push(OsString::from(value.trim()));
            }
            None => args.push(OsString::from(line)),
        }
    }
    Ok(args)
}

/// Rebuilds `argv` with `preset` spliced in after the subcommand name.
///
/// After the subcommand, so the preset's flags belong to it; before everything
/// else, so what was typed overrides what was written down.
pub fn splice(argv: &[OsString], preset: Vec<OsString>) -> Vec<OsString> {
    let mut out: Vec<OsString> = Vec::with_capacity(argv.len() + preset.len());
    let mut rest = argv.iter();
    // The program name and the subcommand.
    out.extend(rest.by_ref().take(2).cloned());
    out.extend(preset);
    out.extend(rest.cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Vec<String> {
        parse(text)
            .unwrap()
            .into_iter()
            .map(|s| s.into_string().unwrap())
            .collect()
    }

    #[test]
    fn a_flag_on_its_own_and_a_flag_with_a_value() {
        assert_eq!(
            parsed("--tape\n--bars 4\n"),
            vec!["--tape", "--bars", "4"]
        );
    }

    #[test]
    fn a_value_keeps_its_spaces() {
        // The reason for splitting once rather than tokenising: this archive
        // lives under a path with a space and an apostrophe in it, and a quoting
        // rule is one more thing to get wrong.
        assert_eq!(
            parsed("--out-dir /mnt/ALL STUFF OF ME/cut\n"),
            vec!["--out-dir", "/mnt/ALL STUFF OF ME/cut"]
        );
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text = "# 4-bar loops\n\n  --bars 4  \n\n# done\n";
        assert_eq!(parsed(text), vec!["--bars", "4"]);
    }

    #[test]
    fn an_equals_sign_is_left_to_clap() {
        // `--bars=4` is clap's own spelling, so it passes through untouched
        // rather than being re-split here into a second dialect.
        assert_eq!(parsed("--bars=4\n"), vec!["--bars=4"]);
    }

    #[test]
    fn a_line_that_is_not_a_flag_is_refused() {
        // The alternative would be to treat it as a positional argument, which
        // would let a stray word in a preset become an input filename.
        let e = parse("bars 4\n").unwrap_err();
        assert!(e.contains("line 1"), "{e}");
        assert!(e.contains("not a flag"), "{e}");
    }

    #[test]
    fn the_line_number_is_the_one_in_the_file() {
        // Counted over all lines, not over the ones that parsed, or the number
        // would point at the wrong line in any file with a comment.
        let e = parse("# a comment\n\n--bars 4\noops\n").unwrap_err();
        assert!(e.contains("line 4"), "{e}");
    }

    #[test]
    fn splicing_puts_the_preset_between_the_subcommand_and_the_rest() {
        let argv: Vec<OsString> = ["loopslcr", "cut", "x.wav", "--bars", "8"]
            .iter()
            .map(OsString::from)
            .collect();
        let preset: Vec<OsString> = ["--bars", "4", "--tape"].iter().map(OsString::from).collect();
        let out: Vec<String> = splice(&argv, preset)
            .into_iter()
            .map(|s| s.into_string().unwrap())
            .collect();
        assert_eq!(
            out,
            vec!["loopslcr", "cut", "--bars", "4", "--tape", "x.wav", "--bars", "8"]
        );
        // The typed --bars comes last, which is how it wins.
        assert!(out.iter().rposition(|a| a == "8") > out.iter().rposition(|a| a == "4"));
    }

    #[test]
    fn splicing_a_command_line_with_nothing_after_the_subcommand_still_works() {
        let argv: Vec<OsString> = ["loopslcr", "batch"].iter().map(OsString::from).collect();
        let preset: Vec<OsString> = vec![OsString::from("--tape")];
        let out: Vec<String> = splice(&argv, preset)
            .into_iter()
            .map(|s| s.into_string().unwrap())
            .collect();
        assert_eq!(out, vec!["loopslcr", "batch", "--tape"]);
    }
}
