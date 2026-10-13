//! lcu-tac: dependency-free core for `tac`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Error type for `tac` (variants to be filled in per-util).
#[derive(Debug)]
pub enum Error {
    /// Placeholder until real port lands; keeps stub compilable.
    Todo,
}

/// Options for `tac` (fields to be filled in per-util).
#[derive(Debug, Default)]
pub struct Options {
    /// Placeholder until real port lands; keeps stub constructible.
    pub _placeholder: bool,
}

/// Entry point for `tac`. Explicit params only, no implicit state.
pub fn call(_options: &Options) -> Result<(), Error> {
    todo!("port from uutils behavior")
}
