//! lcu-dir: dependency-free core for `dir`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Error type for `dir` (variants to be filled in per-util).
#[derive(Debug)]
pub enum Error {
    /// Placeholder until real port lands; keeps stub compilable.
    Todo,
}

/// Options for `dir` (fields to be filled in per-util).
#[derive(Debug, Default)]
pub struct Options {
    /// Placeholder until real port lands; keeps stub constructible.
    pub _placeholder: bool,
}

/// Entry point for `dir`. Explicit params only, no implicit state.
pub fn call(_options: &Options) -> Result<(), Error> {
    todo!("port from uutils behavior")
}
