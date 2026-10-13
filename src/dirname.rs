//! lcu-dirname: dependency-free core for `dirname`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Error type for `dirname`.
///
/// `dirname` is infallible over `&str` input: every path maps to a
/// directory name, so this enum has no variants.
#[derive(Debug)]
pub enum Error {}

/// Entry point for `dirname`. Explicit params only, no implicit state.
///
/// Returns the directory component of `path`: trailing slashes are
/// stripped, the last component is removed, and trailing slashes are
/// stripped again. Leading slashes are preserved verbatim. A bare name
/// or empty path yields `"."`, and an all-slash path yields `"/"`
/// (on platforms where `//` is distinct from `/`, the caller must
/// handle that; this matches coreutils on Linux).
pub fn call(path: &str) -> Result<String, Error> {
    if path.is_empty() {
        return Ok(".".to_owned());
    }
    let stripped = path.trim_end_matches('/');
    if stripped.is_empty() {
        return Ok("/".to_owned());
    }
    let Some(slash) = stripped.rfind('/') else {
        return Ok(".".to_owned());
    };
    if slash == 0 {
        return Ok("/".to_owned());
    }
    let parent = stripped[..slash].trim_end_matches('/');
    if parent.is_empty() {
        return Ok("/".to_owned());
    }
    Ok(parent.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Cases mapped from uutils tests/by-util/test_dirname.rs ---
    // CLI-only cases (missing operand, invalid option, -z flag form,
    // exit codes, non-UTF-8 argv) don't apply to this single-path
    // `&str` function: -z is output formatting, argv decoding is the
    // CLI's job.

    #[test]
    fn returns_parent_directory() {
        // uutils test_existing_behavior_preserved +
        // test_multiple_paths_comprehensive.
        assert_eq!(call("d/f").unwrap(), "d");
        assert_eq!(call("/d/f").unwrap(), "/d");
        assert_eq!(call("/home/dos").unwrap(), "/home");
        assert_eq!(call("/var/log").unwrap(), "/var");
        assert_eq!(call("relative/path").unwrap(), "relative");
    }

    #[test]
    fn strips_trailing_slashes_first() {
        // uutils test_path_with_trailing_slashes +
        // test_existing_behavior_preserved.
        assert_eq!(call("d/f/").unwrap(), "d");
        assert_eq!(call("d/f//").unwrap(), "d");
        assert_eq!(call("/home/dos/").unwrap(), "/home");
        assert_eq!(
            call("/root/alpha/beta/gamma/delta/epsilon/omega//").unwrap(),
            "/root/alpha/beta/gamma/delta/epsilon"
        );
    }

    #[test]
    fn bare_name_yields_dot() {
        // Probed against coreutils 9.10: a name with no slash.
        assert_eq!(call("f").unwrap(), ".");
    }

    #[test]
    fn empty_path_yields_dot() {
        // uutils test_empty (+ test_multiple_paths_comprehensive).
        assert_eq!(call("").unwrap(), ".");
    }

    #[test]
    fn root_yields_root() {
        // uutils test_root + test_trailing_dot ("/." -> "/").
        assert_eq!(call("/").unwrap(), "/");
        assert_eq!(call("///").unwrap(), "/");
        assert_eq!(call("/.").unwrap(), "/");
    }

    #[test]
    fn slash_file_yields_root() {
        // Probed against coreutils 9.10.
        assert_eq!(call("/f").unwrap(), "/");
    }

    #[test]
    fn dot_yields_dot() {
        // uutils test_pwd + test_multiple_paths_comprehensive.
        assert_eq!(call(".").unwrap(), ".");
    }

    #[test]
    fn preserves_leading_slashes() {
        // Probed against coreutils 9.10: leading runs are preserved
        // verbatim; only trailing slashes are stripped.
        assert_eq!(call("///a///b").unwrap(), "///a");
        assert_eq!(call("///a//b/").unwrap(), "///a");
    }

    #[test]
    fn double_slash_collapses_on_linux() {
        // coreutils resolves `//` vs `/` per-platform; on Linux `//`
        // collapses to `/`, which is what this pure-string function
        // returns unconditionally.
        assert_eq!(call("//").unwrap(), "/");
        assert_eq!(call("//a//").unwrap(), "/");
        assert_eq!(call("///a///").unwrap(), "/");
    }

    #[test]
    fn name_with_space() {
        // Quoting style only affects display, not the returned string.
        assert_eq!(call("q name/f").unwrap(), "q name");
    }

    #[test]
    fn slash_runs() {
        // Probed against coreutils 9.10.
        assert_eq!(call("a//b").unwrap(), "a");
        assert_eq!(call("/a/").unwrap(), "/");
        assert_eq!(call("//a").unwrap(), "/");
        assert_eq!(call("ab/").unwrap(), ".");
        assert_eq!(call("a/").unwrap(), ".");
    }

    #[test]
    fn trailing_dot_component_is_removed() {
        // uutils test_trailing_dot (-z is output formatting).
        assert_eq!(call("/home/dos/.").unwrap(), "/home/dos");
        assert_eq!(call("hello/.").unwrap(), "hello");
        assert_eq!(call("/foo/bar/baz/.").unwrap(), "/foo/bar/baz");
        assert_eq!(call("/tmp/.").unwrap(), "/tmp");
    }

    #[test]
    fn dot_slash_runs() {
        // uutils test_all_dot_slash_variations +
        // test_dot_slash_component_preservation +
        // test_trailing_dot_edge_cases: only trailing slashes are
        // stripped; interior "." components are kept verbatim.
        assert_eq!(call("foo//.").unwrap(), "foo");
        assert_eq!(call("foo///.").unwrap(), "foo");
        assert_eq!(call("foo/./").unwrap(), "foo");
        assert_eq!(call("foo/bar/./").unwrap(), "foo/bar");
        assert_eq!(call("foo/./bar").unwrap(), "foo/.");
        assert_eq!(call("a/./b").unwrap(), "a/.");
        assert_eq!(call("a/./b/./c").unwrap(), "a/./b/.");
        assert_eq!(call("foo/./bar/baz").unwrap(), "foo/./bar");
        assert_eq!(call("/path/./to/file").unwrap(), "/path/./to");
        assert_eq!(call("/home/dos//.").unwrap(), "/home/dos");
    }

    #[test]
    fn dotdot_is_not_resolved() {
        // uutils test_existing_behavior_preserved: `..` is just a name.
        assert_eq!(call("/home/dos/..").unwrap(), "/home/dos");
    }

    #[test]
    fn emoji_paths() {
        // uutils test_emoji_handling + test_trailing_dot_emoji
        // (-z is output formatting, not core behavior).
        assert_eq!(call("/🌍/path/to/🦀.txt").unwrap(), "/🌍/path/to");
        assert_eq!(call("/🎉/path/to/🚀/").unwrap(), "/🎉/path/to");
        assert_eq!(call("/🌍/path/.").unwrap(), "/🌍/path");
        assert_eq!(call("/🎉/🚀/.").unwrap(), "/🎉/🚀");
    }

    #[test]
    fn multiple_paths_each_map() {
        // uutils test_multiple_paths_comprehensive + test_trailing_dot
        // (multi-path invocation is a caller loop; each line is one
        // core call) + test_path_without_trailing_slashes.
        for (path, expected) in [
            ("/home/dos/.", "/home/dos"),
            ("/var/log", "/var"),
            (".", "."),
            ("/tmp/.", "/tmp"),
            ("", "."),
            ("/", "/"),
            ("relative/path", "relative"),
            (
                "/root/alpha/beta/gamma/delta/epsilon/omega",
                "/root/alpha/beta/gamma/delta/epsilon",
            ),
        ] {
            assert_eq!(call(path).unwrap(), expected, "dirname {path}");
        }
    }
}
