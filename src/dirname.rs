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
/// handle that; this matches GNU on Linux).
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

    #[test]
    fn returns_parent_directory() {
        // gnu/tests/misc/dirname.pl cases a, b
        assert_eq!(call("d/f").unwrap(), "d");
        assert_eq!(call("/d/f").unwrap(), "/d");
    }

    #[test]
    fn strips_trailing_slashes_first() {
        // pl cases c, d
        assert_eq!(call("d/f/").unwrap(), "d");
        assert_eq!(call("d/f//").unwrap(), "d");
    }

    #[test]
    fn bare_name_yields_dot() {
        // pl case e
        assert_eq!(call("f").unwrap(), ".");
    }

    #[test]
    fn empty_path_yields_dot() {
        // pl case m
        assert_eq!(call("").unwrap(), ".");
    }

    #[test]
    fn root_yields_root() {
        // pl cases f, h
        assert_eq!(call("/").unwrap(), "/");
        assert_eq!(call("///").unwrap(), "/");
    }

    #[test]
    fn slash_file_yields_root() {
        assert_eq!(call("/f").unwrap(), "/");
    }

    #[test]
    fn gnu_preserves_leading_slashes() {
        // pl cases k, l
        assert_eq!(call("///a///b").unwrap(), "///a");
        assert_eq!(call("///a//b/").unwrap(), "///a");
    }

    #[test]
    fn gnu_double_slash_collapses_on_linux() {
        // pl cases g, i, j. GNU computes // vs / per-platform via
        // stat; on Linux (this machine) // collapses to /, which is
        // what this pure-string function returns unconditionally.
        assert_eq!(call("//").unwrap(), "/");
        assert_eq!(call("//a//").unwrap(), "/");
        assert_eq!(call("///a///").unwrap(), "/");
    }

    #[test]
    fn gnu_name_with_space() {
        // pl q-lit/q-shell/q-invalid: quoting style only affects
        // display, not the returned string.
        assert_eq!(call("q name/f").unwrap(), "q name");
    }

    #[test]
    fn gnu_slash_runs() {
        // Probed against GNU 9.10.
        assert_eq!(call("a//b").unwrap(), "a");
        assert_eq!(call("/a/").unwrap(), "/");
        assert_eq!(call("//a").unwrap(), "/");
        assert_eq!(call("ab/").unwrap(), ".");
        assert_eq!(call("a/").unwrap(), ".");
    }
}
