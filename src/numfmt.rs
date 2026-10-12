//! lcu-numfmt: dependency-free core for `numfmt`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Error type for `numfmt` (variants to be filled in per-util).
#[derive(Debug)]
pub enum Error {
    /// Placeholder until real port lands; keeps stub compilable.
    Todo,
}

/// Options for `numfmt` (fields to be filled in per-util).
#[derive(Debug, Default)]
pub struct Options {
    /// Placeholder until real port lands; keeps stub constructible.
    pub _placeholder: bool,
}

/// Entry point for `numfmt`. Explicit params only, no implicit state.
pub fn call(_options: &Options) -> Result<(), Error> {
    todo!("port from GNU/uutils behavior")
}
