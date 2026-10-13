//! lcu-sha256sum: dependency-free core for `sha256sum`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Error type for `sha256sum` (variants to be filled in per-util).
#[derive(Debug)]
pub enum Error {
    /// Placeholder until real port lands; keeps stub compilable.
    Todo,
}

/// Options for `sha256sum` (fields to be filled in per-util).
#[derive(Debug, Default)]
pub struct Options {
    /// Placeholder until real port lands; keeps stub constructible.
    pub _placeholder: bool,
}

/// Entry point for `sha256sum`. Explicit params only, no implicit state.
pub fn call(_options: &Options) -> Result<(), Error> {
    todo!("port from uutils behavior")
}
