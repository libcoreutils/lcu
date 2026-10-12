//! lcu-yes: dependency-free core for `yes`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.
//!
//! The CLI owns `--help`/`--version`, option parsing (GNU accepts only
//! the standard options and rejects others, e.g. `yes -n` fails at the
//! argv layer), diagnostics, and the infinite write loop: GNU `yes`
//! emits its line forever, which a pure function cannot do, so `call`
//! computes exactly one period of that output — the caller repeats it
//! (`loop { stdout.write_all(&line)? }`), as uutils does internally.
//!
//! Behavior modeled on installed GNU coreutils 9.10 under `LC_ALL=C`:
//! - No operands: the period is `y\n`.
//! - Operands are joined with single spaces and terminated by `\n`
//!   (`yes.c` appends each operand plus a space, then overwrites the
//!   last byte with the newline, so empty operands keep their
//!   separator: `a '' b` yields `a  b\n`).
//! - Operands are copied verbatim: no escape processing, no
//!   option-like handling, non-UTF-8 bytes pass through.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;

/// Error type for `yes`.
///
/// Joining operand bytes cannot fail, so this enum has no variants.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {}

/// Entry point for `yes`. Explicit params only, no implicit state.
///
/// Returns one period of the infinite output GNU `yes` would produce
/// for `args` (after CLI option parsing), newline included.
pub fn call(args: &[&OsStr]) -> Result<OsString, Error> {
    let mut out = Vec::new();
    if args.is_empty() {
        // GNU yes.c: no operands -> the line is "y".
        out.extend_from_slice(b"y");
    } else {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(arg.as_encoded_bytes());
        }
    }
    out.push(b'\n');
    Ok(OsString::from_vec(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    fn period(args: &[&str]) -> Vec<u8> {
        let refs: Vec<&OsStr> = args.iter().map(|s| OsStr::new(s)).collect();
        call(&refs).unwrap().into_vec()
    }

    #[test]
    fn no_operands_yields_y() {
        // Probed GNU 9.10: `yes` emits "y\n".
        assert_eq!(period(&[]), b"y\n");
    }

    #[test]
    fn single_operand() {
        // Probed GNU 9.10: `yes hi` emits "hi\n".
        assert_eq!(period(&["hi"]), b"hi\n");
    }

    #[test]
    fn operands_joined_with_spaces() {
        // Probed GNU 9.10 + uutils test_args: `yes a bar c` → "a bar c".
        assert_eq!(period(&["a", "bar", "c"]), b"a bar c\n");
    }

    #[test]
    fn empty_operand_only_yields_newline() {
        // Probed GNU 9.10: `yes ''` emits empty lines.
        assert_eq!(period(&[""]), b"\n");
    }

    #[test]
    fn empty_operand_keeps_its_separator() {
        // Probed GNU 9.10: `yes a '' b` → "a  b\n" (double space).
        assert_eq!(period(&["a", "", "b"]), b"a  b\n");
    }

    #[test]
    fn escapes_are_literal() {
        // Probed GNU 9.10: `yes 'a\nb'` → literal "a\nb" bytes.
        assert_eq!(period(&["a\\nb"]), b"a\\nb\n");
    }

    #[test]
    fn option_like_operands_are_verbatim() {
        // GNU's `yes -n` rejection ("invalid option") happens in
        // argv parsing; the core never sees options.
        assert_eq!(period(&["-n"]), b"-n\n");
    }

    #[test]
    fn non_utf8_operand_passes_through() {
        // uutils test_non_utf8: raw bytes are joined untouched.
        let raw = OsStr::from_bytes(b"\xbf\xff\xee");
        let got = call(&[raw]).unwrap();
        assert_eq!(got.as_encoded_bytes(), b"\xbf\xff\xee\n");
    }

    #[test]
    fn repeated_periods_match_gnu_stream() {
        // uutils test_simple: output starts with "y\ny\ny\ny\n".
        let line = period(&[]);
        let stream: Vec<u8> = line.iter().copied().cycle().take(8).collect();
        assert_eq!(stream, b"y\ny\ny\ny\n");
    }
}
