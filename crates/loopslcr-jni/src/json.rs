//! A flat JSON object, written and read.
//!
//! # Why a format at all, and why this one
//!
//! The call surface has to carry twenty-odd parameters in and a wide analysis
//! result out. The alternatives were worse: a fixed-layout `double[]` is
//! positional, so adding a field in the middle silently reinterprets every field
//! after it, and constructing Java objects from Rust couples the native code to
//! class names and constructor signatures that only fail at run time, on a
//! device, in a stack trace.
//!
//! JSON is self-describing, so a field the other side does not know about is
//! ignored rather than misread, and both directions are readable in a test.
//! Analysis happens once per file, so nothing here is on a hot path.
//!
//! # Why hand-written
//!
//! Because this is a *flat* object of numbers, booleans and a handful of fixed
//! keywords — no nesting, no arrays, no user-supplied strings. That is a small
//! enough grammar to implement completely, which is the only condition under
//! which hand-rolling a standard format is honest. String *values* are escaped
//! properly all the same, since a filename could reach one later.
//!
//! This lives in the JNI crate rather than the core: the core has no wire
//! format and should not grow one.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Builds a flat JSON object.
///
/// `BTreeMap` rather than insertion order, so the same values always produce the
/// same bytes — the reproducibility invariant applies to what crosses the
/// boundary too, and a test that compares output is worth having.
#[derive(Default, Debug)]
pub struct Object(BTreeMap<String, String>);

impl Object {
    pub fn new() -> Self {
        Object::default()
    }

    pub fn number(&mut self, key: &str, value: f64) -> &mut Self {
        // Non-finite has no JSON spelling. `null` is the honest answer, and it
        // reads back as absent rather than as zero, which a NaN silently would.
        let text = if value.is_finite() {
            format_number(value)
        } else {
            "null".to_string()
        };
        self.0.insert(key.to_string(), text);
        self
    }

    pub fn integer(&mut self, key: &str, value: i64) -> &mut Self {
        self.0.insert(key.to_string(), value.to_string());
        self
    }

    pub fn bool(&mut self, key: &str, value: bool) -> &mut Self {
        self.0.insert(key.to_string(), value.to_string());
        self
    }

    pub fn string(&mut self, key: &str, value: &str) -> &mut Self {
        self.0.insert(key.to_string(), quote(value));
        self
    }

    /// A value that may be absent. `None` becomes `null`, not a zero.
    pub fn maybe_number(&mut self, key: &str, value: Option<f64>) -> &mut Self {
        match value {
            Some(v) => self.number(key, v),
            None => {
                self.0.insert(key.to_string(), "null".to_string());
                self
            }
        }
    }

    pub fn maybe_integer(&mut self, key: &str, value: Option<i64>) -> &mut Self {
        match value {
            Some(v) => self.integer(key, v),
            None => {
                self.0.insert(key.to_string(), "null".to_string());
                self
            }
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::from("{");
        for (i, (key, value)) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}:{value}", quote(key));
        }
        out.push('}');
        out
    }
}

