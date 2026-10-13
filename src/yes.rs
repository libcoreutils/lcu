//! lcu-yes: dependency-free core for `yes`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.
//!
//! The CLI owns `--help`/`--version`, option parsing (unknown options
//! such as `yes -n` fail at the argv layer), diagnostics, and the
//! infinite write loop: `yes` emits its line forever, which a pure
//! function cannot do, so `call` computes exactly one period of that
//! output — the caller repeats it (`loop { stdout.write_all(&line)? }`),
//! as uutils does internally.
//!
//! Behavior matches uutils (tests/by-util/test_yes.rs) and the
//! reference coreutils 9.10 binary (probes under `LC_ALL=C`):
//! - No operands: the period is `y\n`.
//! - Operands are joined with single spaces and terminated by `\n`
//!   (empty operands keep their separator: `a '' b` yields `a  b\n`).
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
/// Returns one period of the infinite output `yes` produces
/// for `args` (after CLI option parsing), newline included.
pub fn call(args: &[&OsStr]) -> Result<OsString, Error> {
    let mut out = Vec::new();
    if args.is_empty() {
        // No operands -> the line is "y" (uutils test_simple).
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
        let refs: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        call(&refs).unwrap().into_vec()
    }

    #[test]
    fn no_operands_yields_y() {
        // uutils test_simple + probed against coreutils 9.10.
        assert_eq!(period(&[]), b"y\n");
    }

    #[test]
    fn single_operand() {
        // Probed against coreutils 9.10: `yes hi` emits "hi\n".
        assert_eq!(period(&["hi"]), b"hi\n");
    }

    #[test]
    fn operands_joined_with_spaces() {
        // uutils test_args: `yes a bar c` → "a bar c" lines.
        assert_eq!(period(&["a", "bar", "c"]), b"a bar c\n");
    }

    #[test]
    fn empty_operand_only_yields_newline() {
        // Probed against coreutils 9.10: `yes ''` emits empty lines.
        assert_eq!(period(&[""]), b"\n");
    }

    #[test]
    fn empty_operand_keeps_its_separator() {
        // Probed against coreutils 9.10: `yes a '' b` → "a  b\n".
        assert_eq!(period(&["a", "", "b"]), b"a  b\n");
    }

    #[test]
    fn escapes_are_literal() {
        // Probed against coreutils 9.10: backslashes pass through.
        assert_eq!(period(&["a\\nb"]), b"a\\nb\n");
    }

    #[test]
    fn option_like_operands_are_verbatim() {
        // Unknown-option rejection (uutils test_invalid_arg) happens
        // in argv parsing; the core never sees options.
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
    fn non_utf8_operands_join_with_spaces() {
        // uutils test_non_utf8: raw bytes join exactly like text.
        let raw = OsStr::from_bytes(b"\xbf\xff\xee");
        let got = call(&[raw, OsStr::new("bar")]).unwrap();
        assert_eq!(got.as_encoded_bytes(), b"\xbf\xff\xee bar\n");
    }

    #[test]
    fn long_operand_is_not_truncated() {
        // uutils test_long_input + test_long_line_exceeds_pipe_capacity:
        // the period is the full join at any size; chunking the
        // infinite write loop is the caller's job.
        let big = "abcdef".repeat(14000);
        let refs: Vec<&OsStr> = vec![OsStr::new(&big)];
        let got = call(&refs).unwrap().into_vec();
        let mut expected = big.into_bytes();
        expected.push(b'\n');
        assert_eq!(got, expected);
    }

    #[test]
    fn repeated_periods_match_output_stream() {
        // uutils test_simple: output starts with "y\ny\ny\ny\n".
        let line = period(&[]);
        let stream: Vec<u8> = line.iter().copied().cycle().take(8).collect();
        assert_eq!(stream, b"y\ny\ny\ny\n");
    }
}
