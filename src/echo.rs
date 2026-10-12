//! lcu-echo: dependency-free core for `echo`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.
//!
//! The CLI owns argv parsing, `--help`/`--version`, and diagnostics.
//! This pure core receives the already-parsed operand list and only the
//! two option flags that affect output: whether backslash escapes are in
//! effect and whether the trailing newline is suppressed.
//!
//! Behavior modeled on installed GNU coreutils 9.10 under `LC_ALL=C`:
//! - Default is no escape processing.
//! - `-e` enables escape processing.
//! - `-E` disables escape processing.
//! - `-n` suppresses the trailing newline.
//! - `echo` has no `\\u`/`\\U`/`\\"` escapes.
//! - Octal accepts up to 3 digits and the `\\0NNN` GNU 4-digit form.
//! - Incomplete `\\x` is emitted literally.
//! - `\\c` stops all further output; exit stays success.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;

/// Options for `echo`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// True when backslash escapes should be interpreted.
    pub escapes: bool,
    /// True when the trailing newline should be suppressed.
    pub no_newline: bool,
}

/// Error type for `echo`.
///
/// `echo` never fails in the core: every operand list maps to output
/// bytes and GNU echo exits 0 (write failures happen in the CLI), so
/// this enum has no variants.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {}

/// Entry point for `echo`. Explicit params only, no implicit state.
///
/// `args` is the operand list *after* CLI option parsing.
pub fn call(args: &[&OsStr], options: &Options) -> Result<OsString, Error> {
    let mut out = Vec::new();
    let mut stopped = false;
    for (i, arg) in args.iter().enumerate() {
        if stopped {
            break;
        }
        if i > 0 {
            out.push(b' ');
        }
        if options.escapes {
            stopped = emit_echo_arg(&mut out, arg.as_encoded_bytes());
        } else {
            out.extend_from_slice(arg.as_encoded_bytes());
        }
    }
    // GNU: `\c` stops all further output, including the trailing newline.
    if !stopped && !options.no_newline {
        out.push(b'\n');
    }
    Ok(OsString::from_vec(out))
}

/// Emit one argument's bytes, returning true if `\c` stopped output.
fn emit_echo_arg(out: &mut Vec<u8>, s: &[u8]) -> bool {
    let mut j = 0;
    while j < s.len() {
        let c = s[j];
        if c == b'\\' && j + 1 < s.len() {
            let (emit, next, stop) = parse_echo_escape(s, j + 1);
            out.extend_from_slice(&emit);
            if stop {
                return true;
            }
            j = next;
        } else {
            out.push(c);
            j += 1;
        }
    }
    false
}

fn parse_echo_escape(s: &[u8], pos: usize) -> (Vec<u8>, usize, bool) {
    if pos >= s.len() {
        return (vec![b'\\'], pos, false);
    }
    match s[pos] {
        b'a' => (vec![0x07], pos + 1, false),
        b'b' => (vec![0x08], pos + 1, false),
        b'c' => (Vec::new(), pos + 1, true),
        b'e' => (vec![0x1b], pos + 1, false),
        b'f' => (vec![0x0c], pos + 1, false),
        b'n' => (vec![0x0a], pos + 1, false),
        b'r' => (vec![0x0d], pos + 1, false),
        b't' => (vec![0x09], pos + 1, false),
        b'v' => (vec![0x0b], pos + 1, false),
        b'\\' => (vec![b'\\'], pos + 1, false),
        b'0' | b'1' | b'2' | b'3' | b'4' | b'5' | b'6' | b'7' => parse_echo_octal(s, pos),
        b'x' => parse_echo_hex(s, pos + 1),
        _ => (vec![b'\\', s[pos]], pos + 1, false),
    }
}

/// Parse an octal escape starting at `pos`, where `s[pos]` is the first
/// octal digit. GNU caps octal escapes by digit count: a leading `0`
/// allows 4 digits total (the `\0NNN` form), any other first digit
/// allows 3. The accumulated value is masked to 8 bits; digits beyond
/// the cap stay literal. All limits probed against GNU 9.10.
fn parse_echo_octal(s: &[u8], pos: usize) -> (Vec<u8>, usize, bool) {
    let max = if s[pos] == b'0' { 4 } else { 3 };
    let mut p = pos;
    let mut val: u32 = 0;
    let mut digits = 0;
    while digits < max {
        match s.get(p) {
            Some(&c) if (b'0'..=b'7').contains(&c) => {
                val = val * 8 + u32::from(c - b'0');
                p += 1;
                digits += 1;
            }
            _ => break,
        }
    }
    (vec![(val & 0xff) as u8], p, false)
}