/// `f64` in a form JSON accepts and Rust reads back identically.
///
/// `{:?}` rather than `{}`: the `Debug` formatting of an `f64` is the shortest
/// decimal that round-trips, so a value written here and parsed there is the
/// same bits. `{}` would print `1` for `1.0`, which is still valid JSON but
/// loses the type on a reader that distinguishes.
fn format_number(value: f64) -> String {
    format!("{value:?}")
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Everything below 0x20 must be escaped; JSON is UTF-8 above it.
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// One value read out of a flat object.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Bool(bool),
    String(String),
    Null,
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

/// Parses a flat JSON object.
///
/// Flat is the whole contract: a nested object or an array is an error rather
/// than something quietly skipped, because silently ignoring a parameter the
/// caller meant is exactly the failure this format exists to avoid.
pub fn parse(text: &str) -> Result<BTreeMap<String, Value>, String> {
    let mut chars = text.char_indices().peekable();
    let mut out = BTreeMap::new();

    skip_space(&mut chars);
    match chars.next() {
        Some((_, '{')) => {}
        _ => return Err("expected an object".to_string()),
    }

    // A trailing comma is a syntax error, not a kindness. The justification for
    // hand-rolling a standard format is implementing it completely; accepting
    // what JSON forbids would make this "JSON plus", which is the drift the
    // format was chosen to avoid.
    let mut after_comma = false;
    loop {
        skip_space(&mut chars);
        match chars.peek() {
            Some((_, '}')) if after_comma => return Err("trailing comma".to_string()),
            Some((_, '}')) => {
                chars.next();
                break;
            }
            None => return Err("unterminated object".to_string()),
            _ => {}
        }

        let key = parse_string(&mut chars)?;
        skip_space(&mut chars);
        match chars.next() {
            Some((_, ':')) => {}
            _ => return Err(format!("expected ':' after {key:?}")),
        }
        skip_space(&mut chars);
        let value = parse_value(text, &mut chars, &key)?;
        out.insert(key, value);

        skip_space(&mut chars);
        match chars.next() {
            Some((_, ',')) => {
                after_comma = true;
                continue;
            }
            Some((_, '}')) => break,
            _ => return Err("expected ',' or '}'".to_string()),
        }
    }
    Ok(out)
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

fn skip_space(chars: &mut Chars<'_>) {
    while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
        chars.next();
    }
}

fn parse_string(chars: &mut Chars<'_>) -> Result<String, String> {
    match chars.next() {
        Some((_, '"')) => {}
        _ => return Err("expected a quoted string".to_string()),
    }
    let mut out = String::new();
    loop {
        match chars.next() {
            Some((_, '"')) => return Ok(out),
            Some((_, '\\')) => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 'r')) => out.push('\r'),
                Some((_, 't')) => out.push('\t'),
                Some((_, 'u')) => {
                    let hex: String = (0..4).filter_map(|_| chars.next().map(|(_, c)| c)).collect();
                    let code = u32::from_str_radix(&hex, 16)
                        .map_err(|_| format!("bad escape \\u{hex}"))?;
                    out.push(char::from_u32(code).ok_or_else(|| format!("bad escape \\u{hex}"))?);
                }
                Some((_, c)) => out.push(c),
                None => return Err("unterminated escape".to_string()),
            },
            Some((_, c)) => out.push(c),
            None => return Err("unterminated string".to_string()),
        }
    }
}

