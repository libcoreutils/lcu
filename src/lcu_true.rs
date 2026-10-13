//! lcu-true: dependency-free core for `true`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Options for `true`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Options;

/// Entry point for `true`. Explicit params only, no implicit state.
///
/// `true` always succeeds and produces no output (uutils
/// test_true.rs: test_no_args; operands and --help/--version forms
/// are argv-layer concerns that never reach this core).
pub fn call(_options: &Options) -> Result<(), Error> {
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_succeeds() {
        assert!(call(&Options).is_ok());
    }
}
