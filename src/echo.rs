//! lcu-echo: dependency-free core for `echo`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

//! lcu-echo: dependency-free core for `echo`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.
//!
//! This is the single-operand core: leading CLI words are already
//! split into `args` and flags by the caller. Flag mapping:
//! `-n` becomes `Options.trailing_newline = false`,
//! `-e`/`-E` become `Options.interpret_escapes`. Option *parsing*
//! itself (combined flags like `-ne`, first-non-option stops parsing,
//! no `--` handling, `POSIXLY_CORRECT` variants, `--help`/`--version`)
//! belongs to the CLI layer: it reads `argv`/env, which this pure
//! function never touches.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// Error type for `echo`.
///
/// `echo` is infallible: every input maps to an output, so this enum
/// has no variants.
#[derive(Debug)]
pub enum Error {}

/// Options for `echo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Interpret backslash escapes (`-e`). When false (`-E`, the GNU
    /// default without `POSIXLY_CORRECT`), backslashes pass through
    /// literally, including `\c`.
    pub interpret_escapes: bool,
    /// Emit the trailing newline (`-n` sets this to false). A `\c`
    /// escape in the input forces this off regardless.
    pub trailing_newline: bool,
}

impl Default for Options {
    /// GNU default without `POSIXLY_CORRECT`: escapes off, newline on.
    fn default() -> Self {
        Options {
            interpret_escapes: false,
            trailing_newline: true,
        }
    }
}

/// The computed output: `text` holds the exact bytes (escapes may
/// produce non-UTF8 bytes, hence `OsString`), and `trailing_newline`
/// tells the caller whether to append the line terminator.
#[derive(Debug, PartialEq, Eq)]
pub struct Output {
    pub text: OsString,
    pub trailing_newline: bool,
}

