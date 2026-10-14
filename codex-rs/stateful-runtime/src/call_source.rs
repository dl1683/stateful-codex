use sha2::Digest;
use sha2::Sha256;

/// Maps a non-empty raw call ID to deterministic, bounded, collision-resistant provenance.
/// Ordinary store-valid IDs remain verbatim; other IDs use a domain-separated digest.
pub fn call_source(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    const PREFIX: &str = "stateful:call:v1:sha256:";
    if raw.len() <= 512
        && raw.trim() == raw
        && !raw.chars().any(char::is_control)
        && !raw.starts_with(PREFIX)
    {
        return Some(raw.to_string());
    }
    let mut hash = Sha256::new();
    hash.update(b"stateful-call-provenance-v1\0");
    hash.update(raw.as_bytes());
    Some(format!("{PREFIX}{:x}", hash.finalize()))
}
