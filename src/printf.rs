//! lcu-printf: dependency-free core for `printf`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.
//!
//! Single-pass core over an already-parsed FORMAT: leading `--`,
//! `--help`/`--version`, missing-operand errors, and `POSIXLY_CORRECT`
//! effects on *option parsing* belong to the CLI layer, which reads
//! `argv`/env that this pure function never touches. `Options` carries
//! only the one environment-dependent knob that affects computed
//! output diagnostics.
//!
//! Assumptions (C locale, documented where they bite):
//! - No locale reads: `%'` grouping is a no-op, multibyte handling
//!   assumes UTF-8 bytes, `%q` printability is pure bytes.
//! - `long double` narrows to `f64`: `%a` describes the `f64` value
//!   exactly (last-digit and subnormal forms may differ from GNU's
//!   80-bit rendering; dyadic rationals match).
//! - Absurd widths/precisions degrade gracefully instead of aborting;
//!   magnitudes up to `%500000s` work fully.
//! - Diagnostic texts are GNU C-locale message bodies WITHOUT the
//!   `prog: ` prefix; the CLI adds that.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// Options for `printf`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Suppress the "character(s) following character constant" warning,
    /// matching GNU under `POSIXLY_CORRECT`. Default false (warn).
    pub posixly_correct: bool,
}

/// Fatal errors: GNU aborts immediately with exit status 1.
/// Texts match GNU C-locale diagnostics; the CLI prepends `prog: `.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// `%#d`, `%5b`, `%$d`, stray `%`, ...: `{spec}` runs from `%`
    /// through the offending conversion character (raw format bytes).
    InvalidConversion { spec: OsString },
    /// `\x`, `\u`, `\U` without the required hex digits.
    MissingHexNumber,
    /// `\u`/`\U` naming a surrogate (`digits` holds the lowercase
    /// zero-padded value, e.g. `ud800`; always ASCII).
    InvalidUniversalChar { digits: String },
    /// `*` width outside `INT_MIN..=INT_MAX` (raw argument bytes).
    InvalidFieldWidth { arg: OsString },
    /// `*` precision above `INT_MAX` (raw argument bytes).
    InvalidPrecision { arg: OsString },
}

impl Error {
    /// The GNU C-locale diagnostic body for this error, byte-exact
    /// (diagnostics can carry non-UTF8 argument bytes).
    pub fn message(&self) -> OsString {
        match self {
            Error::InvalidConversion { spec } => {
                let mut m = spec.as_encoded_bytes().to_vec();
                m.extend_from_slice(b": invalid conversion specification");
                OsString::from_vec(m)
            }
            Error::MissingHexNumber => OsString::from("missing hexadecimal number in escape"),
            Error::InvalidUniversalChar { digits } => {
                OsString::from(format!("invalid universal character name \\{digits}"))
            }
            Error::InvalidFieldWidth { arg } => {
                let mut m = b"invalid field width: ".to_vec();
                m.extend_from_slice(&squote(arg.as_encoded_bytes()));
                OsString::from_vec(m)
            }
            Error::InvalidPrecision { arg } => {
                let mut m = b"invalid precision: ".to_vec();
                m.extend_from_slice(&squote(arg.as_encoded_bytes()));
                OsString::from_vec(m)
            }
        }
    }
}

/// Non-fatal diagnostics: GNU prints them, keeps going, and (except
/// for excess arguments) exits 1. Texts are message bodies; the CLI
/// prepends `prog: `. Arguments are raw bytes for byte-exact output.
#[derive(Debug, PartialEq, Eq)]
pub enum Warning {
    /// `%d no-num`: value used is 0.
    ExpectedNumericValue { arg: OsString },
    /// `%d 9z`: leading part converted (`9`), rest ignored.
    ValueNotCompletelyConverted { arg: OsString },
    /// Clamped `INTMAX`/`UINTMAX`/infinity results.
    OutOfRange { arg: OsString },
    /// `%d '"a"'`: trailing bytes after a character constant ignored.
    /// Raw (GNU does not quote it).
    CharConstTrailing { trailing: OsString },
    /// Format consumed no arguments while some remain. Exit stays 0.
    ExcessArguments { first: OsString },
}

impl Warning {
    /// The GNU C-locale diagnostic body for this warning, byte-exact.
    pub fn message(&self) -> OsString {
        match self {
            Warning::ExpectedNumericValue { arg } => {
                let mut m = squote(arg.as_encoded_bytes());
                m.extend_from_slice(b": expected a numeric value");
                OsString::from_vec(m)
            }
            Warning::ValueNotCompletelyConverted { arg } => {
                let mut m = squote(arg.as_encoded_bytes());
                m.extend_from_slice(b": value not completely converted");
                OsString::from_vec(m)
            }
            Warning::OutOfRange { arg } => {
                let mut m = squote(arg.as_encoded_bytes());
                m.extend_from_slice(b": Numerical result out of range");
                OsString::from_vec(m)
            }
            Warning::CharConstTrailing { trailing } => {
                let mut m = b"warning: ".to_vec();
                m.extend_from_slice(trailing.as_encoded_bytes());
                m.extend_from_slice(
                    b": character(s) following character constant have been ignored",
                );
                OsString::from_vec(m)
            }
            Warning::ExcessArguments { first } => {
                let mut m = b"warning: ignoring excess arguments, starting with ".to_vec();
                m.extend_from_slice(&squote(first.as_encoded_bytes()));
                OsString::from_vec(m)
            }
        }
    }

    /// Whether this warning flips the exit status to 1. Character
    /// constants and excess arguments warn but exit 0 (GNU sets the
    /// status only from numeric conversion failures).
    pub fn fails(&self) -> bool {
        matches!(
            self,
            Warning::ExpectedNumericValue { .. }
                | Warning::ValueNotCompletelyConverted { .. }
                | Warning::OutOfRange { .. }
        )
    }
}

/// A fatal error with everything emitted before it: GNU flushes
/// prior output even when a later directive aborts, so the caller
/// still prints `output.bytes`/`output.warnings` before the message.
#[derive(Debug, PartialEq, Eq)]
pub struct FatalError {
    pub error: Error,
    pub output: Output,
}

impl FatalError {
    /// The GNU C-locale diagnostic body for the fatal error.
    pub fn message(&self) -> OsString {
        self.error.message()
    }
}

/// The computed output: exact `bytes`, accumulated `warnings`, and the
/// exit status the CLI should use (1 on fatal-leaning warnings).
#[derive(Debug, PartialEq, Eq)]
pub struct Output {
    pub bytes: OsString,
    pub warnings: Vec<Warning>,
}

impl Output {
    /// 1 if any warning fails, else 0.
    pub fn exit_code(&self) -> i32 {
        if self.warnings.iter().any(Warning::fails) {
            1
        } else {
            0
        }
    }
}

/// Entry point for `printf`. Explicit params only, no implicit state.
///
/// Interprets `format` (backslash escapes plus `%` directives) against
/// `args`, reusing the format while arguments remain, exactly like GNU.
/// Never prints: the caller writes `output.bytes` and maps
/// `output.exit_code()` to its process status.
pub fn call(format: &OsStr, args: &[&OsStr], options: &Options) -> Result<Output, FatalError> {
    let fmt = format.as_bytes();
    let argv: Vec<&[u8]> = args.iter().map(|a| a.as_bytes()).collect();
    let mut st = Run {
        out: Vec::new(),
        warnings: Vec::new(),
        stopped: false,
        posixly: options.posixly_correct,
    };
    let mut rest = &argv[..];
    loop {
        let used = match st.pass(fmt, rest) {
            Ok(used) => used,
            Err(error) => {
                return Err(FatalError {
                    error,
                    output: st.finish(),
                });
            }
        };
        if st.stopped {
            break;
        }
        rest = &rest[used.min(rest.len())..];
        if used == 0 || rest.is_empty() {
            break;
        }
    }
    if !st.stopped && !rest.is_empty() {
        st.warnings.push(Warning::ExcessArguments {
            first: OsString::from_vec(rest[0].to_vec()),
        });
    }
    Ok(st.finish())
}

/// Upper bound on effective field widths and precisions. The real
/// binary formats the full requested width when asked (probed against
/// coreutils 9.10: `%20000000f` emits 20 MB); a library stays
/// deterministic instead. Every width pinned in the tests (up to
/// `%500000s`) is far below the cap and unaffected.
const PAD_CAP: u64 = 10_000_000;
const INT_MAX_I64: i64 = 2147483647;
const INT_MIN_I64: i64 = -2147483648;

struct Run {
    out: Vec<u8>,
    warnings: Vec<Warning>,
    stopped: bool,
    posixly: bool,
}

fn bump(end: &mut Option<usize>, a: usize) {
    *end = Some(end.map_or(a, |e| e.max(a)));
}

fn next_seq(seq: &mut usize, end: &mut Option<usize>) -> usize {
    let a = *seq;
    *seq = seq.saturating_add(1);
    bump(end, a);
    a
}

/// Positional slot for a `*` or conversion: an explicit index reuses
/// it, otherwise the next sequential slot is consumed. Either way the
/// high-water mark advances (even past the end of `args`).
fn take_slot(direc: Option<usize>, seq: &mut usize, end: &mut Option<usize>) -> usize {
    match direc {
        Some(i) => {
            bump(end, i);
            i
        }
        None => next_seq(seq, end),
    }
}

/// Parse `N$` at `pos`: 1-based index with `$`, value above zero.
/// Saturates, so absurd values behave as out-of-range (like INT_MAX).
fn parse_index(fmt: &[u8], pos: usize) -> Option<(usize, usize)> {
    if !fmt.get(pos).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut j = pos;
    let mut v: usize = 0;
    while j < fmt.len() && fmt[j].is_ascii_digit() {
        v = v
            .saturating_mul(10)
            .saturating_add((fmt[j] - b'0') as usize);
        j += 1;
    }
    if v > 0 && fmt.get(j) == Some(&b'$') {
        Some((v - 1, j + 1))
    } else {
        None
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// Parse one backslash escape starting after the `\` at `pos - 1`.
/// Returns emitted bytes, the index past the escape, and whether `\c`
/// stopped all output. `octal_0` selects `\0ooo` (`%b`) over `\ooo`.
fn parse_escape(bytes: &[u8], pos: usize, octal_0: bool) -> Result<(Vec<u8>, usize, bool), Error> {
    if pos >= bytes.len() {
        return Ok((vec![b'\\'], pos, false));
    }
    match bytes[pos] {
        b'a' => Ok((vec![0x07], pos + 1, false)),
        b'b' => Ok((vec![0x08], pos + 1, false)),
        b'e' => Ok((vec![0x1b], pos + 1, false)),
        b'f' => Ok((vec![0x0c], pos + 1, false)),
        b'n' => Ok((vec![0x0a], pos + 1, false)),
        b'r' => Ok((vec![0x0d], pos + 1, false)),
        b't' => Ok((vec![0x09], pos + 1, false)),
        b'v' => Ok((vec![0x0b], pos + 1, false)),
        b'\\' => Ok((vec![b'\\'], pos + 1, false)),
        b'"' => Ok((vec![b'"'], pos + 1, false)),
        b'\'' => Ok((vec![b'\''], pos + 1, false)),
        b'c' => Ok((Vec::new(), pos + 1, true)),
        b'x' => {
            let mut j = pos + 1;
            let mut val: u32 = 0;
            let mut n = 0;
            while n < 2 && j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                val = val * 16 + u32::from(hex_val(bytes[j]));
                j += 1;
                n += 1;
            }
            if n == 0 {
                return Err(Error::MissingHexNumber);
            }
            Ok((vec![(val & 0xff) as u8], j, false))
        }
        b'0'..=b'7' => {
            let mut j = pos;
            if octal_0 && bytes[j] == b'0' {
                j += 1;
            }
            let mut val: u32 = 0;
            let mut n = 0;
            while n < 3 && j < bytes.len() && (b'0'..=b'7').contains(&bytes[j]) {
                val = val * 8 + u32::from(bytes[j] - b'0');
                j += 1;
                n += 1;
            }
            Ok((vec![(val & 0xff) as u8], j, false))
        }
        b'u' | b'U' => {
            let letter = bytes[pos];
            let need = if letter == b'u' { 4 } else { 8 };
            if bytes.len() - pos - 1 < need
                || !bytes[pos + 1..pos + 1 + need]
                    .iter()
                    .all(|c| c.is_ascii_hexdigit())
            {
                return Err(Error::MissingHexNumber);
            }
            let mut value: u32 = 0;
            for &c in &bytes[pos + 1..pos + 1 + need] {
                value = value * 16 + u32::from(hex_val(c));
            }
            if (0xd800..=0xdfff).contains(&value) {
                let digits = if letter == b'u' {
                    format!("u{value:04x}")
                } else {
                    format!("U{value:08x}")
                };
                return Err(Error::InvalidUniversalChar { digits });
            }
            // Surrogates are errors above; any other value too large
            // for a code point re-emits the escape in canonical form
            // (probed against coreutils 9.10: `\U` + uppercase digits).
            if let Some(c) = char::from_u32(value) {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf).as_bytes().to_vec();
                Ok((bytes, pos + 1 + need, false))
            } else {
                Ok((
                    format!("\\{}{value:0>8X}", letter as char).into_bytes(),
                    pos + 1 + need,
                    false,
                ))
            }
        }
        other => Ok((vec![b'\\', other], pos + 1, false)),
    }
}

