//! lcu-basename: dependency-free core for `basename`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Error type for `basename`.
///
/// `basename` is infallible over `&str` input: every path maps to a
/// name, so this enum has no variants.
#[derive(Debug)]
pub enum Error {}

/// Options for `basename`.
#[derive(Debug, Default)]
pub struct Options {
    /// Trailing suffix to strip from the base name, e.g. `".txt"`.
    /// The suffix is kept when it does not match, or when it equals
    /// the whole base name. `None` disables stripping.
    pub suffix: Option<String>,
}

/// Entry point for `basename`. Explicit params only, no implicit state.
///
/// Returns the final component of `path` after stripping trailing
/// slashes, with `options.suffix` removed when applicable.
pub fn call(path: &str, options: &Options) -> Result<String, Error> {
    let stripped = path.trim_end_matches('/');
    if stripped.is_empty() {
        if path.is_empty() {
            return Ok(String::new());
        }
        return Ok("/".to_owned());
    }
    let mut base = stripped.rsplit('/').next().unwrap_or(stripped);
    if let Some(suffix) = &options.suffix {
        if base != suffix.as_str() {
            if let Some(without) = base.strip_suffix(suffix.as_str()) {
                base = without;
            }
        }
    }
    Ok(base.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(suffix: Option<&str>) -> Options {
        Options {
            suffix: suffix.map(str::to_owned),
        }
    }

    #[test]
    fn returns_last_path_component() {
        assert_eq!(call("/usr/bin/sort", &Options::default()).unwrap(), "sort");
        assert_eq!(call("a/b/c", &Options::default()).unwrap(), "c");
        assert_eq!(call("foo.txt", &Options::default()).unwrap(), "foo.txt");
    }

    #[test]
    fn strips_trailing_slashes_first() {
        assert_eq!(call("/usr/bin/", &Options::default()).unwrap(), "bin");
        assert_eq!(call("foo/", &Options::default()).unwrap(), "foo");
    }

    #[test]
    fn all_slash_paths_yield_root() {
        assert_eq!(call("/", &Options::default()).unwrap(), "/");
        assert_eq!(call("///", &Options::default()).unwrap(), "/");
        assert_eq!(call("//", &Options::default()).unwrap(), "/");
    }

    #[test]
    fn dot_entries_pass_through() {
        assert_eq!(call(".", &Options::default()).unwrap(), ".");
        assert_eq!(call("..", &Options::default()).unwrap(), "..");
    }

    #[test]
    fn empty_path_yields_empty() {
        assert_eq!(call("", &Options::default()).unwrap(), "");
    }

    #[test]
    fn strips_matching_suffix() {
        assert_eq!(call("foo.txt", &opts(Some(".txt"))).unwrap(), "foo");
        assert_eq!(call("foo.o", &opts(Some(".o"))).unwrap(), "foo");
        assert_eq!(call("foobar", &opts(Some("bar"))).unwrap(), "foo");
    }

    #[test]
    fn keeps_name_when_suffix_does_not_match() {
        assert_eq!(call("foo.txt", &opts(Some(".log"))).unwrap(), "foo.txt");
    }

    #[test]
    fn keeps_name_when_suffix_equals_full_name() {
        assert_eq!(call("foo", &opts(Some("foo"))).unwrap(), "foo");
        assert_eq!(call(".txt", &opts(Some(".txt"))).unwrap(), ".txt");
    }

    // --- Cases mapped from uutils tests/by-util/test_basename.rs ---
    // CLI-only cases (missing/extra operands, -a/-s/-z flag forms,
    // quoting style, exit codes, non-UTF-8 argv) don't apply to this
    // single-operand `&str` function: -a is a caller loop, -s maps to
    // Options.suffix, -z is output formatting, argv decoding is the
    // CLI's job.

    #[test]
    fn strips_multiple_trailing_slashes() {
        // uutils test_trailing_separators.
        assert_eq!(call("d/f//", &Options::default()).unwrap(), "f");
        assert_eq!(call("///a///", &Options::default()).unwrap(), "a");
        assert_eq!(call("foo/bar///", &Options::default()).unwrap(), "bar");
    }

    #[test]
    fn trailing_dot_component_is_kept() {
        // uutils test_trailing_separators + test_trailing_dot: a
        // trailing "." is a real name, never stripped.
        assert_eq!(call("foo/./", &Options::default()).unwrap(), ".");
        assert_eq!(call("foo/.//", &Options::default()).unwrap(), ".");
        assert_eq!(call("/.", &Options::default()).unwrap(), ".");
        assert_eq!(call("hello/.", &Options::default()).unwrap(), ".");
        assert_eq!(call("/foo/bar/.", &Options::default()).unwrap(), ".");
    }

    #[test]
    fn double_slash_collapses_on_linux() {
        // uutils test_double_slash. coreutils resolves `//` vs `/`
        // per-platform; on Linux `//` collapses to `/`, which is what
        // this pure-string function returns unconditionally.
        assert_eq!(call("//", &opts(Some("/"))).unwrap(), "/");
        assert_eq!(call("//", &opts(Some("//"))).unwrap(), "/");
    }

    #[test]
    fn suffix_pairs() {
        // uutils test_simple_format (suffix passed positionally).
        assert_eq!(call("aa", &opts(Some("a"))).unwrap(), "a");
        assert_eq!(call("ba", &opts(Some("a"))).unwrap(), "b");
        assert_eq!(call("ab", &opts(Some("a"))).unwrap(), "ab");
        assert_eq!(call("a-a", &opts(Some("-a"))).unwrap(), "a");
        assert_eq!(call("a--help", &opts(Some("--help"))).unwrap(), "a");
        assert_eq!(call("a-h", &opts(Some("-h"))).unwrap(), "a");
        assert_eq!(call("f.s", &opts(Some(".s"))).unwrap(), "f");
        assert_eq!(call("a-s", &opts(Some("-s"))).unwrap(), "a");
        assert_eq!(call("a-z", &opts(Some("-z"))).unwrap(), "a");
        assert_eq!(call("fs", &opts(Some("s"))).unwrap(), "f");
    }

    #[test]
    fn suffix_applies_after_slash_stripping() {
        // uutils test_remove_suffix + test_zero_does_not_imply_multiple
        // (-z is output formatting; the suffix rule is the same).
        assert_eq!(call("fs/", &opts(Some("s"))).unwrap(), "f");
        assert_eq!(call("dir/file.suf", &opts(Some(".suf"))).unwrap(), "file");
        assert_eq!(
            call(
                "/usr/local/bin/reallylongexecutable.exe",
                &opts(Some(".exe"))
            )
            .unwrap(),
            "reallylongexecutable"
        );
        assert_eq!(call("foo.c", &opts(Some("c"))).unwrap(), "foo.");
    }

    #[test]
    fn suffix_equal_to_full_name_is_kept() {
        // uutils test_do_not_remove_suffix.
        assert_eq!(call("/foo/bar/baz", &opts(Some("baz"))).unwrap(), "baz");
    }

    #[test]
    fn empty_suffix_is_noop() {
        assert_eq!(call("fs", &opts(Some(""))).unwrap(), "fs");
    }

    #[test]
    fn suffix_with_slash_does_not_match() {
        // Trailing slashes are stripped from the path, never from the suffix.
        assert_eq!(call("fs/", &opts(Some("s/"))).unwrap(), "fs");
    }

    #[test]
    fn interior_and_leading_slash_runs() {
        // Probed against coreutils 9.10: interior runs collapse,
        // leading runs are skipped to reach the last component.
        assert_eq!(call("a//b", &Options::default()).unwrap(), "b");
        assert_eq!(call("//a", &Options::default()).unwrap(), "a");
        assert_eq!(call("ab/", &Options::default()).unwrap(), "ab");
    }

    #[test]
    fn deep_path_and_etc_passwd() {
        // uutils test_directory + test_file.
        assert_eq!(
            call(
                "/root/alpha/beta/gamma/delta/epsilon/omega/",
                &Options::default()
            )
            .unwrap(),
            "omega"
        );
        assert_eq!(call("/etc/passwd", &Options::default()).unwrap(), "passwd");
    }

    #[test]
    fn emoji_names_pass_through() {
        // uutils test_emoji_handling.
        assert_eq!(
            call("/path/to/🦀.txt", &Options::default()).unwrap(),
            "🦀.txt"
        );
        assert_eq!(
            call("/🌍/path/to/🚀.exe", &Options::default()).unwrap(),
            "🚀.exe"
        );
        assert_eq!(
            call("/path/to/file🎯.emoji", &opts(Some(".emoji"))).unwrap(),
            "file🎯"
        );
    }
}