/// Entry point for `echo`. Explicit params only, no implicit state.
///
/// Joins `args` with single spaces, interpreting backslash escapes
/// per GNU when `options.interpret_escapes` is set. A `\c` escape ends
/// output immediately and clears `trailing_newline`.
pub fn call(args: &[&OsStr], options: &Options) -> Result<Output, Error> {
    let mut out: Vec<u8> = Vec::new();
    let mut stopped = false;
    if options.interpret_escapes {
        'args: for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            let bytes = arg.as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                let c = bytes[j];
                if c != b'\\' || j + 1 >= bytes.len() {
                    out.push(c);
                    j += 1;
                    continue;
                }
                match bytes[j + 1] {
                    b'a' => out.push(0x07),
                    b'b' => out.push(0x08),
                    b'e' => out.push(0x1b),
                    b'f' => out.push(0x0c),
                    b'n' => out.push(0x0a),
                    b'r' => out.push(0x0d),
                    b't' => out.push(0x09),
                    b'v' => out.push(0x0b),
                    b'\\' => out.push(b'\\'),
                    b'c' => {
                        stopped = true;
                        break 'args;
                    }
                    b'x' => {
                        if j + 2 < bytes.len() && is_hex(bytes[j + 2]) {
                            let mut v = hex_val(bytes[j + 2]);
                            j += 3;
                            if j < bytes.len() && is_hex(bytes[j]) {
                                v = v * 16 + hex_val(bytes[j]);
                                j += 1;
                            }
                            out.push(v);
                            continue;
                        }
                        out.push(b'\\');
                        out.push(b'x');
                    }
                    b'0'..=b'7' => {
                        // Up to 3 octal digits, or 4 when leading with
                        // \0 (mirrors echo.c: case '0' consumes one
                        // digit before falling through to two more).
                        let max = if bytes[j + 1] == b'0' { 4 } else { 3 };
                        let mut v: u32 = 0;
                        let mut k = j + 1;
                        for _ in 0..max {
                            if k < bytes.len() && (b'0'..=b'7').contains(&bytes[k]) {
                                v = v * 8 + u32::from(bytes[k] - b'0');
                                k += 1;
                            } else {
                                break;
                            }
                        }
                        out.push((v & 0xff) as u8);
                        j = k;
                        continue;
                    }
                    other => {
                        out.push(b'\\');
                        out.push(other);
                    }
                }
                j += 2;
            }
        }
    } else {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(arg.as_bytes());
        }
    }
    Ok(Output {
        text: OsString::from_vec(out),
        trailing_newline: options.trailing_newline && !stopped,
    })
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    fn opts(interpret: bool, newline: bool) -> Options {
        Options {
            interpret_escapes: interpret,
            trailing_newline: newline,
        }
    }

    fn run(args: &[&str], options: &Options) -> (Vec<u8>, bool) {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let out = call(&args, options).unwrap();
        (out.text.as_bytes().to_vec(), out.trailing_newline)
    }

    #[test]
    fn default_options_match_gnu_default() {
        assert_eq!(opts(false, true), Options::default());
    }

    #[test]
    fn joins_args_with_single_space() {
        // echo.sh: `--` has no end-of-options meaning, echoes literally.
        assert_eq!(run(&["--", "foo"], &opts(false, true)).0, b"-- foo");
        assert_eq!(run(&["a", "b", "c"], &opts(false, true)).0, b"a b c");
    }

    #[test]
    fn no_args_yields_empty_text() {
        assert_eq!(run(&[], &opts(false, true)), (b"".to_vec(), true));
    }

    #[test]
    fn without_interpret_escapes_pass_through() {
        // GNU default without -e: backslashes (even \c) are literal.
        assert_eq!(run(&[r"foo\n"], &opts(false, true)).0, b"foo\\n");
        assert_eq!(run(&[r"a\cb"], &opts(false, true)), (b"a\\cb".to_vec(), true));
    }

    #[test]
    fn interprets_letter_escapes() {
        // echo.sh line 83: od expectation 07 08 1b 0c 0a 0d 09 0b.
        assert_eq!(
            run(&[r"\a\b\e\f\n\r\t\v"], &opts(true, false)).0,
            b"\x07\x08\x1b\x0c\x0a\x0d\x09\x0b"
        );
    }

    #[test]
    fn interprets_hex_escapes() {
        // echo.sh line 87: \x4a..\x4f == JKLMNO, both cases.
        assert_eq!(
            run(&[r"\x4a\x4b\x4c\x4d\x4e\x4f\x4A\x4B\x4C\x4D\x4E\x4F"], &opts(true, false)).0,
            b"JKLMNOJKLMNO"
        );
        // One hex digit is enough.
        assert_eq!(run(&[r"\x41"], &opts(true, false)).0, b"A");
    }

    #[test]
    fn incomplete_hex_escape_is_literal() {
        // echo.sh lines 31-34: unlike printf, kept as-is.
        assert_eq!(run(&[r"\x"], &opts(true, false)).0, b"\\x");
        assert_eq!(run(&["a\\xzb"], &opts(true, false)).0, b"a\\xzb");
    }

    #[test]
    fn octal_escapes_allow_four_digits() {
        // echo.sh lines 25-29: \x1b \e \33 \033 \0033 are all 0x1B.
        assert_eq!(
            run(&[r"\x1b"], &opts(true, false)).0,
            run(&[r"\e"], &opts(true, false)).0
        );
        assert_eq!(run(&[r"\e"], &opts(true, false)).0, b"\x1b");
        assert_eq!(run(&[r"\33"], &opts(true, false)).0, b"\x1b");
        assert_eq!(run(&[r"\033"], &opts(true, false)).0, b"\x1b");
        assert_eq!(run(&[r"\0033"], &opts(true, false)).0, b"\x1b");
        // Bare \0 is a single NUL byte.
        assert_eq!(run(&[r"\0"], &opts(true, false)).0, b"\x00");
    }

    #[test]
    fn backslash_c_stops_output() {
        // echo.sh lines 45-48: `foo\n\cbar` yields `foo\n`, no newline.
        assert_eq!(
            run(&[r"foo\n\cbar"], &opts(true, true)),
            (b"foo\n".to_vec(), false)
        );
        // Everything after \c is dropped, including later args.
        assert_eq!(
            run(&["a\\c", "b"], &opts(true, true)),
            (b"a".to_vec(), false)
        );
    }

    #[test]
    fn no_trailing_newline_option() {
        // -n: text intact, terminator flag off.
        assert_eq!(
            run(&["foo"], &opts(false, false)),
            (b"foo".to_vec(), false)
        );
    }

    #[test]
    fn unknown_escapes_stay_literal() {
        // echo.sh lines 99-119: no \", \u, \U escapes (unlike printf).
        assert_eq!(run(&["a\\\"b"], &opts(true, false)).0, b"a\\\"b");
        assert_eq!(run(&["a\\u0041b"], &opts(true, false)).0, b"a\\u0041b");
        assert_eq!(run(&["\\u"], &opts(true, false)).0, b"\\u");
        assert_eq!(run(&["\\U0000004"], &opts(true, false)).0, b"\\U0000004");
        assert_eq!(run(&["\\d"], &opts(true, false)).0, b"\\d");
    }

    #[test]
    fn trailing_backslash_is_literal() {
        assert_eq!(run(&["foo\\"], &opts(true, false)).0, b"foo\\");
    }

    #[test]
    fn double_backslash_yields_one() {
        // echo.sh line 70.
        assert_eq!(run(&["\\\\"], &opts(true, false)).0, b"\\");
    }

    #[test]
    fn escapes_can_emit_non_utf8_bytes() {
        // From echo.c hex logic: byte-exact output, not lossy.
        let (bytes, _) = run(&["\\xff"], &opts(true, false));
        assert_eq!(bytes, vec![0xff]);
        let out = call(&[OsStr::new("\\xff")], &opts(true, false)).unwrap();
        assert_eq!(out.text, OsString::from_vec(vec![0xff]));
    }
}