/// C-locale printable byte: the `%q` segment rule pivots on this.
fn is_print(c: u8) -> bool {
    (0x20..=0x7e).contains(&c)
}

/// Double-quote-safe byte for `%q`: needs no backslash escape inside
/// `"..."`. This is the mid-position bare set plus space; `$ " \ `` `
/// and friends still need escapes there, so their presence forces
/// segmented quoting instead.
fn dbl_safe(c: u8) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            b'-' | b'_' | b'.' | b'/' | b',' | b':' | b'+' | b'@' | b'%' | b']' | b' '
        )
}

/// Double-quote eligibility with position: `~` and `#` count only at
/// index 0 (probed: `~a'b`/`#a'b` double, `'~a`/`'=x`-style do not).
fn dbl_at(arg: &[u8], i: usize) -> bool {
    let c = arg[i];
    dbl_safe(c) || ((c == b'~' || c == b'#') && i == 0)
}

/// Bare-safe byte for `%q`, with the leading-position rule folded in:
/// `~` and `#` are safe only past the first byte; `=`, `^` and `!`
/// never are (pinned by probing).
fn bare_safe(c: u8, is_first: bool) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            b'-' | b'_' | b'.' | b'/' | b',' | b':' | b'+' | b'@' | b'%' | b'{' | b'}' | b']'
        )
        || (!is_first && matches!(c, b'~' | b'#'))
}

fn bare_ok(arg: &[u8]) -> bool {
    // Lone braces are quoted despite being safe elsewhere; the
    // rationale is unknown, the behavior is pinned by probing.
    if arg == b"{" || arg == b"}" {
        return false;
    }
    arg.iter()
        .enumerate()
        .all(|(i, &c)| is_print(c) && bare_safe(c, i == 0))
}

/// Quote `arg` for reuse as shell input (shell-escape style, C
/// locale, matching coreutils 9.10 output): bare safe words pass
/// through, lone `'`-only unsafety
/// takes `"..."`, otherwise single-quote runs and `$''` runs adjoin.
fn shell_quote(arg: &[u8]) -> Vec<u8> {
    if arg.is_empty() {
        return b"''".to_vec();
    }
    if bare_ok(arg) {
        return arg.to_vec();
    }
    // `"..."` iff printable, containing a quote, with everything else
    // double-quote-safe: `';'`/`'$'`/`'a"b'` segment instead because
    // `;`, `$`, `"` would need escapes inside doubles.
    let mut issue = false;
    let mut dbl = true;
    for (i, &c) in arg.iter().enumerate() {
        if !is_print(c) {
            dbl = false;
            break;
        }
        if c == b'\'' {
            issue = true;
        } else if !dbl_at(arg, i) {
            issue = false;
            dbl = false;
            break;
        }
    }
    if dbl && issue {
        let mut v = vec![b'"'];
        v.extend_from_slice(arg);
        v.push(b'"');
        return v;
    }
    enum Run<'a> {
        Quote,
        Bytes(&'a [u8]),
        Escaped(&'a [u8]),
    }
    // Quotes always stand alone; printable runs never contain them.
    // Rendering fuses quotes across run boundaries: a middle quote
    // with printable runs on both sides borrows both neighbors'
    // adjoining quotes (`'a'\'b$c'`), while every other quote pads
    // with an empty pair wherever the neighbor (or void) does not
    // already end or begin with one.
    let mut runs: Vec<Run> = Vec::new();
    let mut i = 0;
    while i < arg.len() {
        let c = arg[i];
        if c == b'\'' {
            runs.push(Run::Quote);
            i += 1;
        } else if !is_print(c) {
            let mut j = i + 1;
            while j < arg.len() && !is_print(arg[j]) {
                j += 1;
            }
            runs.push(Run::Escaped(&arg[i..j]));
            i = j;
        } else {
            let mut j = i + 1;
            while j < arg.len() && is_print(arg[j]) && arg[j] != b'\'' {
                j += 1;
            }
            runs.push(Run::Bytes(&arg[i..j]));
            i = j;
        }
    }
    // Quotes render as `\'`, padded with an empty pair at the edges
    // of the word and around `$''` runs, whose `$` cannot abut the
    // escape. A printable run after a non-first quote continues its
    // borrow by dropping its opening quote (`'a'\'b$c'`); after a
    // first quote it opens normally (`''\''a"b'`).
    let mut v = Vec::new();
    for (ri, run) in runs.iter().enumerate() {
        let last = ri == runs.len() - 1;
        match run {
            // Quotes render as `\'`. The leading pad is `''` at the
            // start of the word and `'` between two `$''` runs (whose
            // `$` cannot abut the escape); the trailing pad is `''`
            // at the end of the word and before a `$''` run.
            Run::Quote => {
                if ri == 0 {
                    v.extend_from_slice(b"''");
                }
                v.extend_from_slice(b"\\'");
                // Trailing pad unless a printable run follows that
                // opens its own quote.
                if last || !matches!(runs.get(ri + 1), Some(Run::Bytes(_))) {
                    v.extend_from_slice(b"''");
                }
            }
            Run::Bytes(bs) => {
                v.push(b'\'');
                v.extend_from_slice(bs);
                v.push(b'\'');
            }
            Run::Escaped(bs) => {
                if ri == 0 {
                    v.extend_from_slice(b"''");
                }
                v.extend_from_slice(b"$'");
                for &b in *bs {
                    match b {
                        0x07 => v.extend_from_slice(b"\\a"),
                        0x08 => v.extend_from_slice(b"\\b"),
                        0x0c => v.extend_from_slice(b"\\f"),
                        0x0a => v.extend_from_slice(b"\\n"),
                        0x0d => v.extend_from_slice(b"\\r"),
                        0x09 => v.extend_from_slice(b"\\t"),
                        0x0b => v.extend_from_slice(b"\\v"),
                        // NUL cannot arrive via argv; `\0` is the
                        // readable choice for API-supplied NUL bytes.
                        0x00 => v.extend_from_slice(b"\\0"),
                        _ => v.extend_from_slice(format!("\\{b:03o}").as_bytes()),
                    }
                }
                v.push(b'\'');
            }
        }
    }
    v
}

/// Decode the first UTF-8 character (character-constant values).
fn decode_first(s: &[u8]) -> (u32, usize) {
    if s.is_empty() {
        return (0, 0);
    }
    match std::str::from_utf8(s) {
        Ok(st) => {
            let c = st.chars().next().unwrap();
            (c as u32, c.len_utf8())
        }
        Err(e) => {
            let v = e.valid_up_to();
            if v == 0 {
                (u32::from(s[0]), 1)
            } else {
                let st = std::str::from_utf8(&s[..v]).unwrap();
                let c = st.chars().next().unwrap();
                (c as u32, c.len_utf8())
            }
        }
    }
}

enum ParsedInt {
    Char {
        value: u64,
        trailing: usize,
    },
    Num {
        neg: bool,
        mag: u128,
        end: usize,
        had: bool,
    },
}

/// Scan a base-0 integer with C2X `0b` support (probed against
/// coreutils 9.10) and `'"'`/`'\''` character constants. Mirrors
/// strtoimax/strtoumax consumption.
fn scan_int(s: &[u8]) -> ParsedInt {
    if s.len() > 1 && (s[0] == b'"' || s[0] == b'\'') && s[1] != 0 {
        let (value, len) = decode_first(&s[1..]);
        return ParsedInt::Char {
            value: u64::from(value),
            trailing: 1 + len,
        };
    }
    let mut j = 0;
    while j < s.len() && is_space(s[j]) {
        j += 1;
    }
    let mut neg = false;
    if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
        neg = s[j] == b'-';
        j += 1;
    }
    let mut mag: u128 = 0;
    let mut had = false;
    if j < s.len() && s[j] == b'0' {
        let n1 = *s.get(j + 1).unwrap_or(&0);
        let has_hex =
            (n1 == b'x' || n1 == b'X') && s.get(j + 2).is_some_and(|c| c.is_ascii_hexdigit());
        let has_bin = (n1 == b'b' || n1 == b'B')
            && matches!(s.get(j + 2), Some(c) if *c == b'0' || *c == b'1');
        if has_hex {
            j += 2;
            while j < s.len() && s[j].is_ascii_hexdigit() {
                mag = mag
                    .saturating_mul(16)
                    .saturating_add(u128::from(hex_val(s[j])));
                j += 1;
                had = true;
            }
        } else if has_bin {
            j += 2;
            while j < s.len() && (s[j] == b'0' || s[j] == b'1') {
                mag = mag
                    .saturating_mul(2)
                    .saturating_add(u128::from(s[j] - b'0'));
                j += 1;
                had = true;
            }
        } else {
            while j < s.len() && (b'0'..=b'7').contains(&s[j]) {
                mag = mag
                    .saturating_mul(8)
                    .saturating_add(u128::from(s[j] - b'0'));
                j += 1;
                had = true;
            }
        }
    } else {
        while j < s.len() && s[j].is_ascii_digit() {
            mag = mag
                .saturating_mul(10)
                .saturating_add(u128::from(s[j] - b'0'));
            j += 1;
            had = true;
        }
    }
    ParsedInt::Num {
        neg,
        mag,
        end: if had { j } else { 0 },
        had,
    }
}

enum INum {
    I(i64),
    U(u64),
}

fn match_ci(s: &[u8], k: usize, pat: &[u8]) -> bool {
    s.len() >= k + pat.len() && s[k..k + pat.len()].eq_ignore_ascii_case(pat)
}

