use pretty_assertions::assert_eq;

use super::StartupWarningDeduper;

#[test]
fn consumes_only_the_expected_number_of_exact_duplicates() {
    let warnings = vec![
        "duplicate warning".to_string(),
        "duplicate warning".to_string(),
        "different warning".to_string(),
    ];
    let mut deduper = StartupWarningDeduper::new(&warnings);

    let outcomes = [
        deduper.take_duplicate("duplicate warning"),
        deduper.take_duplicate("different warning"),
        deduper.take_duplicate("duplicate warning"),
        deduper.take_duplicate("duplicate warning"),
        deduper.take_duplicate("runtime warning"),
    ];

    assert_eq!(outcomes, [true, true, true, false, false]);
}
