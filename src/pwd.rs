//! lcu-pwd: dependency-free core for `pwd`.
//! No process::exit, no subprocess, no implicit cwd/env reads, no stdout writes.
//!
//! The CLI owns `--help`/`--version`, `-L`/`-P` parsing, the
//! "ignoring non-option arguments" warning, and reading argv/env:
//! this pure function receives the current directory, the raw `PWD`
//! value, and the mode explicitly, and computes what `pwd` prints:
//! - physical mode (the default, `-P`): `cwd` itself, which must be
//!   the physical current directory (`std::env::current_dir()` on Unix)
//! - logical mode (`-L`, or the default when `POSIXLY_CORRECT` is set):
//!   `PWD` is used only when it is absolute, has no `.`/`..`
//!   components, and names the same directory (device + inode) as
//!   `cwd`; otherwise fall back to `cwd`

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Options for `pwd`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Options {
    /// Raw `PWD` environment value, if set (read by the CLI).
    pub pwd: Option<OsString>,
    /// True for logical mode: `-L`, or `-P` not given while
    /// `POSIXLY_CORRECT` is set. False for physical mode (the default).
    pub logical: bool,
}

/// Error type for `pwd`.
///
/// Once the caller supplies the current directory, `pwd` is infallible:
/// an unacceptable `PWD` falls back to `cwd`, so this enum has no variants.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {}

/// Entry point for `pwd`. Explicit params only, no implicit state.
///
/// `cwd` is the process's current directory, physical on Unix (e.g.
/// from `std::env::current_dir()`). Returns the path `pwd` prints
/// for the given mode.
pub fn call(cwd: &Path, options: &Options) -> Result<PathBuf, Error> {
    if options.logical {
        if let Some(pwd) = options.pwd.as_deref() {
            if pwd_is_logical(pwd, cwd) {
                return Ok(PathBuf::from(pwd));
            }
        }
    }
    Ok(cwd.to_path_buf())
}

/// Return whether `pwd -L` accepts `PWD` value `pwd` for output,
/// given the physical current directory `cwd`.
///
/// Logical-mode acceptance rule: textual validation first (absolute,
/// no `.` or `..` components), then a stat comparison against `cwd`
/// (`PWD` must name the same directory, device + inode).
fn pwd_is_logical(pwd: &OsStr, cwd: &Path) -> bool {
    let bytes = pwd.as_encoded_bytes();
    if bytes.first() != Some(&b'/') {
        return false;
    }
    if bytes
        .split(|&b| b == b'/')
        .any(|part| part == b"." || part == b"..")
    {
        return false;
    }
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(pwd), std::fs::metadata(cwd)) {
        (Ok(st1), Ok(st2)) => st1.dev() == st2.dev() && st1.ino() == st2.ino(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(pwd: Option<&str>, logical: bool) -> Options {
        Options {
            pwd: pwd.map(OsString::from),
            logical,
        }
    }

    #[test]
    fn physical_default_prints_cwd() {
        // uutils test_default: without POSIXLY_CORRECT, cwd is printed as-is.
        assert_eq!(
            call(Path::new("/"), &Options::default()).unwrap(),
            Path::new("/")
        );
    }

    #[test]
    fn physical_mode_ignores_pwd() {
        // `-P` never consults PWD, even a valid one.
        assert_eq!(
            call(Path::new("/dev"), &opts(Some("/"), false)).unwrap(),
            Path::new("/dev")
        );
    }

    #[test]
    fn logical_without_pwd_falls_back_to_cwd() {
        assert_eq!(
            call(Path::new("/dev"), &opts(None, true)).unwrap(),
            Path::new("/dev")
        );
    }

    #[test]
    fn logical_accepts_pwd_naming_cwd() {
        assert_eq!(
            call(Path::new("/"), &opts(Some("/"), true)).unwrap(),
            Path::new("/")
        );
    }

    #[test]
    fn logical_rejects_nonexistent_pwd() {
        // uutils test_nonexistent_logical
        assert_eq!(
            call(Path::new("/dev"), &opts(Some("/somefakedir"), true)).unwrap(),
            Path::new("/dev")
        );
    }

    #[test]
    fn logical_rejects_other_directory() {
        // uutils test_wrong_logical: absolute, clean, but not cwd.
        assert_eq!(
            call(Path::new("/dev"), &opts(Some("/"), true)).unwrap(),
            Path::new("/dev")
        );
    }

    #[test]
    fn logical_rejects_relative_pwd() {
        // uutils test_relative_logical
        assert_eq!(
            call(Path::new("/dev"), &opts(Some("."), true)).unwrap(),
            Path::new("/dev")
        );
    }

    #[test]
    fn logical_rejects_dot_components() {
        // uutils test_redundant_logical and friends: any `.`/`..`
        // component rejects PWD, even one that still names cwd
        // ("/dev/." would stat-match but must fall back).
        for pwd in ["/.", "/a/./b", "/a/../b", "/a/b/..", "/a/b/.", "/dev/."] {
            assert_eq!(
                call(Path::new("/dev"), &opts(Some(pwd), true)).unwrap(),
                Path::new("/dev"),
                "PWD={pwd} should be rejected"
            );
        }
    }

    #[test]
    fn logical_accepts_symlinked_pwd() {
        // uutils test_symlinked_logical: PWD may differ textually from
        // the physical cwd as long as it names the same directory.
        let base = std::env::temp_dir().join(format!("lcu-pwd-test-{}", std::process::id()));
        let real = base.join("real");
        let link = base.join("link");
        std::fs::create_dir_all(&real).unwrap();
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let got = call(&real, &opts(Some(link.to_str().unwrap()), true)).unwrap();
        assert_eq!(got, link);

        // Physical mode resolves to the real path instead.
        let got = call(&real, &opts(Some(link.to_str().unwrap()), false)).unwrap();
        assert_eq!(got, real);

        let _ = std::fs::remove_dir_all(&base);
    }
}