/// Scan a C-locale double: inf/infinity, nan[(chars)], hex float, or
/// decimal. Returns value, end index, whether digits converted, and
/// whether errno (overflow/underflow) applies.
fn scan_float(s: &[u8]) -> (f64, usize, bool, bool) {
    let mut j = 0;
    while j < s.len() && is_space(s[j]) {
        j += 1;
    }
    let mut neg = false;
    if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
        neg = s[j] == b'-';
        j += 1;
    }
    let k = j;
    let apply = |v: f64| if neg { -v } else { v };
    if match_ci(s, k, b"infinity") {
        return (apply(f64::INFINITY), k + 8, true, false);
    }
    if match_ci(s, k, b"inf") {
        return (apply(f64::INFINITY), k + 3, true, false);
    }
    if match_ci(s, k, b"nan") {
        j = k + 3;
        if s.get(j) == Some(&b'(') {
            let mut t = j + 1;
            while t < s.len() && (s[t].is_ascii_alphanumeric() || s[t] == b'_') {
                t += 1;
            }
            if s.get(t) == Some(&b')') {
                j = t + 1;
            }
        }
        return (apply(f64::NAN), j, true, false);
    }
    if s.get(k) == Some(&b'0')
        && matches!(s.get(k + 1), Some(b'x' | b'X'))
        && s.get(k + 2).is_some_and(|c| c.is_ascii_hexdigit())
    {
        j = k + 2;
        let mut int_v: f64 = 0.0;
        while j < s.len() && s[j].is_ascii_hexdigit() {
            int_v = int_v * 16.0 + f64::from(hex_val(s[j]));
            j += 1;
        }
        let mut frac_v: f64 = 0.0;
        let mut scale: f64 = 1.0;
        if s.get(j) == Some(&b'.') {
            j += 1;
            while j < s.len() && s[j].is_ascii_hexdigit() {
                frac_v = frac_v * 16.0 + f64::from(hex_val(s[j]));
                scale *= 16.0;
                j += 1;
            }
        }
        let mut exp2: i64 = 0;
        if matches!(s.get(j), Some(b'p' | b'P')) {
            let mut t = j + 1;
            let mut esign = 1i64;
            if t < s.len() && (s[t] == b'+' || s[t] == b'-') {
                if s[t] == b'-' {
                    esign = -1;
                }
                t += 1;
            }
            let ds = t;
            let mut ev: i64 = 0;
            while t < s.len() && s[t].is_ascii_digit() {
                ev = ev.saturating_mul(10).saturating_add(i64::from(s[t] - b'0'));
                t += 1;
            }
            if t > ds {
                exp2 = esign * ev;
                j = t;
            }
        }
        let v = (int_v + frac_v / scale) * 2f64.powi(exp2.clamp(-2000000, 2000000) as i32);
        let nonzero = int_v != 0.0 || frac_v != 0.0;
        let errno = nonzero && (v.is_infinite() || v == 0.0);
        return (apply(v), j, true, errno);
    }
    // Decimal: longest valid prefix, then one correctly-rounded parse.
    let mut t = k;
    let mut digits = 0usize;
    let mut nonzero = false;
    while t < s.len() && s[t].is_ascii_digit() {
        if s[t] != b'0' {
            nonzero = true;
        }
        digits += 1;
        t += 1;
    }
    if s.get(t) == Some(&b'.') {
        t += 1;
        while t < s.len() && s[t].is_ascii_digit() {
            if s[t] != b'0' {
                nonzero = true;
            }
            digits += 1;
            t += 1;
        }
    }
    if digits == 0 {
        return (0.0, 0, false, false);
    }
    if matches!(s.get(t), Some(b'e' | b'E')) {
        let mut u = t + 1;
        if u < s.len() && (s[u] == b'+' || s[u] == b'-') {
            u += 1;
        }
        let ds = u;
        while u < s.len() && s[u].is_ascii_digit() {
            u += 1;
        }
        if u > ds {
            t = u;
        }
    }
    let v: f64 = std::str::from_utf8(&s[k..t])
        .ok()
        .and_then(|st| st.parse().ok())
        .unwrap_or(0.0);
    // errno iff a nonzero input overflows to infinity or underflows to
    // zero. Subnormal results do NOT warn: GNU parses with 80-bit
    // `long double` (normal down to ~3e-4932), so f64-subnormal inputs
    // convert cleanly there; warning on them would diverge. Inputs
    // below f64 range still warn via the zero rule. Documented gap:
    // values unrepresentable in f64 but normal in long double
    // (e.g. 5e-524) print tiny there and zero here.
    let errno = nonzero && (v.is_infinite() || v == 0.0);
    (apply(v), t, true, errno)
}

fn conv_allowed(conv: u8, alt: bool, zero: bool, quote: bool, has_prec: bool) -> bool {
    match conv {
        b'a' | b'A' | b'c' | b'd' | b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'i' | b'o'
        | b's' | b'u' | b'x' | b'X' => {
            let mut ok = true;
            if quote {
                ok &= matches!(conv, b'd' | b'f' | b'F' | b'g' | b'G' | b'i' | b'u');
            }
            if alt {
                ok &= !matches!(conv, b'c' | b'd' | b'i' | b's' | b'u');
            }
            if zero {
                ok &= !matches!(conv, b'c' | b's');
            }
            if has_prec {
                ok &= conv != b'c';
            }
            ok
        }
        _ => false,
    }
}

impl Run {
    fn finish(self) -> Output {
        Output {
            bytes: OsString::from_vec(self.out),
            warnings: self.warnings,
        }
    }

    fn warn_expected(&mut self, arg: &[u8]) {
        self.warnings.push(Warning::ExpectedNumericValue {
            arg: OsString::from_vec(arg.to_vec()),
        });
    }

    fn convert_int(&mut self, s: &[u8], signed: bool) -> INum {
        match scan_int(s) {
            ParsedInt::Char { value, trailing } => {
                if trailing < s.len() && !self.posixly {
                    self.warnings.push(Warning::CharConstTrailing {
                        trailing: OsString::from_vec(s[trailing..].to_vec()),
                    });
                }
                if signed {
                    INum::I(value.min(i64::MAX as u64) as i64)
                } else {
                    INum::U(value)
                }
            }
            ParsedInt::Num { neg, mag, end, had } => {
                if !had {
                    self.warn_expected(s);
                    return if signed { INum::I(0) } else { INum::U(0) };
                }
                if signed {
                    let (v, over) = if neg {
                        // `-2^63` is exactly representable; only larger
                        // magnitudes clamp (negating `mag as i64` at the
                        // boundary would overflow).
                        if mag == 1u128 << 63 {
                            (i64::MIN, false)
                        } else if mag > (1u128 << 63) {
                            (i64::MIN, true)
                        } else {
                            (-(mag as i64), false)
                        }
                    } else if mag > i64::MAX as u128 {
                        (i64::MAX, true)
                    } else {
                        (mag as i64, false)
                    };
                    if over {
                        self.warnings.push(Warning::OutOfRange {
                            arg: OsString::from_vec(s.to_vec()),
                        });
                    } else if end < s.len() {
                        self.warnings.push(Warning::ValueNotCompletelyConverted {
                            arg: OsString::from_vec(s.to_vec()),
                        });
                    }
                    INum::I(v)
                } else {
                    let over = mag > u64::MAX as u128;
                    let mut v = if over { u64::MAX } else { mag as u64 };
                    if over {
                        self.warnings.push(Warning::OutOfRange {
                            arg: OsString::from_vec(s.to_vec()),
                        });
                    } else if end < s.len() {
                        self.warnings.push(Warning::ValueNotCompletelyConverted {
                            arg: OsString::from_vec(s.to_vec()),
                        });
                    }
                    // Overflow clamps to UINTMAX_MAX without the
                    // negation (strtoumax's ERANGE result); in-range
                    // negatives wrap modulo 2^64 as usual.
                    if neg && !over {
                        v = v.wrapping_neg();
                    }
                    INum::U(v)
                }
            }
        }
    }

    fn convert_float(&mut self, s: &[u8]) -> f64 {
        let (v, end, had, errno) = scan_float(s);
        if !had {
            self.warn_expected(s);
            return 0.0;
        }
        if errno {
            self.warnings.push(Warning::OutOfRange {
                arg: OsString::from_vec(s.to_vec()),
            });
        } else if end < s.len() {
            self.warnings.push(Warning::ValueNotCompletelyConverted {
                arg: OsString::from_vec(s.to_vec()),
            });
        }
        v
    }