fn parse_echo_hex(s: &[u8], mut pos: usize) -> (Vec<u8>, usize, bool) {
    let mut val: u32 = 0;
    let mut n = 0;
    while n < 2 && pos < s.len() && s[pos].is_ascii_hexdigit() {
        val = val * 16 + u32::from(hex_val(s[pos]));
        pos += 1;
        n += 1;
    }
    if n == 0 {
        // GNU: `\x` with no hex digits is emitted literally.
        return (vec![b'\\', b'x'], pos, false);
    }
    (vec![(val & 0xff) as u8], pos, false)
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

    fn run(args: &[&str], escapes: bool, no_newline: bool) -> OsString {
        call(
            &args.iter().map(OsStr::new).collect::<Vec<_>>(),
            &Options {
                escapes,
                no_newline,
            },
        )
        .unwrap()
    }

    fn out_bytes(args: &[&str], escapes: bool, no_newline: bool) -> Vec<u8> {
        run(args, escapes, no_newline).as_encoded_bytes().to_vec()
    }

    #[test]
    fn empty_args_print_newline() {
        assert_eq!(out_bytes(&[], false, false), b"\n");
    }

    #[test]
    fn one_arg_prints_with_newline() {
        assert_eq!(out_bytes(&["hi"], false, false), b"hi\n");
    }

    #[test]
    fn two_args_space_separated() {
        assert_eq!(out_bytes(&["a", "b"], false, false), b"a b\n");
    }

    #[test]
    fn n_suppresses_newline() {
        assert_eq!(out_bytes(&["hi"], false, true), b"hi");
    }

    #[test]
    fn e_enables_escapes() {
        assert_eq!(out_bytes(&["\\a"], true, false), b"\x07\n");
    }

    #[test]
    fn e_disables_default_escape_suppression() {
        assert_eq!(out_bytes(&["\\n"], true, false), b"\n\n");
    }

    #[test]
    fn capital_e_disables_escapes() {
        assert_eq!(out_bytes(&["\\n"], false, false), b"\\n\n");
    }

    #[test]
    fn n_e_escape_no_newline() {
        assert_eq!(out_bytes(&["\\a"], true, true), b"\x07");
    }

    #[test]
    fn escape_backslash() {
        assert_eq!(out_bytes(&["\\\\"], true, false), b"\\\n");
    }

    #[test]
    fn escape_tab() {
        assert_eq!(out_bytes(&["\\t"], true, false), b"\t\n");
    }

    #[test]
    fn escape_vertical_tab() {
        assert_eq!(out_bytes(&["\\v"], true, false), b"\x0b\n");
    }

    #[test]
    fn escape_form_feed() {
        assert_eq!(out_bytes(&["\\f"], true, false), b"\x0c\n");
    }

    #[test]
    fn escape_carriage_return() {
        assert_eq!(out_bytes(&["\\r"], true, false), b"\r\n");
    }

    #[test]
    fn escape_escape() {
        assert_eq!(out_bytes(&["\\e"], true, false), b"\x1b\n");
    }

    #[test]
    fn escape_alert() {
        assert_eq!(out_bytes(&["\\a"], true, false), b"\x07\n");
    }

    #[test]
    fn escape_backspace() {
        assert_eq!(out_bytes(&["\\b"], true, false), b"\x08\n");
    }

    #[test]
    fn escape_octal_short() {
        // Probed against GNU 9.10: `\0101` reads digits "101" -> 65.
        assert_eq!(out_bytes(&["\\0101"], true, false), b"A\n");
    }

    #[test]
    fn escape_octal_short_777() {
        assert_eq!(out_bytes(&["\\777"], true, false), b"\xff\n");
    }

    #[test]
    fn escape_octal_zero_4_digits() {
        assert_eq!(out_bytes(&["\\0100"], true, false), b"@\n");
    }

    // GNU caps octal escapes by digit count, not by value: a leading
    // `0` allows 4 octal digits total, any other first digit allows 3.
    // The value is masked to 8 bits; excess digits stay literal.
    // All expectations probed against GNU 9.10 (`LC_ALL=C echo -e`).

    #[test]
    fn escape_octal_3_digit_cap_leaves_rest_literal() {
        // `\1000` -> `\100` (64) + literal `0`.
        assert_eq!(out_bytes(&["\\1000"], true, false), b"@0\n");
        // `\1000000` -> `\100` + four literal zeros.
        assert_eq!(out_bytes(&["\\1000000"], true, false), b"@0000\n");
    }

    #[test]
    fn escape_octal_zero_4_digit_cap_leaves_rest_literal() {
        // `\012345` -> `\0123` (83 = 'S') + literal `45`.
        assert_eq!(out_bytes(&["\\012345"], true, false), b"S45\n");
        // `\00777` -> `\0077` (63) + literal `7`.
        assert_eq!(out_bytes(&["\\00777"], true, false), b"\x3f7\n");
        // `\01777` -> `\0177` (127) + literal `7`.
        assert_eq!(out_bytes(&["\\01777"], true, false), b"\x7f7\n");
        // `\00001234` -> `\0000` (0) + literal `1234`.
        assert_eq!(out_bytes(&["\\00001234"], true, false), b"\x001234\n");
        // `\00000` -> `\0000` (0) + literal `0`.
        assert_eq!(out_bytes(&["\\00000"], true, false), b"\x000\n");
    }

    #[test]
    fn escape_octal_masks_to_8_bits_after_full_count() {
        // `\402` consumes all 3 digits (258) then masks -> 2.
        assert_eq!(out_bytes(&["\\402"], true, false), b"\x02\n");
        // `\4000` -> `\400` (256 masked to 0) + literal `0`.
        assert_eq!(out_bytes(&["\\4000"], true, false), b"\x000\n");
        // `\0400` -> all 4 digits (256 masked to 0).
        assert_eq!(out_bytes(&["\\0400"], true, false), b"\x00\n");
        // `\7777` -> `\777` (511 masked to 255) + literal `7`.
        assert_eq!(out_bytes(&["\\7777"], true, false), b"\xff7\n");
        // `\01777` also proves the 4-digit form masks: `\0177` = 127.
        assert_eq!(out_bytes(&["\\0033"], true, false), b"\x1b\n");
    }

    #[test]
    fn escape_octal_stops_at_non_octal_digit() {
        // `\18` -> `\1` + literal `8`; `\48` -> `\4` + literal `8`.
        assert_eq!(out_bytes(&["\\18"], true, false), b"\x018\n");
        assert_eq!(out_bytes(&["\\48"], true, false), b"\x048\n");
        // `\08` -> `\0` + literal `8`.
        assert_eq!(out_bytes(&["\\08"], true, false), b"\x008\n");
    }

    #[test]
    fn escape_hex_2_digit_cap_leaves_rest_literal() {
        // `\xFFf` -> 0xFF + literal `f`; `\x0041` -> 0x00 + literal `41`.
        assert_eq!(out_bytes(&["\\xFFf"], true, false), [0xff, b'f', b'\n']);
        assert_eq!(out_bytes(&["\\x0041"], true, false), b"\x0041\n");
    }

    #[test]
    fn escape_nul() {
        assert_eq!(out_bytes(&["\\0"], true, false), b"\x00\n");
    }

    #[test]
    fn escape_nul_00() {
        assert_eq!(out_bytes(&["\\00"], true, false), b"\x00\n");
    }

    #[test]
    fn escape_nul_000() {
        assert_eq!(out_bytes(&["\\000"], true, false), b"\x00\n");
    }

    #[test]
    fn escape_hex_41() {
        assert_eq!(out_bytes(&["\\x41"], true, false), b"A\n");
    }

    #[test]
    fn escape_hex_case_mixed() {
        assert_eq!(
            out_bytes(
                &["\\x4a\\x4b\\x4c\\x4d\\x4e\\x4f\\x4A\\x4B\\x4C\\x4D\\x4E\\x4F",],
                true,
                false,
            ),
            b"JKLMNOJKLMNO\n"
        );
    }

    #[test]
    fn escape_c_stops_output() {
        assert_eq!(out_bytes(&["foo\\n\\cbar"], true, false), b"foo\n");
    }

    #[test]
    fn escape_c_at_start_stops_output() {
        assert_eq!(out_bytes(&["\\cfoo"], true, false), b"");
    }

    #[test]
    fn escape_c_with_n_and_no_newline() {
        assert_eq!(out_bytes(&["\\cfoo"], true, true), b"");
    }

    #[test]
    fn escape_c_with_n_no_newline_on_stop() {
        assert_eq!(out_bytes(&["a\\cb"], true, true), b"a");
    }

    #[test]
    fn incomplete_hex_is_literal() {
        assert_eq!(out_bytes(&["\\x"], true, false), b"\\x\n");
    }

    #[test]
    fn incomplete_hex_after_text_is_literal() {
        assert_eq!(out_bytes(&["foo\\x bar"], true, false), b"foo\\x bar\n");
    }

    #[test]
    fn u_escape_is_literal() {
        assert_eq!(out_bytes(&["\\u0041"], true, false), b"\\u0041\n");
    }

    #[test]
    fn capital_u_escape_is_literal() {
        assert_eq!(out_bytes(&["\\U00000041"], true, false), b"\\U00000041\n");
    }

    #[test]
    fn backslash_quote_is_literal() {
        assert_eq!(out_bytes(&["\\\""], true, false), b"\\\"\n");
    }

    #[test]
    fn invalid_octal_digit_is_literal() {
        assert_eq!(out_bytes(&["\\08"], true, false), b"\x008\n");
    }

    #[test]
    fn invalid_octal_digit_after_text_is_literal() {
        assert_eq!(out_bytes(&["\\8"], true, false), b"\\8\n");
    }

    #[test]
    fn escape_c_stops_further_args() {
        // Probed against GNU 9.10: `echo -e 'a\cb' c` prints just `a`.
        assert_eq!(out_bytes(&["a\\cb", "c"], true, false), b"a");
    }

    #[test]
    fn multiple_args_space_separated_with_escapes() {
        assert_eq!(out_bytes(&["a\\nb", "c"], true, false), b"a\nb c\n");
    }
}