fn parse_value(text: &str, chars: &mut Chars<'_>, key: &str) -> Result<Value, String> {
    match chars.peek() {
        Some((_, '"')) => Ok(Value::String(parse_string(chars)?)),
        Some((_, '{')) | Some((_, '[')) => {
            Err(format!("{key:?}: nested values are not supported"))
        }
        Some(&(start, _)) => {
            // A bare token: a number, or one of the three keywords.
            let mut end = start;
            while let Some(&(i, c)) = chars.peek() {
                if c == ',' || c == '}' || c.is_whitespace() {
                    break;
                }
                end = i + c.len_utf8();
                chars.next();
            }
            match &text[start..end] {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                "null" => Ok(Value::Null),
                number => number
                    .parse::<f64>()
                    .map(Value::Number)
                    .map_err(|_| format!("{key:?}: {number:?} is not a number")),
            }
        }
        None => Err(format!("{key:?}: value is missing")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_round_trips() {
        let mut o = Object::new();
        o.integer("frames", 940_800)
            .number("tempo", 103.5)
            .bool("exact", true)
            .string("workflow", "warmup")
            .maybe_number("ratio", None)
            .maybe_integer("bars", Some(8));

        let text = o.render();
        let back = parse(&text).unwrap();
        assert_eq!(back["frames"].as_u64(), Some(940_800));
        assert_eq!(back["tempo"].as_f64(), Some(103.5));
        assert_eq!(back["exact"].as_bool(), Some(true));
        assert_eq!(back["workflow"].as_str(), Some("warmup"));
        assert_eq!(back["ratio"], Value::Null);
        assert_eq!(back["bars"].as_u64(), Some(8));
    }

    #[test]
    fn the_same_values_always_render_the_same_bytes() {
        // Sorted keys rather than insertion order: what crosses the boundary is
        // covered by the reproducibility invariant too, and a test that compares
        // whole payloads is worth being able to write.
        let build = |order: bool| {
            let mut o = Object::new();
            if order {
                o.integer("a", 1).integer("b", 2);
            } else {
                o.integer("b", 2).integer("a", 1);
            }
            o.render()
        };
        assert_eq!(build(true), build(false));
        assert_eq!(build(true), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn a_float_survives_the_round_trip_bit_for_bit() {
        // The reason for `{:?}`: the shortest decimal that reads back identically.
        for value in [0.1, 1.0 / 3.0, 103.5, 1e-17, 2f64.powi(-53), 1234567.891] {
            let mut o = Object::new();
            o.number("x", value);
            let back = parse(&o.render()).unwrap();
            assert_eq!(back["x"].as_f64(), Some(value), "{value} did not survive");
        }
    }

    #[test]
    fn a_non_finite_number_becomes_null_rather_than_a_lie() {
        // NaN has no JSON spelling. Writing 0 would be a plausible-looking
        // wrong answer on the other side.
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut o = Object::new();
            o.number("x", value);
            assert_eq!(o.render(), r#"{"x":null}"#);
        }
    }

    #[test]
    fn strings_are_escaped() {
        let mut o = Object::new();
        o.string("name", "a \"quoted\" \\ name\nwith\ttabs\u{1}");
        let text = o.render();
        let back = parse(&text).unwrap();
        assert_eq!(
            back["name"].as_str(),
            Some("a \"quoted\" \\ name\nwith\ttabs\u{1}")
        );
        assert!(text.contains("\\u0001"), "{text}");
    }

    #[test]
    fn a_nested_value_is_refused_rather_than_skipped() {
        // Quietly ignoring a parameter the caller meant is the failure this
        // format exists to avoid, so it must not be quiet about the one shape it
        // does not handle.
        let e = parse(r#"{"a":{"b":1}}"#).unwrap_err();
        assert!(e.contains("nested"), "{e}");
        let e = parse(r#"{"a":[1,2]}"#).unwrap_err();
        assert!(e.contains("nested"), "{e}");
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for bad in [
            "", "{", "}", "[]", r#"{"a"}"#, r#"{"a":}"#, r#"{"a":1,}"#, r#"{"a":x}"#,
            r#"{a:1}"#, r#"{"a":1"#, r#"{"unterminated"#,
        ] {
            assert!(parse(bad).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn whitespace_between_everything_is_fine() {
        let back = parse("  {  \"a\" : 1 ,\n \"b\" : true  }  ").unwrap();
        assert_eq!(back["a"].as_f64(), Some(1.0));
        assert_eq!(back["b"].as_bool(), Some(true));
    }

    #[test]
    fn an_empty_object_is_valid() {
        assert!(parse("{}").unwrap().is_empty());
        assert_eq!(Object::new().render(), "{}");
    }

    #[test]
    fn a_negative_or_fractional_value_is_not_read_as_a_count() {
        // `as_u64` is used for frame counts and bar numbers, where a negative
        // would wrap to something enormous through an `as` cast.
        assert_eq!(Value::Number(-1.0).as_u64(), None);
        assert_eq!(Value::Number(f64::NAN).as_u64(), None);
        assert_eq!(Value::Number(4.7).as_u64(), Some(4));
        assert_eq!(Value::Bool(true).as_u64(), None);
    }
}
