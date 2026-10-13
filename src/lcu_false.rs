//! lcu-false: dependency-free core for `false`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Options for `false`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Options;

/// Error type for `false`.
///
/// `false` always exits with failure and prints nothing, so this enum
/// carries no diagnostics: `call` returning it *is* the failing status.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// `false` exits with status 1 and no diagnostic.
    Failure,
}

/// Entry point for `false`. Explicit params only, no implicit state.
///
/// `false` always fails: `call` returns `Err(Error::Failure)`, which
/// the caller maps to exit status 1 (uutils test_false.rs:
/// test_no_args). Ignored operands, `--help`, and `--version` are
/// argv concerns and never reach this core.
pub fn call(_options: &Options) -> Result<(), Error> {
    Err(Error::Failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_fails() {
        assert_eq!(call(&Options), Err(Error::Failure));
    }
}
