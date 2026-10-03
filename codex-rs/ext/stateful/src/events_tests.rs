use pretty_assertions::assert_eq;

use super::MAX_RECEIPT_TEXT_BYTES;
use super::receipt_text;

#[test]
fn long_receipts_end_at_a_word_boundary() {
    let text = "never touch the docs folder ".repeat(20);
    let receipt = receipt_text(&text);
    assert_eq!(
        (
            receipt.len() <= MAX_RECEIPT_TEXT_BYTES,
            receipt.ends_with("folder...")
                || receipt.ends_with("the...")
                || receipt.ends_with("never...")
                || receipt.ends_with("touch...")
                || receipt.ends_with("docs..."),
            receipt_text("short"),
        ),
        (true, true, "short".to_string())
    );
}
