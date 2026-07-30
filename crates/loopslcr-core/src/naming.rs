//! Reading tempo and loop length out of a filename.
//!
//! For this archive the filename is the *primary* tempo source, not a fallback:
//! of 277 WAVE files, none carries an `acid` chunk, and 261 carry the tempo in
//! their name. So this parser has to handle what is actually there rather than
//! one tidy convention:
//!
//! ```text
//! 103 29Jul26 1Punkt1 Cstc.wav        leading, space-separated
//! 102-MTRX-01.wav                     leading, hyphen — no word break
//! 105CSTC-APRL02-ATM01-4BRS.wav       leading, running straight into letters
//! 00005 136BPM E01 01 comp 01 cut.wav a marker, not leading at all
//! 58.5 DL_4BAR_Lumiko Imai 01.wav     decimal — 58 names a different file
//! WIN10-INSTALL-01.wav                no tempo; must yield nothing
//! ```
//!
//! Plausibility is what does the disambiguating. `00005` is not a tempo and
//! `29Jul26` is a date, so a number only counts if it could be a tempo at all —
//! and an explicit `BPM` marker outranks position.

use crate::rational::Rational;
use crate::timing::Tempo;

/// The range a number has to fall in to be read as a tempo.
///
/// Wide enough for half-time dubstep at the bottom and drum & bass at the top;
/// narrow enough that dates (`29Jul26`), take numbers (`01`), bar counts
/// (`4BRS`) and Caustic's `00005` cannot be mistaken for one.
pub const PLAUSIBLE_BPM: std::ops::RangeInclusive<i128> = 40..=300;

/// The range a number has to fall in to be read as a bar count.
pub const PLAUSIBLE_BARS: std::ops::RangeInclusive<u64> = 1..=64;

/// One number found in a name, with the text that followed it.
struct Number<'a> {
    value: Rational,
    integer: i128,
    /// What comes directly after the digits, so a suffix like `BPM` or `BRS`
    /// can be recognised without scanning the name twice.
    rest: &'a str,
}

/// Every number in `name`, left to right, decimals included.
fn numbers(name: &str) -> Vec<Number<'_>> {
    let bytes = name.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let int_end = i;

        // A decimal point only continues the number if a digit follows it, so
        // `132-JAZZ.ORG` stops at 132 while `58.5` does not stop at 58.
        let mut frac_end = int_end;
        if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            frac_end = i;
        }

        let integer: i128 = match name[start..int_end].parse() {
            Ok(n) => n,
            // A digit run too long for `i128` is not a tempo.
            Err(_) => continue,
        };
        let value = if frac_end > int_end {
            let digits = &name[int_end + 1..frac_end];
            let scale = match 10i128.checked_pow(digits.len() as u32) {
                Some(s) => s,
                None => continue,
            };
            match digits.parse::<i128>() {
                Ok(frac) => Rational::new(integer * scale + frac, scale),
                Err(_) => continue,
            }
        } else {
            Rational::from_int(integer)
        };

        out.push(Number {
            value,
            integer,
            rest: &name[frac_end..],
        });
    }
    out
}

/// Strips one trailing extension, so `.wav` cannot be read as a decimal.
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        // Only if what follows looks like an extension rather than a decimal.
        Some(at) if name[at + 1..].chars().all(|c| c.is_ascii_alphanumeric()) => &name[..at],
        _ => name,
    }
}

/// Extensions an output filename may drop.
pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "wave", "aif", "aiff", "flac"];

/// The part of `name` to build an output filename from.
///
/// **Not** `Path::file_stem`, which strips whatever follows the last dot. The
/// archive holds `78-SMPL.BRN-21OCT23-01` and `78-SMPL.BRN-21OCT23-02` — two
/// WAVE files with no extension at all — and `file_stem` reduces both to
/// `78-SMPL`, so the second output overwrites the first. Found by running the
/// batch over the archive, which is exactly what that run is for.
///
/// So an extension is only dropped when it is one this program recognises as an
/// audio extension. Anything else is part of the name, because it is.
pub fn output_stem(name: &str) -> &str {
    if let Some(at) = name.rfind('.') {
        let extension = &name[at + 1..];
        if AUDIO_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
        {
            return &name[..at];
        }
    }
    name
}

