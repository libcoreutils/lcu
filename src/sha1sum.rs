//! lcu-sha1sum: dependency-free core for `sha1sum`.
//! No process::exit, no implicit cwd/env reads, no stdout writes.

/// Error type for `sha1sum` (variants to be filled in per-util).
#[derive(Debug)]
pub enum Error {
    /// Placeholder until real port lands; keeps stub compilable.
    Todo,
}

/// Options for `sha1sum` (fields to be filled in per-util).
#[derive(Debug, Default)]
pub struct Options {
    /// Placeholder until real port lands; keeps stub constructible.
    pub _placeholder: bool,
}

/// Entry point for `sha1sum`. Explicit params only, no implicit state.
pub fn call(_options: &Options) -> Result<(), Error> {
    todo!("port from GNU/uutils behavior")
}
