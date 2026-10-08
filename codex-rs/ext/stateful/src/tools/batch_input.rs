//! Bounded discriminator for the proposal branch, before any owned JSON decoding.

use codex_extension_api::FunctionCallError;
use codex_extension_api::ToolCall;

const MAX_PROPOSAL_INPUT_BYTES: usize = 32768;

pub(super) fn preflight(call: &ToolCall<'_>) -> Result<(), FunctionCallError> {
    let input = call.function_arguments()?;
    if input.len() > MAX_PROPOSAL_INPUT_BYTES && has_discriminator(input) {
        return Err(super::bounded_respond(
            call,
            "source proposal exceeds the 32 KiB input bound; nothing written",
        ));
    }
    Ok(())
}

/// Recognizes the same top-level `type` presence as the batch decoder. Values,
/// including escaped strings and flat record arrays, are scanned in place. The
/// four discriminator characters can each be literal ASCII or a Unicode escape;
/// comparing those bytes needs no owned key or JSON parser scratch buffer.
/// Auxiliary allocation is independent of input size, key count and depth.
/// The existing decoder still owns syntax/type diagnostics for admitted input;
/// an oversized proposal is refused before inspecting its payload's validity.
fn has_discriminator(input: &str) -> bool {
    let input = input.trim_start();
    if !input.starts_with('{') {
        return false;
    }
    let bytes = input.as_bytes();
    let mut depth = 0usize;
    let mut key_expected = false;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                let start = index;
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                if index >= bytes.len() {
                    return false;
                }
                if depth == 1 && key_expected {
                    let mut key = bytes[start + 1..index].iter().copied();
                    let matches = b"type".iter().all(|expected| match key.next() {
                        Some(b'\\') => {
                            key.next() == Some(b'u')
                                && key.next() == Some(b'0')
                                && key.next() == Some(b'0')
                                && key.next() == Some(b'0' + (expected >> 4))
                                && key.next() == Some(b'0' + (expected & 15))
                        }
                        Some(byte) => byte == *expected,
                        None => false,
                    });
                    if matches && key.next().is_none() {
                        return true;
                    }
                    key_expected = false;
                }
            }
            b'{' | b'[' => {
                depth += 1;
                if depth == 1 {
                    key_expected = true;
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return false;
                }
            }
            b',' if depth == 1 => key_expected = true,
            _ => {}
        }
        index += 1;
    }
    false
}

#[cfg(test)]
#[path = "batch_input_tests.rs"]
mod tests;
