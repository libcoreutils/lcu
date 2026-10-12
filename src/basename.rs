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
        assert_eq!(
            call("foo.txt", &opts(Some(".log"))).unwrap(),
            "foo.txt"
        );
    }

    #[test]
    fn keeps_name_when_suffix_equals_full_name() {
        assert_eq!(call("foo", &opts(Some("foo"))).unwrap(), "foo");
        assert_eq!(call(".txt", &opts(Some(".txt"))).unwrap(), ".txt");
    }
}