/// Case-insensitive check for `marker` at the start of `s`, after any spaces,
/// underscores or hyphens.
fn starts_with_marker(s: &str, markers: &[&str]) -> bool {
    let s = s.trim_start_matches([' ', '_', '-']);
    markers
        .iter()
        .any(|m| s.len() >= m.len() && s[..m.len()].eq_ignore_ascii_case(m))
}

/// The tempo a filename declares, if any.
///
/// An explicit `BPM` marker wins wherever it sits. Otherwise the first number
/// that could plausibly be a tempo, which for this archive means the leading
/// one.
pub fn tempo_from_name(name: &str) -> Option<Tempo> {
    let name = stem(name);
    let found = numbers(name);

    // A marker outranks position: `00005 136BPM …` means 136, not 5.
    let marked = found
        .iter()
        .find(|n| starts_with_marker(n.rest, &["bpm"]) && PLAUSIBLE_BPM.contains(&n.integer));
    let pick = marked.or_else(|| {
        found
            .iter()
            .find(|n| PLAUSIBLE_BPM.contains(&n.integer) && !starts_with_marker(n.rest, &["bar", "brs"]))
    })?;

    Tempo::new(pick.value, crate::timing::BpmUnit::quarter()).ok()
}

/// The loop length a filename declares, if any — `4BRS`, `2BARS`, `8 bars`.
///
/// Worth reading because the archive's own convention says so: 4-bar loops
/// dominate it, and a default of 8 would be wrong for most of these files.
pub fn bars_from_name(name: &str) -> Option<u64> {
    let name = stem(name);
    numbers(name)
        .iter()
        .filter(|n| starts_with_marker(n.rest, &["bars", "bar", "brs"]))
        .filter_map(|n| u64::try_from(n.integer).ok())
        .find(|b| PLAUSIBLE_BARS.contains(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bpm(name: &str) -> Option<String> {
        tempo_from_name(name).map(|t| {
            let v = t.value();
            if v.is_integer() {
                v.num().to_string()
            } else {
                format!("{}", v)
            }
        })
    }

    /// Real names from the archive, each one a case that broke a simpler rule.
    #[test]
    fn reads_the_archives_actual_conventions() {
        for (name, expected) in [
            // The reference file: leading, space-separated.
            ("103 29Jul26 1Punkt1 Cstc.wav", Some("103")),
            // No word break after the number — `\b` would not have helped.
            ("102-MTRX-01.wav", Some("102")),
            ("105CSTC-APRL02-ATM01-4BRS.wav", Some("105")),
            ("110DL_BTWG-VST_01-2BARS.wav", Some("110")),
            ("132-JAZZ.ORG-01_4BRS.wav", Some("132")),
            ("106-cstc-08DEC24-ATMO-01.wav", Some("106")),
            ("120-APRL17 8BRS.wav", Some("120")),
            ("78-SMPL.BRN-01", Some("78")),
            // A marker rather than a leading number.
            ("00005 136BPM E01 01 comp 01 cut 01.wav", Some("136")),
            // Decimal: 58 would name a file that does not exist.
            ("58.5 DL_4BAR_Lumiko Imai 01.wav", Some("117/2")),
            // No tempo at all — must not invent one from a version number.
            ("WIN10-INSTALL-01.wav", None),
            ("untitled #20.wav", None),
            ("ACEVNTRA-01.wav", None),
            ("BLEEP-WIN10-01.wav", None),
            ("maLOUTE-GLYCN-01.wav", None),
        ] {
            assert_eq!(
                bpm(name).as_deref(),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn a_bpm_marker_outranks_position() {
        // Both plausible; the marked one wins.
        assert_eq!(bpm("120 something 90BPM.wav").as_deref(), Some("90"));
        // The marker is ignored when its number is not a plausible tempo, so a
        // stray "5BPM" cannot beat a real leading tempo.
        assert_eq!(bpm("103 weird 5BPM.wav").as_deref(), Some("103"));
    }

    #[test]
    fn a_bar_count_is_not_mistaken_for_a_tempo() {
        // 4 is implausible anyway, but 64 bars is inside the tempo range.
        assert_eq!(bpm("DL_64BARS_untitled.wav"), None);
        assert_eq!(bpm("DL_4BAR_Lumiko.wav"), None);
        // With a real tempo present, the bar count must not win.
        assert_eq!(bpm("110DL_64BARS.wav").as_deref(), Some("110"));
    }

    #[test]
    fn the_extension_is_not_a_decimal() {
        // `.5` would otherwise read as a fraction of the preceding number.
        assert_eq!(bpm("103.wav").as_deref(), Some("103"));
        assert_eq!(bpm("103.5.wav").as_deref(), Some("207/2"));
        // A name with no extension still works.
        assert_eq!(bpm("103 loop").as_deref(), Some("103"));
    }

    #[test]
    fn dates_and_take_numbers_are_rejected() {
        // 29Jul26: both numbers are outside the tempo range.
        assert_eq!(bpm("29Jul26 take 01.wav"), None);
        // 2024 is not a tempo either.
        assert_eq!(bpm("session 2024-03-01.wav"), None);
        // But a date after a real tempo does not disturb it.
        assert_eq!(bpm("105-DT-APRL19-23 01.wav").as_deref(), Some("105"));
    }

    #[test]
    fn reads_bar_counts() {
        for (name, expected) in [
            ("105CSTC-APRL02-ATM01-4BRS.wav", Some(4)),
            ("110DL_BTWG-VST_01-2BARS.wav", Some(2)),
            ("120-APRL17 8BRS.wav", Some(8)),
            ("58.5 DL_4BAR_Lumiko Imai 01.wav", Some(4)),
            ("132-JAZZ.ORG-01_4BRS.wav", Some(4)),
            // No marker: the leading tempo must not be read as a bar count.
            ("103 29Jul26 1Punkt1 Cstc.wav", None),
            ("102-MTRX-01.wav", None),
            // Implausible counts are refused.
            ("DL_0BARS.wav", None),
            ("DL_500BARS.wav", None),
        ] {
            assert_eq!(bars_from_name(name), expected, "{name}");
        }
    }

    #[test]
    fn empty_and_odd_input_is_handled() {
        assert_eq!(bpm(""), None);
        assert_eq!(bpm(".wav"), None);
        assert_eq!(bpm("....."), None);
        assert_eq!(bars_from_name(""), None);
        // A digit run far too long for any integer type must not panic.
        let huge = "9".repeat(200);
        assert_eq!(bpm(&format!("{huge}.wav")), None);
        assert_eq!(bpm(&format!("103 {huge}.wav")).as_deref(), Some("103"));
    }

    #[test]
    fn an_output_stem_only_drops_a_real_audio_extension() {
        assert_eq!(output_stem("103 Cstc.wav"), "103 Cstc");
        assert_eq!(output_stem("103 Cstc.WAV"), "103 Cstc");
        assert_eq!(output_stem("loop.aiff"), "loop");

        // The two archive files that exposed the bug: no extension, and a dot
        // in the middle of the name. `Path::file_stem` reduces both of these to
        // `78-SMPL`, which names two different loops the same file.
        assert_eq!(output_stem("78-SMPL.BRN-21OCT23-01"), "78-SMPL.BRN-21OCT23-01");
        assert_eq!(output_stem("78-SMPL.BRN-21OCT23-02"), "78-SMPL.BRN-21OCT23-02");
        assert_ne!(
            output_stem("78-SMPL.BRN-21OCT23-01"),
            output_stem("78-SMPL.BRN-21OCT23-02")
        );

        // A decimal tempo in the name is not an extension either.
        assert_eq!(output_stem("58.5 DL_4BAR"), "58.5 DL_4BAR");
        assert_eq!(output_stem("no dots here"), "no dots here");
    }
}