    fn emit_b(&mut self, arg: &[u8]) -> Result<(), Error> {
        let mut j = 0;
        while j < arg.len() {
            if arg[j] != b'\\' {
                self.out.push(arg[j]);
                j += 1;
                continue;
            }
            let (emit, np, stop) = parse_escape(arg, j + 1, true)?;
            self.out.extend_from_slice(&emit);
            if stop {
                self.stopped = true;
                return Ok(());
            }
            j = np;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn directive(
        &mut self,
        conv: u8,
        left: bool,
        plus: bool,
        space: bool,
        alt: bool,
        zero: bool,
        width: usize,
        prec: Option<u64>,
        arg: Option<&[u8]>,
    ) -> Result<(), Error> {
        let f = Fmt {
            left,
            plus,
            space,
            alt,
            zero,
            width: width.min(PAD_CAP as usize),
            prec: prec.map(|v| v.min(PAD_CAP)),
        };
        match conv {
            b'd' | b'i' => {
                let v = match arg {
                    Some(s) => match self.convert_int(s, true) {
                        INum::I(v) => v,
                        INum::U(v) => v as i64,
                    },
                    None => 0,
                };
                self.out.extend(fmt_int(v, 10, false, &f));
            }
            b'o' | b'u' | b'x' | b'X' => {
                let v = match arg {
                    Some(s) => match self.convert_int(s, false) {
                        INum::U(v) => v,
                        INum::I(v) => v as u64,
                    },
                    None => 0,
                };
                let base = if conv == b'o' {
                    8
                } else if conv == b'u' {
                    10
                } else {
                    16
                };
                self.out
                    .extend(fmt_uint(v, false, base, conv == b'X', false, &f));
            }
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                let v = match arg {
                    Some(s) => self.convert_float(s),
                    None => 0.0,
                };
                self.out.extend(fmt_float(v, conv, f.prec, &f));
            }
            b'c' => {
                let b = arg.map_or(0, |s| *s.first().unwrap_or(&0));
                self.out.extend(fmt_char(b, &f));
            }
            b's' => {
                let s = arg.unwrap_or(b"");
                self.out.extend(fmt_str(s, &f));
            }
            b'b' => {
                if let Some(s) = arg {
                    self.emit_b(s)?;
                }
            }
            b'q' => {
                if let Some(s) = arg {
                    self.out.extend(shell_quote(s));
                }
            }
            _ => unreachable!("conversion validated before directive"),
        }
        Ok(())
    }

    fn pass(&mut self, fmt: &[u8], args: &[&[u8]]) -> Result<usize, Error> {
        let mut seq: usize = 0;
        let mut end: Option<usize> = None;
        let mut direc: Option<usize>;
        let mut p = 0;
        while p < fmt.len() && !self.stopped {
            let c = fmt[p];
            if c == b'\\' {
                let (emit, np, stop) = parse_escape(fmt, p + 1, false)?;
                self.out.extend_from_slice(&emit);
                if stop {
                    self.stopped = true;
                    break;
                }
                p = np;
                continue;
            }
            if c != b'%' {
                self.out.push(c);
                p += 1;
                continue;
            }
            let start = p;
            p += 1;
            if fmt.get(p) == Some(&b'%') {
                self.out.push(b'%');
                p += 1;
                continue;
            }
            if let Some((i, np)) = parse_index(fmt, p) {
                direc = Some(i);
                p = np;
            } else {
                direc = None;
            }
            if matches!(fmt.get(p), Some(b'b') | Some(b'q')) {
                let conv = fmt[p];
                p += 1;
                let ai = take_slot(direc, &mut seq, &mut end);
                if ai < args.len() {
                    if conv == b'b' {
                        self.emit_b(args[ai])?;
                    } else {
                        self.out.extend(shell_quote(args[ai]));
                    }
                }
                continue;
            }
            let (mut left, mut plus, mut space, mut alt, mut zero, mut quote) =
                (false, false, false, false, false, false);
            loop {
                match fmt.get(p) {
                    Some(b'-') => left = true,
                    Some(b'+') => plus = true,
                    Some(b' ') => space = true,
                    Some(b'#') => alt = true,
                    Some(b'0') => zero = true,
                    Some(b'\'') => quote = true,
                    // Glibc-only flag; C-locale no-op, accepted.
                    Some(b'I') => {}
                    _ => break,
                }
                p += 1;
            }
            let mut width: i64 = 0;
            if fmt.get(p) == Some(&b'*') {
                p += 1;
                let wi = match parse_index(fmt, p) {
                    Some((i, np)) => {
                        p = np;
                        bump(&mut end, i);
                        i
                    }
                    None => next_seq(&mut seq, &mut end),
                };
                if wi < args.len() {
                    let w = match self.convert_int(args[wi], true) {
                        INum::I(v) => v,
                        INum::U(v) => v as i64,
                    };
                    if !(INT_MIN_I64..=INT_MAX_I64).contains(&w) {
                        return Err(Error::InvalidFieldWidth {
                            arg: OsString::from_vec(args[wi].to_vec()),
                        });
                    }
                    width = w;
                }
            } else {
                let mut w: u64 = 0;
                while p < fmt.len() && fmt[p].is_ascii_digit() {
                    w = w
                        .saturating_mul(10)
                        .saturating_add(u64::from(fmt[p] - b'0'));
                    p += 1;
                }
                width = w.min(PAD_CAP) as i64;
            }
            if width < 0 {
                left = true;
                width = -width;
            }
            let mut prec: Option<u64> = None;
            let mut prec_dot = false;
            if fmt.get(p) == Some(&b'.') {
                p += 1;
                prec_dot = true;
                if fmt.get(p) == Some(&b'*') {
                    p += 1;
                    let pi = match parse_index(fmt, p) {
                        Some((i, np)) => {
                            p = np;
                            bump(&mut end, i);
                            i
                        }
                        None => next_seq(&mut seq, &mut end),
                    };
                    if pi < args.len() {
                        let v = match self.convert_int(args[pi], true) {
                            INum::I(v) => v,
                            INum::U(v) => v as i64,
                        };
                        if v > INT_MAX_I64 {
                            return Err(Error::InvalidPrecision {
                                arg: OsString::from_vec(args[pi].to_vec()),
                            });
                        }
                        prec = if v < 0 { None } else { Some(v as u64) };
                    } else {
                        prec = Some(0);
                    }
                } else {
                    let mut pr: u64 = 0;
                    while p < fmt.len() && fmt[p].is_ascii_digit() {
                        pr = pr
                            .saturating_mul(10)
                            .saturating_add(u64::from(fmt[p] - b'0'));
                        p += 1;
                    }
                    prec = Some(pr.min(PAD_CAP));
                }
            }
            while matches!(fmt.get(p), Some(b'l' | b'L' | b'h' | b'j' | b't' | b'z')) {
                p += 1;
            }
            let conv = *fmt.get(p).unwrap_or(&0);
            if !conv_allowed(conv, alt, zero, quote, prec_dot) {
                let send = (p + 1).min(fmt.len());
                return Err(Error::InvalidConversion {
                    spec: OsString::from_vec(fmt[start..send].to_vec()),
                });
            }
            p += 1;
            let ai = take_slot(direc, &mut seq, &mut end);
            self.directive(
                conv,
                left,
                plus,
                space,
                alt,
                zero,
                width as usize,
                prec,
                args.get(ai).copied(),
            )?;
        }
        Ok(end.map_or(0, |e| e.saturating_add(1)).min(args.len()))
    }
}

struct Fmt {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: usize,
    prec: Option<u64>,
}

/// Pad sign + prefix + body to width. Zero padding goes after sign
/// and prefix; space padding goes outside unless left-justified.
fn apply_width(sign: &[u8], prefix: &[u8], body: Vec<u8>, f: &Fmt, zero_ok: bool) -> Vec<u8> {
    let total = sign.len() + prefix.len() + body.len();
    if f.width <= total {
        let mut v = Vec::with_capacity(total);
        v.extend_from_slice(sign);
        v.extend_from_slice(prefix);
        v.extend_from_slice(&body);
        return v;
    }
    let padn = f.width - total;
    let use_zero = f.zero && !f.left && zero_ok;
    let pad_byte = if use_zero { b'0' } else { b' ' };
    let mut v = Vec::with_capacity(f.width);
    if f.left {
        v.extend_from_slice(sign);
        v.extend_from_slice(prefix);
        v.extend_from_slice(&body);
        v.extend(std::iter::repeat(pad_byte).take(padn));
    } else if use_zero {
        v.extend_from_slice(sign);
        v.extend_from_slice(prefix);
        v.extend(std::iter::repeat(pad_byte).take(padn));
        v.extend_from_slice(&body);
    } else {
        v.extend(std::iter::repeat(pad_byte).take(padn));
        v.extend_from_slice(sign);
        v.extend_from_slice(prefix);
        v.extend_from_slice(&body);
    }
    v
}

fn to_base(mut mag: u64, base: u32, upper: bool) -> Vec<u8> {
    if mag == 0 {
        return vec![b'0'];
    }
    let digits: &[u8] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut v = Vec::new();
    while mag > 0 {
        v.push(digits[(mag % u64::from(base)) as usize]);
        mag /= u64::from(base);
    }
    v.reverse();
    v
}

fn fmt_int(v: i64, base: u32, upper: bool, f: &Fmt) -> Vec<u8> {
    let neg = v < 0;
    fmt_uint(v.unsigned_abs(), neg, base, upper, true, f)
}

fn fmt_uint(mag: u64, neg: bool, base: u32, upper: bool, signed: bool, f: &Fmt) -> Vec<u8> {
    let mut digits = to_base(mag, base, upper);
    if let Some(p) = f.prec {
        if mag == 0 && p == 0 {
            digits.clear();
        } else {
            let p = (p as usize).min(PAD_CAP as usize);
            if digits.len() < p {
                let mut v = vec![b'0'; p - digits.len()];
                v.extend_from_slice(&digits);
                digits = v;
            }
        }
    }
    let mut prefix: &[u8] = b"";
    if f.alt {
        if base == 8 {
            if digits.first() != Some(&b'0') {
                digits.insert(0, b'0');
            }
        } else if base == 16 && mag != 0 {
            prefix = if upper { b"0X" } else { b"0x" };
        }
    }
    let sign: &[u8] = if signed {
        if neg {
            b"-"
        } else if f.plus {
            b"+"
        } else if f.space {
            b" "
        } else {
            b""
        }
    } else {
        b""
    };
    apply_width(sign, prefix, digits, f, f.prec.is_none())
}

fn fmt_char(b: u8, f: &Fmt) -> Vec<u8> {
    apply_width(b"", b"", vec![b], f, false)
}

fn fmt_str(s: &[u8], f: &Fmt) -> Vec<u8> {
    let end = match f.prec {
        Some(p) => (p as usize).min(PAD_CAP as usize).min(s.len()),
        None => s.len(),
    };
    apply_width(b"", b"", s[..end].to_vec(), f, false)
}

/// Format a finite non-NaN value per `conv` (fFHVFeEgGaA casing via
/// `conv`), precision defaulting per conversion. `v` must be finite;
///
/// NaN and infinity are handled by the caller.
fn fmt_float(v: f64, conv: u8, prec: Option<u64>, f: &Fmt) -> Vec<u8> {
    let upper = matches!(conv, b'A' | b'E' | b'F' | b'G');
    let neg = v.is_sign_negative();
    let sign: &[u8] = if neg {
        b"-"
    } else if f.plus {
        b"+"
    } else if f.space {
        b" "
    } else {
        b""
    };
    let body: Vec<u8> = if v.is_nan() {
        if upper {
            b"NAN".to_vec()
        } else {
            b"nan".to_vec()
        }
    } else if v.is_infinite() {
        if upper {
            b"INF".to_vec()
        } else {
            b"inf".to_vec()
        }
    } else {
        finite_float(v.abs(), conv, prec, f)
    };
    // Glibc pads infinities and NaN with spaces even under `0`.
    let zero_ok = v.is_finite();
    apply_width(sign, b"", body, f, zero_ok)
}

fn finite_float(v: f64, conv: u8, prec: Option<u64>, f: &Fmt) -> Vec<u8> {
    match conv {
        b'f' | b'F' => {
            let p = prec.unwrap_or(6).min(PAD_CAP);
            let mut s = fmt_fixed(v, p);
            if f.alt && !s.contains('.') {
                s.push('.');
            }
            s.into_bytes()
        }
        b'e' | b'E' => {
            let p = prec.unwrap_or(6).min(PAD_CAP);
            let (m, exp) = fmt_exp_parts(v, p);
            let mut out = String::with_capacity(m.len() + 6);
            out.push_str(&m);
            if f.alt && !out.contains('.') {
                out.push('.');
            }
            out.push(if conv == b'E' { 'E' } else { 'e' });
            out.push_str(&format!("{exp:+03}"));
            out.into_bytes()
        }
        b'g' | b'G' => {
            let mut p = prec.unwrap_or(6);
            if p == 0 {
                p = 1;
            }
            let eform = conv == b'G';
            if v == 0.0 {
                let mut s = fmt_fixed(v, p - 1);
                if !f.alt {
                    strip_zeros(&mut s);
                } else if !s.contains('.') {
                    s.push('.');
                }
                return s.into_bytes();
            }
            let exp10 = short_exp(v);
            let mut s = if -4 <= exp10 && exp10 < p as i64 {
                fmt_fixed(v, (p as i64 - 1 - exp10) as u64)
            } else {
                let (m, exp) = fmt_exp_parts(v, p - 1);
                format!("{m}e{exp:+03}")
            };
            if !f.alt {
                strip_zeros(&mut s);
            } else if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                s.push('.');
            }
            if eform {
                // Reformat a lowercase exponent the %E way.
                if let Some((m, e)) = s.split_once('e') {
                    let exp: i32 = e.parse().unwrap_or(0);
                    s = format!("{m}E{exp:+03}");
                }
            }
            s.into_bytes()
        }
        _ => hexfloat(v, prec, conv == b'A', f.alt),
    }
}

