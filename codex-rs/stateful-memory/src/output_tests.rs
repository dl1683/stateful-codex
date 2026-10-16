use super::*;
use pretty_assertions::assert_eq;

#[test]
fn ascii_wire_roundtrips_bmp_supplementary_and_escaped_content() {
    let value = json!({"text": "\u{092e}\u{0930}\u{093e}\u{0920}\u{0940} \u{1f642}\nC:\\docs\\parcel.json\""});
    for style in [OutputStyle::Compact, OutputStyle::Indented] {
        let encoded = encode_reply(&value, style, MAX_REPLY_BYTES).unwrap();
        assert!(encoded.is_ascii());
        assert_eq!(serde_json::from_slice::<Value>(&encoded).unwrap(), value);
    }
}

#[test]
fn envelope_budget_includes_unicode_expansion_and_newline() {
    let mut text = String::new();
    loop {
        let value = json!({"text": text});
        if let Some(encoded) = encode_reply(&value, OutputStyle::Compact, MAX_REPLY_BYTES) {
            assert!(encoded.len() <= MAX_REPLY_BYTES);
            assert_eq!(serde_json::from_slice::<Value>(&encoded).unwrap(), value);
        } else {
            assert!(serde_json::to_vec(&value).unwrap().len() < MAX_REPLY_BYTES);
            break;
        }
        text.push('\u{092e}');
    }
}
