//! Preflight OS arguments before clap clones or interprets them.
use std::ffi::OsString;

pub(super) fn bounded(arguments: impl Iterator<Item = OsString>) -> Result<Vec<String>, ()> {
    let mut result = Vec::new();
    let mut charged = 0usize;
    for argument in arguments {
        if result.len() == 32 {
            return Err(());
        }
        charged = charged
            .checked_add(argument.as_encoded_bytes().len())
            .and_then(|size| size.checked_add(/*rhs*/ 64))
            .ok_or(())?;
        if charged > 16 * 1024 {
            return Err(());
        }
        result.push(argument.to_str().ok_or(())?.to_owned());
    }
    Ok(result)
}

#[cfg(test)]
#[path = "arguments_tests.rs"]
mod tests;