/// Fixed-notation float with any precision: Rust caps float
/// precision at 65535, so larger precisions render at the cap and pad
/// with zeros. Exact f64 expansions end far below the cap, making the
/// extra digits all zero either way: identical output.
fn fmt_fixed(v: f64, prec: u64) -> String {
    const LIM: u64 = 65535;
    if prec <= LIM {
        return format!("{v:.p$}", p = prec as usize);
    }
    let mut s = format!("{v:.p$}", p = LIM as usize);
    s.extend(std::iter::repeat('0').take((prec - LIM) as usize));
    s
}

/// Scientific-notation mantissa with any precision (same chunking as
/// [`fmt_fixed`]), plus the exact decimal exponent.
fn fmt_exp_parts(v: f64, prec: u64) -> (String, i32) {
    const LIM: u64 = 65535;
    let p = prec.min(LIM) as usize;
    let s = format!("{v:.p$e}", p = p);
    let (m, e) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    let mut m = m.to_owned();
    m.extend(std::iter::repeat('0').take((prec.min(PAD_CAP) - prec.min(LIM)) as usize));
    (m, e.parse().unwrap_or(0))
}

/// Trailing-zero strip for `%g` without `#`.
fn strip_zeros(s: &mut String) {
    if let Some(dot) = s.find('.') {
        let exp_at = s.find('e').or_else(|| s.find('E')).unwrap_or(s.len());
        let mut end = exp_at;
        while end > dot + 1 && s.as_bytes()[end - 1] == b'0' {
            end -= 1;
        }
        let mut cut = end;
        if cut == dot + 1 {
            cut = dot;
        }
        let tail = s[exp_at..].to_owned();
        s.truncate(cut);
        s.push_str(&tail);
    }
}

/// Exact decimal exponent of `v` via its shortest `%e` rendering.
fn short_exp(v: f64) -> i64 {
    let s = format!("{v:e}");
    s.split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0)
}

/// Exact shortest hexadecimal float (`%a`): groups the significand
/// from the most significant bit, strips trailing zero groups, and
/// adjusts the exponent. Describes the `f64` exactly; GNU's rendering
/// of the same decimal input may differ in trailing digits because it
/// formats the 80-bit `long double` (see tests).
fn hexfloat(v: f64, prec: Option<u64>, upper: bool, alt: bool) -> Vec<u8> {
    let bits = v.to_bits();
    let (s, e) = if v == 0.0 {
        (0u64, 0i32)
    } else {
        let ebits = ((bits >> 52) & 0x7FF) as i32;
        if ebits == 0 {
            (bits & 0xFFFFFFFFFFFFF, -1074)
        } else {
            ((bits & 0xFFFFFFFFFFFFF) | (1 << 52), ebits - 1023 - 52)
        }
    };
    let (mut digits, mut exp) = if s == 0 {
        (vec![0u8], 0i32)
    } else {
        let bitlen = 64 - s.leading_zeros();
        let pad = (4 - bitlen % 4) % 4;
        let sp = s << pad;
        let ep = e - pad as i32;
        let n = ((bitlen + pad) / 4) as usize;
        let mut digits: Vec<u8> = (0..n)
            .map(|i| ((sp >> (4 * (n - 1 - i))) & 0xF) as u8)
            .collect();
        let mut t = 0;
        while digits.len() > 1 && digits.last() == Some(&0) {
            digits.pop();
            t += 1;
        }
        (digits, ep + 4 * t)
    };
    // Placing the point after the first significant digit scales the
    // mantissa by 16^(len-1): the exponent moves the opposite way.
    // This uses the significant length, before precision padding.
    if digits.len() > 1 {
        exp += 4 * (digits.len() - 1) as i32;
    }
    if let Some(p) = prec {
        let p = (p.min(PAD_CAP)) as usize;
        if digits.len() - 1 > p {
            round_hex(&mut digits, &mut exp, p + 1);
        } else {
            while digits.len() < p + 1 {
                digits.push(0);
            }
        }
    }
    let hexd: &[u8] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Vec::new();
    out.extend_from_slice(if upper { b"0X" } else { b"0x" });
    out.push(hexd[digits[0] as usize]);
    if digits.len() > 1 || alt {
        out.push(b'.');
        for &d in &digits[1..] {
            out.push(hexd[d as usize]);
        }
    }
    out.push(if upper { b'P' } else { b'p' });
    out.extend_from_slice(format!("{exp:+}").as_bytes());
    out
}

/// Round hex `digits` to `keep` digits (half to even on the dropped
/// stream), renormalizing on carry overflow with `exp += 4`.
fn round_hex(digits: &mut Vec<u8>, exp: &mut i32, keep: usize) {
    if digits.len() <= keep {
        return;
    }
    let first = digits[keep];
    let rest_nonzero = digits[keep + 1..].iter().any(|&d| d != 0);
    let up = first > 8 || (first == 8 && (rest_nonzero || digits[keep - 1] % 2 == 1));
    digits.truncate(keep);
    if !up {
        return;
    }
    let mut k = keep - 1;
    loop {
        if digits[k] < 15 {
            digits[k] += 1;
            break;
        }
        digits[k] = 0;
        if k == 0 {
            *digits = vec![1];
            digits.extend(std::iter::repeat(0).take(keep - 1));
            *exp += 4;
            break;
        }
        k -= 1;
    }
}

/// Quote a diagnostic argument, GNU message style (C locale):
/// single-quote wrap with C backslash escapes (`\'`, `\\`, named
/// controls, `\ooo` octal otherwise). Byte-exact over any input.
fn squote(bytes: &[u8]) -> Vec<u8> {
    let mut s = b"'".to_vec();
    for &b in bytes {
        match b {
            b'\'' => s.extend_from_slice(b"\\'"),
            b'\\' => s.extend_from_slice(b"\\\\"),
            0x07 => s.extend_from_slice(b"\\a"),
            0x08 => s.extend_from_slice(b"\\b"),
            0x0c => s.extend_from_slice(b"\\f"),
            0x0a => s.extend_from_slice(b"\\n"),
            0x0d => s.extend_from_slice(b"\\r"),
            0x09 => s.extend_from_slice(b"\\t"),
            0x0b => s.extend_from_slice(b"\\v"),
            0x20..=0x7e => s.push(b),
            _ => s.extend_from_slice(format!("\\{b:03o}").as_bytes()),
        }
    }
    s.push(b'\'');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    fn run(format: &str, args: &[&str]) -> Output {
        let f = OsStr::new(format);
        let a: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        call(f, &a, &Options::default()).unwrap()
    }

    fn run_bytes(format: &str, args: &[&str]) -> Vec<u8> {
        run(format, args).bytes.as_bytes().to_vec()
    }

    // --- Format escapes (uutils tests/by-util/test_printf.rs) ---

    #[test]
    fn escape_letters() {
        // Probed against coreutils 9.10: \x1b \n \33 \e.
        assert_eq!(
            run_bytes("\\x1b\\n\\33\\n\\e\\n", &[]),
            b"\x1b\n\x1b\n\x1b\n"
        );
    }

    #[test]
    fn escape_unknown_z_then_c_stops() {
        // uutils test_printf.rs: escaped_unrecognized +
        // stop_after_additional_escape: \z is literal, \c ends
        // everything.
        let base: Vec<u8> = b"\x07\x08\x0c\x0a\x0d\x09\x0b\\z".to_vec();
        assert_eq!(run_bytes("\\a\\b\\f\\n\\r\\t\\v\\z\\c", &[]), base);
        assert_eq!(run_bytes("\\a\\b\\f\\n\\r\\t\\v\\z\\ca", &[]), base);
        let mut prefixed = b"a".to_vec();
        prefixed.extend_from_slice(&base);
        assert_eq!(run_bytes("a\\a\\b\\f\\n\\r\\t\\v\\z\\c", &[]), prefixed);
        assert_eq!(run_bytes("a\\a\\b\\f\\n\\r\\t\\v\\z\\cb", &[]), prefixed);
    }

    #[test]
    fn escape_hex_takes_at_most_two_digits() {
        // Probed against coreutils 9.10: \x7e consumes 2 digits, `3`
        // stays literal.
        assert_eq!(run_bytes("\\x7e3\\n", &[]), b"~3\n");
    }

    #[test]
    fn escape_bare_hex_is_fatal() {
        // uutils test_printf.rs: test_missing_escaped_hex_value.
        let err = call(OsStr::new("\\x"), &[], &Options::default()).unwrap_err();
        assert_eq!(err.error, Error::MissingHexNumber);
        assert_eq!(
            err.message(),
            OsString::from("missing hexadecimal number in escape")
        );
    }

    #[test]
    fn escape_octal_forms() {
        // uutils test_printf.rs: variable_sized_octal: \2 \02 \002
        // then \000 + literal `2y`.
        assert_eq!(
            run_bytes("\\2y \\02y \\002y \\0002y", &[]),
            b"\x02y \x02y \x02y \x002y"
        );
    }

    #[test]
    fn escape_octal_three_digit_cap() {
        // uutils test_printf.rs: escaped_octal_and_newline +
        // variable_sized_octal + escaped_unicode_null_byte: an octal
        // escape stops after three digits, and \0ooo in the format
        // string emits a NUL before the literal tail.
        assert_eq!(run_bytes("\\101\\0377\\n", &[]), b"A\x1f7\n");
        assert_eq!(run_bytes("|\\0005|", &[]), [b'|', 0, b'5', b'|']);
        assert_eq!(run_bytes("\\0001_", &[]), [0u8, b'1', b'_']);
        assert_eq!(run_bytes("%b", &["\\0001_"]), [1u8, b'_']);
    }

    #[test]
    fn escape_backslash_and_quotes() {
        // Probed against coreutils 9.10: \\\" is a double quote.
        assert_eq!(run_bytes("\\\"", &[]), b"\"");
        assert_eq!(run_bytes("\\'", &[]), b"'");
        assert_eq!(run_bytes("\\\\", &[]), b"\\");
    }

    #[test]
    fn escape_unicode() {
        // uutils test_printf.rs: escaped_unicode_four_digit /
        // escaped_unicode_eight_digit (ASCII values here; non-ASCII
        // emission follows the UTF-8 assumption in the module docs).
        assert_eq!(run_bytes("\\u0032", &[]), b"2");
        assert_eq!(run_bytes("\\U00000032", &[]), b"2");
    }

    #[test]
    fn escape_unicode_errors() {
        // uutils test_printf.rs: escaped_unicode_incomplete +
        // escaped_unicode_invalid.
        let err = call(OsStr::new("\\u00"), &[], &Options::default()).unwrap_err();
        assert_eq!(err.error, Error::MissingHexNumber);
        let err = call(OsStr::new("\\U0000"), &[], &Options::default()).unwrap_err();
        assert_eq!(err.error, Error::MissingHexNumber);
        let err = call(OsStr::new("\\ud800"), &[], &Options::default()).unwrap_err();
        assert_eq!(
            err.error,
            Error::InvalidUniversalChar {
                digits: "ud800".to_owned()
            }
        );
        assert_eq!(
            err.message(),
            OsString::from("invalid universal character name \\ud800")
        );
        // Uppercase input still reports lowercase digits (probed
        // against coreutils 9.10; uutils preserves the input's case
        // in this message, which the real binary does not).
        let err = call(OsStr::new("\\uD800"), &[], &Options::default()).unwrap_err();
        assert_eq!(
            err.message(),
            OsString::from("invalid universal character name \\ud800")
        );
        let err = call(OsStr::new("\\U0000D8F9"), &[], &Options::default()).unwrap_err();
        assert_eq!(
            err.message(),
            OsString::from("invalid universal character name \\U0000d8f9")
        );
    }

    #[test]
    fn escape_unicode_letter_forms() {
        // uutils test_printf.rs: escaped_unicode_four_digit /
        // escaped_unicode_eight_digit.
        assert_eq!(run_bytes("\\u0125", &[]), "ĥ".as_bytes());
        assert_eq!(run_bytes("\\U00000125", &[]), "ĥ".as_bytes());
        // uutils test_printf.rs: escaped_unicode_incomplete.
        for fmt in ["\\u", "\\U", "\\uabc", "\\Uabcd"] {
            let err = call(OsStr::new(fmt), &[], &Options::default()).unwrap_err();
            assert_eq!(err.error, Error::MissingHexNumber, "format {fmt}");
        }
    }

    #[test]
    fn escape_unicode_overlarge_echoes() {
        // Probed against coreutils 9.10: a code point above U+10FFFF
        // re-emits the escape in canonical uppercase form rather than
        // a character or an error.
        assert_eq!(run_bytes("\\U00110001", &[]), b"\\U00110001");
        assert_eq!(run_bytes("\\U00abcdef", &[]), b"\\U00ABCDEF");
    }

    #[test]
    fn escape_trailing_backslash_is_literal() {
        // Probed against coreutils 9.10: trailing `\` with nothing
        // after stays.
        assert_eq!(run_bytes("foo\\", &[]), b"foo\\");
    }

    #[test]
    fn escape_c_stops_format() {
        // uutils test_printf.rs: stop_after_additional_escape: \c
        // produces no further output; exit stays 0.
        let out = run("ab\\cde", &[]);
        assert_eq!(out.bytes.as_bytes(), b"ab");
        assert_eq!(out.exit_code(), 0);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn escape_c_after_spec_stops_all_output() {
        // uutils test_printf.rs: stop_after_additional_escape: a \c
        // in the format discards the rest of the format and all
        // further arguments.
        let out = run("A%sC\\cD%sF", &["B", "E"]);
        assert_eq!(out.bytes.as_bytes(), b"ABC");
        assert_eq!(out.exit_code(), 0);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn escape_x40() {
        // Probed against coreutils 9.10 (with and without
        // POSIXLY_CORRECT): \x40 is unconditional.
        assert_eq!(run_bytes("\\x40", &[]), b"@");
    }

    // --- Literals and %% ---

    #[test]
    fn literal_passthrough() {
        assert_eq!(run_bytes("hello", &[]), b"hello");
        assert_eq!(run_bytes("", &[]), b"");
    }

    #[test]
    fn percent_percent() {
        // uutils test_printf.rs: escaped_percent_sign.
        assert_eq!(run_bytes("%%", &[]), b"%");
    }

    #[test]
    fn double_dash_format_is_literal() {
        // uutils test_printf.rs: double_dash_as_the_format_is_printed_literally:
        // the CLI layer strips a leading `--`; the function itself
        // treats `--` as plain text.
        assert_eq!(run_bytes("--", &[]), b"--");
    }

    #[test]
    fn stray_percent_is_fatal() {
        // Probed against coreutils 9.10.
        let err = call(OsStr::new("foo%"), &[], &Options::default()).unwrap_err();
        assert_eq!(
            err.error,
            Error::InvalidConversion {
                spec: OsString::from("%")
            }
        );
        assert_eq!(
            err.message(),
            OsString::from("%: invalid conversion specification")
        );
    }

    #[test]
    fn fatal_keeps_partial_output() {
        // Probed against coreutils 9.10: `printf 'a%#db' 1` prints
        // `a`, then fails.
        let args: Vec<&OsStr> = ["1"].iter().map(OsStr::new).collect();
        let fatal = call(OsStr::new("a%#db"), &args, &Options::default()).unwrap_err();
        assert_eq!(fatal.output.bytes.as_bytes(), b"a");
        assert_eq!(
            fatal.error,
            Error::InvalidConversion {
                spec: OsString::from("%#d")
            }
        );
        assert_eq!(
            fatal.message(),
            OsString::from("%#d: invalid conversion specification")
        );
    }

    // --- %s and %c ---

    #[test]
    fn s_basic_width() {
        // uutils test_printf.rs: sub_min_width / sub_min_width_negative.
        assert_eq!(run_bytes("[%10s]", &["left pad"]), b"[  left pad]");
        assert_eq!(run_bytes("[%-10s]", &["right pad"]), b"[right pad ]");
        assert_eq!(run_bytes("hello %7s", &["world"]), b"hello   world");
        assert_eq!(run_bytes("hello %-7s", &["world"]), b"hello world  ");
    }

    #[test]
    fn s_precision_counts_bytes() {
        // uutils test_printf.rs: sub_str_max_chars_input; probed
        // against coreutils 9.10 under the C locale: %.1s of é keeps
        // 1 byte.
        assert_eq!(run_bytes("%.3s", &["unibyte"]), b"uni");
        // uutils test_printf.rs: sub_str_max_chars_input.
        assert_eq!(run_bytes("hello %7.2s", &["world"]), b"hello      wo");
        assert_eq!(run_bytes("%.1s", &["é"]), b"\xc3");
        assert_eq!(run_bytes("%5.3s", &["abcdef"]), b"  abc");
        assert_eq!(run_bytes("%-5.3s", &["abcdef"]), b"abc  ");
    }

    #[test]
    fn s_missing_arg_is_empty() {
        let out = run("[%s]", &[]);
        assert_eq!(out.bytes.as_bytes(), b"[]");
        assert_eq!(out.exit_code(), 0);
    }

    #[test]
    fn s_star_missing_args_is_empty() {
        // Probed against coreutils 9.10: no output, exit 0.
        for fmt in ["%*s", "%.*s"] {
            let out = run(fmt, &[]);
            assert_eq!(out.bytes.as_bytes(), b"");
            assert_eq!(out.exit_code(), 0);
        }
    }

    #[test]
    fn c_takes_first_byte() {
        // uutils test_printf.rs: sub_char / sub_char_from_string /
        // char_as_byte.
        assert_eq!(run_bytes("%c", &["123"]), b"1");
        assert_eq!(run_bytes("%*c", &["3", "123"]), b"  1");
        assert_eq!(run_bytes("%3c", &["ab"]), b"  a");
    }

    #[test]
    fn c_missing_arg_is_nul() {
        // Probed against coreutils 9.10: a missing %c argument
        // formats '\0'.
        assert_eq!(run_bytes("%c", &[]), b"\x00");
    }

    #[test]
    fn invalid_specs_are_fatal() {
        // uutils test_printf.rs: sub_b/q_string_validate_field_params,
        // format_spec_zero_fails, positional_format_specifiers +
        // probes against coreutils 9.10.
        for (fmt, spec) in [
            ("%#d", "%#d"),
            ("%0s", "%0s"),
            ("%.9c", "%.9c"),
            ("%'s", "%'s"),
            ("%5b", "%5b"),
            ("%*b", "%*b"),
            ("%5q", "%5q"),
            ("%-2$s", "%-2$"),
            ("% 2$s", "% 2$"),
            ("%$d", "%$"),
            ("%0$d", "%0$"),
        ] {
            let args: Vec<&OsStr> = ["A", "B"].iter().map(OsStr::new).collect();
            let err = call(OsStr::new(fmt), &args, &Options::default()).unwrap_err();
            assert_eq!(
                err.error,
                Error::InvalidConversion {
                    spec: OsString::from(spec)
                },
                "format {fmt}"
            );
            assert_eq!(
                err.message(),
                OsString::from(format!("{spec}: invalid conversion specification"))
            );
        }
    }

    // --- Integers ---

    #[test]
    fn d_basic_and_bases() {
        assert_eq!(run_bytes("%d", &["42"]), b"42");
        assert_eq!(run_bytes("%i", &["-7"]), b"-7");
        assert_eq!(run_bytes("%d", &["  -0x10"]), b"-16");
        assert_eq!(run_bytes("%o", &["8"]), b"10");
        assert_eq!(run_bytes("%x", &["255"]), b"ff");
        assert_eq!(run_bytes("%X", &["999"]), b"3E7");
        assert_eq!(run_bytes("%u", &["0xFFFFFFFF"]), b"4294967295");
        assert_eq!(run_bytes("%u", &["-1"]), b"18446744073709551615");
        // Probed against coreutils 9.10: base-0 conversion also
        // takes 0b (C2X).
        assert_eq!(run("%d", &["0b101"]).bytes.as_bytes(), b"5");
        assert_eq!(run("%d", &["0b101"]).exit_code(), 0);
    }

    #[test]
    fn d_flags_width_precision() {
        // uutils test_printf.rs: space_padding_with_space_test,
        // zero_padding_with_plus_test, sub_int_leading_zeroes_padded,
        // sub_any_asterisk_* + probes against coreutils 9.10.
        assert_eq!(run_bytes("% d", &["33"]), b" 33");
        assert_eq!(run_bytes("%+d", &["33"]), b"+33");
        assert_eq!(run_bytes("%02d", &["1"]), b"01");
        assert_eq!(run_bytes("%5.4d", &["999"]), b" 0999");
        assert_eq!(run_bytes("%5.4X", &["999"]), b" 03E7");
        assert_eq!(run_bytes("%4X", &["999"]), b" 3E7");
        assert_eq!(run_bytes("%.4X", &["999"]), b"03E7");
        assert_eq!(run_bytes("%*d", &["4", "42"]), b"  42");
        assert_eq!(run_bytes("%.*d", &["4", "42"]), b"0042");
        assert_eq!(run_bytes("%*.*d", &["3", "2", "15"]), b" 15");
        assert_eq!(run_bytes("%.*X", &["4", "42"]), b"002A");
        assert_eq!(run_bytes("%*.*X", &["3", "2", "15"]), b" 0F");
        // Negative precision counts as omitted.
        assert_eq!(run_bytes("%.*d", &["-3", "15"]), b"15");
        // Negative width left-justifies (uutils test_printf.rs:
        // sub_any_asterisk_negative_first_param).
        assert_eq!(run_bytes("9 %*dx", &["-2", "0"]), b"9 0 x");
    }

    #[test]
    fn hash_octal_hex() {
        assert_eq!(run_bytes("%#o", &["420"]), b"0644");
        assert_eq!(run_bytes("%#x", &["255"]), b"0xff");
        assert_eq!(run_bytes("%#x", &["0"]), b"0");
    }

    #[test]
    fn grouping_is_c_locale_noop() {
        // uutils test_printf.rs: sub_num_thousands; probed under
        // LC_ALL=C, accepted for d i u f F g G and ignored.
        assert_eq!(run_bytes("%'d", &["1234567"]), b"1234567");
        assert_eq!(run_bytes("%'d", &["3333"]), b"3333");
    }

    #[test]
    fn length_modifiers_ignored() {
        // uutils test_printf.rs: sub_any_specifiers: l L h j t z are
        // consumed; the glibc-only 'I' no-op is probed.
        assert_eq!(
            run_bytes("%lld %hd %zd %Id", &["5", "-5", "7", "9"]),
            b"5 -5 7 9"
        );
        assert_eq!(run_bytes("%LF %LE", &["1", "2"]), b"1.000000 2.000000E+00");
    }

    #[test]
    fn int_width_overflow_fatal() {
        // uutils test_printf.rs: invalid_precision_tests; the
        // field-width half probed against coreutils 9.10.
        let err = call(
            OsStr::new("%*d"),
            &[OsStr::new("2147483648"), OsStr::new("15")],
            &Options::default(),
        )
        .unwrap_err();
        assert_eq!(
            err.error,
            Error::InvalidFieldWidth {
                arg: OsString::from("2147483648")
            }
        );
        assert_eq!(
            err.message(),
            OsString::from("invalid field width: '2147483648'")
        );
        let err = call(
            OsStr::new("%.*d"),
            &[OsStr::new("2147483648"), OsStr::new("15")],
            &Options::default(),
        )
        .unwrap_err();
        assert_eq!(
            err.error,
            Error::InvalidPrecision {
                arg: OsString::from("2147483648")
            }
        );
        assert_eq!(
            err.message(),
            OsString::from("invalid precision: '2147483648'")
        );
    }

    #[test]
    fn int_width_big_works() {
        // uutils test_printf.rs: test_numeric_field_width_above_u16_max,
        // test_precision_above_formatter_limit: large (not absurd)
        // magnitudes work.
        let out = run("%70000d", &["1"]);
        assert_eq!(out.bytes.as_bytes().len(), 70000);
        assert_eq!(out.bytes.as_bytes()[69999], b'1');
        let out = run("%500000s", &["x"]);
        assert_eq!(out.bytes.as_bytes().len(), 500000);
        let out = run("%.70000f", &["1"]);
        assert_eq!(out.bytes.as_bytes().len(), 70002);
    }

    #[test]
    fn char_constants() {
        // uutils test_printf.rs: sub_num_int_char_const_in: a
        // single-quote char constant is 97 either way.
        assert_eq!(run_bytes("%d", &["\"a"]), b"97");
        assert_eq!(run("%d", &["\"a"]).warnings, vec![]);
        // A lone quote is not a constant: numeric parse fails.
        let out = run("%d", &["\""]);
        assert_eq!(out.bytes.as_bytes(), b"0");
        assert_eq!(
            out.warnings,
            vec![Warning::ExpectedNumericValue {
                arg: OsString::from("\"")
            }]
        );
        // Trailing characters warn but exit 0 (GNU only fails the
        // status on numeric conversion problems, never on this).
        let out = run("%d", &["\"a\""]);
        assert_eq!(out.bytes.as_bytes(), b"97");
        assert_eq!(
            out.warnings,
            vec![Warning::CharConstTrailing {
                trailing: OsString::from("\"")
            }]
        );
        assert_eq!(out.exit_code(), 0);
        // Multibyte constant decodes UTF-8 (assumption from the
        // module docs; uutils test_printf.rs: mb_input covers these
        // variants, locale-dependent fr vectors were dropped).
        assert_eq!(run_bytes("%d", &["\"é"]), "233".as_bytes());
        let out = run("%d", &["\"é="]);
        assert_eq!(out.bytes.as_bytes(), b"233");
        assert_eq!(
            out.warnings,
            vec![Warning::CharConstTrailing {
                trailing: OsString::from("=")
            }]
        );
    }

    #[test]
    fn posixly_correct_suppresses_char_const_warning() {
        // uutils test_printf.rs: partial_char_posixly_correct: the
        // char-constant trailing warning is gated on POSIXLY_CORRECT.
        let out = call(
            OsStr::new("%d"),
            &[OsStr::new("\"a\"")],
            &Options {
                posixly_correct: true,
            },
        )
        .unwrap();
        assert_eq!(out.bytes.as_bytes(), b"97");
        assert!(out.warnings.is_empty());
        assert_eq!(out.exit_code(), 0);
    }

    #[test]
    fn numeric_warnings() {
        // uutils test_printf.rs: partial_integer + probes against
        // coreutils 9.10: bad and partial numeric inputs.
        let out = run("%d", &["no-num"]);
        assert_eq!(out.bytes.as_bytes(), b"0");
        assert_eq!(
            out.warnings,
            vec![Warning::ExpectedNumericValue {
                arg: OsString::from("no-num")
            }]
        );
        assert_eq!(out.exit_code(), 1);

        let out = run("%d", &["9z"]);
        assert_eq!(out.bytes.as_bytes(), b"9");
        assert_eq!(
            out.warnings,
            vec![Warning::ValueNotCompletelyConverted {
                arg: OsString::from("9z")
            }]
        );

        for arg in ["", "+", "08"] {
            let out = run("%d", &[arg]);
            assert_eq!(out.exit_code(), 1, "arg {arg:?}");
        }
        assert_eq!(run_bytes("%d", &["08"]), b"0");
        assert_eq!(run_bytes("%x", &["0x"]), b"0");

        // 30 nines clamp to INTMAX_MAX with an out-of-range warning.
        let nines = "9".repeat(30);
        let out = run("%d", &[nines.as_str()]);
        assert_eq!(out.bytes.as_bytes(), b"9223372036854775807");
        assert_eq!(
            out.warnings,
            vec![Warning::OutOfRange { arg: nines.into() }]
        );
        assert_eq!(out.exit_code(), 1);

        // Width args convert the same way, non-fatally.
        let out = run("%*d", &["abc", "5"]);
        assert_eq!(out.bytes.as_bytes(), b"5");
        assert_eq!(
            out.warnings,
            vec![Warning::ExpectedNumericValue {
                arg: OsString::from("abc")
            }]
        );
        assert_eq!(out.exit_code(), 1);
    }

    // --- Floats ---

    #[test]
    fn float_basic() {
        assert_eq!(run_bytes("%f", &["1e3"]), b"1000.000000");
        assert_eq!(run_bytes("%f", &["0x10p2"]), b"64.000000");
        assert_eq!(run_bytes("%f", &["nan"]), b"nan");
        assert_eq!(run_bytes("%f", &["INF"]), b"inf");
        assert_eq!(run_bytes("%F", &["nan"]), b"NAN");
        assert_eq!(run_bytes("%E", &["inf"]), b"INF");
        assert_eq!(run_bytes("%+f", &["inf"]), b"+inf");
        assert_eq!(run_bytes("%f", &["-0.0"]), b"-0.000000");
        assert_eq!(run_bytes("%e", &["0"]), b"0.000000e+00");
        assert_eq!(run_bytes("%e", &["-0.0"]), b"-0.000000e+00");
        assert_eq!(run_bytes("%e", &["123.456789"]), b"1.234568e+02");
        assert_eq!(run_bytes("%.1e", &["0.00098765"]), b"9.9e-04");
        assert_eq!(run_bytes("%08.2f", &["3.14"]), b"00003.14");
        assert_eq!(run_bytes("%05.2f", &["-3.14159"]), b"-3.14");
        assert_eq!(run_bytes("%.0f", &["2.5"]), b"2");
        assert_eq!(run_bytes("%.0f", &["3.5"]), b"4");
        assert_eq!(run_bytes("%F", &["1"]), b"1.000000");
        assert_eq!(run_bytes("%E", &["2"]), b"2.000000E+00");
        // Trailing garbage converts the leading part + warns.
        let out = run("%f", &["  3.5xyz"]);
        assert_eq!(out.bytes.as_bytes(), b"3.500000");
        assert_eq!(out.exit_code(), 1);
    }

    #[test]
    fn float_nonfinite_pad_with_spaces() {
        // uutils test_printf.rs: float_non_finite_zero_padding;
        // probed: inf/nan pad with spaces even under `0`.
        assert_eq!(run_bytes("%08.2f", &["inf"]), b"     inf");
        assert_eq!(run_bytes("%08.2f", &["-inf"]), b"    -inf");
        assert_eq!(run_bytes("%08.2f", &["nan"]), b"     nan");
        assert_eq!(run_bytes("%08.2f", &["-nan"]), b"    -nan");
    }

    #[test]
    fn float_subnormal_parse_does_not_warn() {
        // 5e-324 is the smallest f64 subnormal: converting it warns
        // under strtof rules, but GNU parses long double (normal
        // there) and stays silent. We match the observable behavior.
        let out = run("%f", &["5e-324"]);
        assert_eq!(out.bytes.as_bytes(), b"0.000000");
        assert!(out.warnings.is_empty());
        assert_eq!(out.exit_code(), 0);
    }

    #[test]
    fn float_g() {
        assert_eq!(run_bytes("%g", &["0.0001"]), b"0.0001");
        assert_eq!(run_bytes("%g", &["0.00001"]), b"1e-05");
        assert_eq!(run_bytes("%g", &["1234567"]), b"1.23457e+06");
        assert_eq!(run_bytes("%g", &["1000000"]), b"1e+06");
        assert_eq!(run_bytes("%g", &["0.000001"]), b"1e-06");
        assert_eq!(run_bytes("%G", &["0.00001"]), b"1E-05");
        assert_eq!(run_bytes("%#g", &["100"]), b"100.000");
        assert_eq!(run_bytes("%.*G", &["4", "42"]), b"42");
    }

    #[test]
    fn float_hex_exact_dyadics() {
        // Exactly representable in f64 AND long double: byte-identical
        // to coreutils 9.10 (probed).
        assert_eq!(run_bytes("%a", &["0.5"]), b"0x8p-4");
        assert_eq!(run_bytes("%a", &["1.5"]), b"0xcp-3");
        assert_eq!(run_bytes("%A", &["255"]), b"0XF.FP+4");
        assert_eq!(run_bytes("%a", &["0.0009765625"]), b"0x8p-13");
        assert_eq!(run_bytes("%.3a", &["1.5"]), b"0xc.000p-3");
        assert_eq!(run_bytes("%a", &["inf"]), b"inf");
        assert_eq!(run_bytes("%a", &["nan"]), b"nan");
        assert_eq!(run_bytes("%a", &["-0.0"]), b"-0x0p+0");
    }

    #[test]
    fn float_hex_documents_long_double_gap() {
        // 0.1 is NOT exactly representable: GNU formats the 80-bit
        // long double (0xc.ccccccccccccccdp-7); we format the f64
        // exactly (one fewer digit). Same shape, documented gap.
        assert_eq!(run_bytes("%a", &["0.1"]), b"0xc.cccccccccccdp-7");
        // Smallest f64 subnormal is exact in f64; GNU shows its
        // long-double expansion instead.
        assert_eq!(run_bytes("%a", &["5e-324"]), b"0x8p-1077");
    }

    #[test]
    fn float_hex_roundtrip() {
        // Probed against coreutils 9.10: %a output parses back
        // through %.10f.
        let hex = run_bytes("%a", &["0.0009765625"]);
        assert_eq!(hex, b"0x8p-13");
        let back = run_bytes("%.10f", &["0x8p-13"]);
        assert_eq!(back, b"0.0009765625");
    }

    // --- %b ---

    #[test]
    fn b_interprets_escapes() {
        // uutils test_printf.rs: sub_b_string_variable_size_unicode /
        // escaped_unicode_null_byte: \1 \01 \001 \0001.
        assert_eq!(
            run_bytes("%b %b %b %b", &["\\1y", "\\01y", "\\001y", "\\0001y"]),
            b"\x01y \x01y \x01y \x01y"
        );
        // Probed against coreutils 9.10: %b also takes \u escapes.
        assert_eq!(run_bytes("%b", &["\\u0041"]), b"A");
    }

    #[test]
    fn b_c_stops_everything() {
        // uutils test_printf.rs: stop_after_additional_escape_in_b_string:
        // '%bfoo' 'bar\cbaz' qux prints just `bar`.
        let out = run("%bfoo", &["bar\\cbaz", "qux"]);
        assert_eq!(out.bytes.as_bytes(), b"bar");
        assert_eq!(out.exit_code(), 0);
    }

    #[test]
    fn b_missing_arg_prints_nothing() {
        // Probed against coreutils 9.10: %b with no argument prints
        // nothing.
        let out = run("%bX", &[]);
        assert_eq!(out.bytes.as_bytes(), b"X");
        assert_eq!(out.exit_code(), 0);
    }

    // --- %q (C locale; fr-locale vectors need locale reads) ---

    #[test]
    fn q_basic_vectors() {
        // uutils test_printf.rs: sub_q_string_empty / sub_q_string_non_printable
        // / test_q_string_control_chars_with_quotes (C locale).
        assert_eq!(run_bytes("%q", &[""]), b"''");
        assert_eq!(run_bytes("%q", &["'"]), b"\"'\"");
        assert_eq!(run_bytes("%q", &["a"]), b"a");
        assert_eq!(run_bytes("%q", &["a b"]), b"'a b'");
        assert_eq!(run_bytes("%q", &["~a"]), b"'~a'");
        assert_eq!(run_bytes("%q", &["a~"]), b"a~");
        assert_eq!(run_bytes("%q", &["a\r"]), b"'a'$'\\r'");
        assert_eq!(
            run_bytes("%q", &["\u{1}'\u{1}"]),
            b"''$'\\001'\\'''$'\\001'"
        );
    }

    #[test]
    fn q_c_locale_bytes() {
        // Probed against coreutils 9.10 under LC_ALL=C: every byte
        // escaped.
        let arg = OsString::from_vec(vec![0xc3, 0xa1, 0xe1, 0xb8, 0x83, 0xc3, 0xa7]);
        let out = call(OsStr::new("%q"), &[&arg], &Options::default()).unwrap();
        assert_eq!(
            out.bytes.as_bytes(),
            b"''$'\\303\\241\\341\\270\\203\\303\\247'"
        );
    }

    #[test]
    fn q_safe_set_and_edges() {
        // Probed against coreutils 9.10 under LC_ALL=C.
        for bare in [
            "-", "_", ".", "/", ",", ":", "+", "@", "%", "a#b", "a}b", "{x", "}x", "a~b",
        ] {
            assert_eq!(run_bytes("%q", &[bare]), bare.as_bytes(), "bare {bare:?}");
        }
        for (arg, expected) in [
            ("=", b"'='".as_slice()),
            ("^", b"'^'".as_slice()),
            ("!", b"'!'".as_slice()),
            ("{", b"'{'".as_slice()),
            ("}", b"'}'".as_slice()),
            ("=x", b"'=x'".as_slice()),
            ("#x", b"'#x'".as_slice()),
            ("^x", b"'^x'".as_slice()),
            ("~x", b"'~x'".as_slice()),
            ("a^b", b"'a^b'".as_slice()),
            ("a=b", b"'a=b'".as_slice()),
            ("a!b", b"'a!b'".as_slice()),
            ("a''b", b"\"a''b\"".as_slice()),
        ] {
            assert_eq!(run_bytes("%q", &[arg]), expected, "quoted {arg:?}");
        }
    }

    #[test]
    fn q_probe_regression() {
        // Probed against coreutils 9.10 (adjoining shapes; regression
        // net for the fuzz).
        assert_eq!(run_bytes("%q", &["a'b$c"]), b"'a'\\''b$c'");
        assert_eq!(run_bytes("%q", &["a=b"]), b"'a=b'");
        assert_eq!(run_bytes("%q", &["!"]), b"'!'");
        assert_eq!(run_bytes("%q", &["a!b"]), b"'a!b'");
    }

    #[test]
    fn q_segments() {
        // Probed segment rendering: safe runs single-quoted, unsafe
        // runs $''-quoted, lone quotes edge-escaped.
        assert_eq!(run_bytes("%q", &["a\u{1}b"]), b"'a'$'\\001''b'");
        assert_eq!(run_bytes("%q", &["\u{1}ab"]), b"''$'\\001''ab'");
        assert_eq!(run_bytes("%q", &["a b\u{1}c"]), b"'a b'$'\\001''c'");
        assert_eq!(run_bytes("%q", &["'a\"b'"]), b"''\\''a\"b'\\'''");
    }

    #[test]
    fn q_missing_arg_prints_nothing() {
        // Probed against coreutils 9.10: %q with no argument emits
        // nothing, exit 0.
        let out = run("%q", &[]);
        assert_eq!(out.bytes.as_bytes(), b"");
        assert_eq!(out.exit_code(), 0);
    }

    // --- Indexed directives (uutils test_printf.rs:
    // positional_format_specifiers) ---

    #[test]
    fn indexed_reorder_and_repeat() {
        assert_eq!(run_bytes("%2$s%1$s\\n", &["1", "2"]), b"21\n");
        assert_eq!(run_bytes("%1$s%1$s\\n", &["1", "2"]), b"11\n22\n");
    }

    #[test]
    fn indexed_mixed_with_sequential() {
        assert_eq!(
            run_bytes("%s %3$s %s\\n", &["A", "B", "C", "D"]),
            b"A C B\nD  \n"
        );
        assert_eq!(run_bytes("%1$*d\\n", &["4", "1"]), b"   4\n1\n");
        assert_eq!(run_bytes("%s %s %1$s\\n", &["A", "B"]), b"A B A\n");
        assert_eq!(run_bytes("%100$*d %s %s %s\\n", &["4", "1"]), b"   0 1  \n");
    }

    #[test]
    fn indexed_width_precision() {
        assert_eq!(run_bytes("%1$*2$.*3$d\\n", &["1", "3", "2"]), b" 01\n");
        assert_eq!(run_bytes("%3$*.*d\\n", &["3", "2", "1"]), b" 01\n");
        assert_eq!(run_bytes("%3$*2$.*d\\n", &["2", "3", "1"]), b" 01\n");
        assert_eq!(run_bytes("%3$*.*2$d\\n", &["3", "2", "1"]), b" 01\n");
        assert_eq!(run_bytes("%2$*1$d\\n", &["4", "1"]), b"   1\n");
        assert_eq!(run_bytes("%2$*d\\n", &["4", "1"]), b"   1\n");
        assert_eq!(run_bytes("%01$4d\\n", &["1"]), b"   1\n");
        assert_eq!(run_bytes("%1$0*2$d\\n", &["1", "4"]), b"0001\n");
    }

    #[test]
    fn indexed_huge_is_silent() {
        // uutils test_printf.rs: positional_format_specifiers: an
        // out-of-range index behaves like a missing argument, still
        // consuming the format pass.
        for idx in ["999", "2147483647", "2147483648"] {
            let fmt = format!("empty%{idx}$s\\n");
            let out = run(&fmt, &["foo"]);
            assert_eq!(out.bytes.as_bytes(), b"empty\n", "index {idx}");
            assert_eq!(out.exit_code(), 0);
        }
    }

    #[test]
    fn indexed_zero_dollar_is_fatal() {
        // uutils test_printf.rs: positional_format_specifiers pins
        // %0$d fatal; a leading %0$ never counts as an index (also
        // probed against coreutils 9.10).
        let err = call(OsStr::new("%0$d"), &[OsStr::new("5")], &Options::default()).unwrap_err();
        assert_eq!(
            err.error,
            Error::InvalidConversion {
                spec: OsString::from("%0$")
            }
        );
    }

    // --- Cycling and excess arguments ---

    #[test]
    fn zero_flag_with_indexed_hex_warns() {
        // Probed against coreutils 9.10: with A B, '%0x2$s %2$s'
        // converts "A" with a warning (prints 0), then literals and
        // %2$s print "2$s B".
        let out = run("%0x2$s %2$s\\n", &["A", "B"]);
        assert_eq!(out.bytes.as_bytes(), b"02$s B\n");
        assert_eq!(
            out.warnings,
            vec![Warning::ExpectedNumericValue {
                arg: OsString::from("A")
            }]
        );
        assert_eq!(out.exit_code(), 1);
    }

    #[test]
    fn format_cycles_over_args() {
        assert_eq!(run_bytes("%d %d,", &["1", "2", "3"]), b"1 2,3 0,");
    }

    #[test]
    fn excess_arguments_warn_but_exit_zero() {
        // uutils test_printf.rs: no_infinite_loop.
        let out = run("X", &["a", "b"]);
        assert_eq!(out.bytes.as_bytes(), b"X");
        assert_eq!(
            out.warnings,
            vec![Warning::ExcessArguments {
                first: OsString::from("a")
            }]
        );
        assert_eq!(out.exit_code(), 0);
        let out = run("%%", &["a"]);
        assert_eq!(out.bytes.as_bytes(), b"%");
        assert_eq!(out.warnings.len(), 1);
        assert_eq!(out.exit_code(), 0);
    }

    // --- Error message bodies ---

    #[test]
    fn error_messages() {
        assert_eq!(
            Error::InvalidConversion {
                spec: OsString::from("%#d")
            }
            .message(),
            "%#d: invalid conversion specification"
        );
        assert_eq!(
            Error::MissingHexNumber.message(),
            "missing hexadecimal number in escape"
        );
        assert_eq!(
            Error::InvalidUniversalChar {
                digits: "ud800".to_owned()
            }
            .message(),
            "invalid universal character name \\ud800"
        );
        assert_eq!(
            Error::InvalidFieldWidth {
                arg: OsString::from("x")
            }
            .message(),
            "invalid field width: 'x'"
        );
        assert_eq!(
            Error::InvalidPrecision {
                arg: OsString::from("x")
            }
            .message(),
            "invalid precision: 'x'"
        );
    }

    #[test]
    fn message_bytes_are_exact() {
        // GNU renders diagnostics byte-exact (C locale): C escapes,
        // octal for the rest, raw bytes for character constants.
        let msg = Warning::ExpectedNumericValue {
            arg: OsString::from("a\nb"),
        }
        .message();
        assert_eq!(msg.as_encoded_bytes(), b"'a\\nb': expected a numeric value");
        let msg = Warning::ExpectedNumericValue {
            arg: OsString::from("a\x7fb"),
        }
        .message();
        assert_eq!(
            msg.as_encoded_bytes(),
            b"'a\\177b': expected a numeric value"
        );
        let msg = Warning::ExpectedNumericValue {
            arg: OsString::from("a'b"),
        }
        .message();
        assert_eq!(msg.as_encoded_bytes(), b"'a\\'b': expected a numeric value");
        let msg = Warning::ExpectedNumericValue {
            arg: OsString::from_vec(vec![0xc3, 0xa1]),
        }
        .message();
        assert_eq!(
            msg.as_encoded_bytes(),
            b"'\\303\\241': expected a numeric value"
        );
        // Non-ASCII digits (probed C locale): GNU leaves them raw and
        // octal-escapes the bytes; same value and warning here.
        let out = run("%d", &["۰۱۲"]);
        assert_eq!(out.bytes.as_bytes(), b"0");
        assert_eq!(
            out.warnings[0].message().as_encoded_bytes(),
            b"'\\333\\260\\333\\261\\333\\262': expected a numeric value"
        );
        // Character-constant trailing bytes stay raw (probed C locale).
        let arg = OsStr::from_bytes(b"\"a\xff");
        let out = call(OsStr::new("%d"), &[arg], &Options::default()).unwrap();
        assert_eq!(out.bytes.as_bytes(), b"97");
        assert_eq!(
            out.warnings[0].message().as_encoded_bytes(),
            b"warning: \xff: character(s) following character constant have been ignored"
        );
    }
}
