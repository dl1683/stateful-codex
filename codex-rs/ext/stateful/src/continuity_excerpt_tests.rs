use pretty_assertions::assert_eq;

use super::quote_excerpt;

/// Answer limit of an older turn in the continuity record.
const ANSWER_LIMIT: usize = 640;

/// The shown parts of a split excerpt and the byte counts its markers state.
#[derive(Debug, PartialEq)]
struct Split {
    head: String,
    omitted: usize,
    tail: String,
    trailing_omitted: Option<usize>,
    total: usize,
}

fn split(excerpt: &str) -> Split {
    let (head, rest) = excerpt.split_once(" [... ").expect("middle marker");
    let (omitted, rest) = rest.split_once(" of ").expect("omitted count");
    let (total, tail) = rest.split_once(" bytes omitted ...] ").expect("total");
    let (tail, trailing_omitted) = match tail.split_once(" [... last ") {
        Some((tail, trailing)) => {
            let (count, _) = trailing.split_once(" of ").expect("trailing count");
            (tail, Some(count.parse().expect("trailing number")))
        }
        None => (tail, None),
    };
    Split {
        head: serde_json::from_str(head).expect("quoted head"),
        omitted: omitted.parse().expect("omitted number"),
        tail: serde_json::from_str(tail).expect("quoted tail"),
        trailing_omitted,
        total: total.parse().expect("total number"),
    }
}

/// The excerpt shows a prefix and an inner run of `text`, and its markers account for
/// every other byte.
fn assert_truthful(text: &str, excerpt: &str) -> Split {
    let parts = split(excerpt);
    let tail_start = parts.head.len() + parts.omitted;
    assert_eq!(parts.total, text.len());
    assert!(text.starts_with(&parts.head), "{excerpt}");
    assert_eq!(
        &text[tail_start..tail_start + parts.tail.len()],
        parts.tail.as_str()
    );
    assert_eq!(
        tail_start + parts.tail.len() + parts.trailing_omitted.unwrap_or(0),
        text.len()
    );
    parts
}

/// A shortened excerpt is never longer than a head-only quote at the same limit.
fn assert_bounded(text: &str, excerpt: &str, limit: usize) {
    let bound = limit + format!(" [shortened at 0 of {} bytes]", text.len()).len();
    assert!(excerpt.len() <= bound, "{} > {bound}", excerpt.len());
}

#[test]
fn a_trailing_next_step_sentence_survives_shortening_whole() {
    let next = "Next step: add coverage for merging nested values across layers and resolving \
                conflicts with default_map.";
    let body = "Implemented the layered config loader: env, file and CLI layers now merge \
                shallowly, and the loader reports which layer supplied each key. "
        .repeat(5);
    let answer = format!("{body}{next}");
    assert!(answer.len() > ANSWER_LIMIT + 60);

    let excerpt = quote_excerpt(&answer, ANSWER_LIMIT);

    assert_bounded(&answer, &excerpt, ANSWER_LIMIT);
    let parts = assert_truthful(&answer, &excerpt);
    assert!(parts.tail.ends_with(next), "{excerpt}");
    assert_eq!(parts.trailing_omitted, None);
    assert!(parts.head.len() > parts.tail.len(), "{excerpt}");
}

#[test]
fn a_next_step_line_in_the_middle_of_the_tail_budget_is_kept_with_what_follows() {
    let answer = format!(
        "{}\n**Next:** run the full suite.\nThanks!",
        "Detail line about the work done so far.\n".repeat(30)
    );

    let parts = assert_truthful(&answer, &quote_excerpt(&answer, 300));

    assert!(
        parts
            .tail
            .ends_with("\n**Next:** run the full suite.\nThanks!"),
        "{parts:?}"
    );
}

#[test]
fn an_overlong_next_step_line_keeps_its_first_part() {
    let answer = format!(
        "{}Next step: {}",
        "Earlier context. ".repeat(60),
        "check every layer ".repeat(40)
    );

    let excerpt = quote_excerpt(&answer, ANSWER_LIMIT);

    assert_bounded(&answer, &excerpt, ANSWER_LIMIT);
    let parts = assert_truthful(&answer, &excerpt);
    assert!(parts.tail.starts_with("Next step: check every layer"));
    assert!(parts.trailing_omitted.is_some_and(|count| count > 0));
    assert!(!parts.head.is_empty());
}

#[test]
fn text_without_a_next_step_keeps_head_and_tail() {
    let answer = (0..200).map(|n| format!("w{n} ")).collect::<String>();

    let excerpt = quote_excerpt(&answer, ANSWER_LIMIT);

    assert_bounded(&answer, &excerpt, ANSWER_LIMIT);
    let parts = assert_truthful(&answer, &excerpt);
    assert!(parts.head.starts_with("w0 w1 "));
    assert!(parts.tail.ends_with("w198 w199 "));
    assert_eq!(parts.trailing_omitted, None);
    assert!(parts.head.len() > parts.tail.len());
}

#[test]
fn multibyte_text_is_cut_on_char_boundaries() {
    let answer = format!("{}Next step: vérifier €.", "é€😀<\"".repeat(200));

    let excerpt = quote_excerpt(&answer, 301);

    assert_bounded(&answer, &excerpt, 301);
    let parts = assert_truthful(&answer, &excerpt);
    assert!(parts.tail.ends_with("Next step: vérifier €."));
    assert!(!excerpt.contains('<'));
}

#[test]
fn text_within_the_limit_is_quoted_whole() {
    let answer = "Done. Next step: ship it.";

    assert_eq!(
        quote_excerpt(answer, ANSWER_LIMIT),
        serde_json::to_string(answer).expect("quote")
    );
}

#[test]
fn a_tiny_limit_falls_back_to_the_head_only_quote() {
    let answer = "x".repeat(700);

    assert_eq!(
        quote_excerpt(&answer, 0),
        "\"\" [shortened at 0 of 700 bytes]"
    );
}

#[test]
fn a_fitting_next_step_line_followed_by_an_appendix_is_kept_whole() {
    let line = format!("Next step: {}", "check layer ".repeat(38));
    let answer = format!(
        "{}{line}\n{}",
        "Done. ".repeat(200),
        "Appendix. ".repeat(60)
    );

    let excerpt = quote_excerpt(&answer, ANSWER_LIMIT);

    assert_bounded(&answer, &excerpt, ANSWER_LIMIT);
    let parts = assert_truthful(&answer, &excerpt);
    assert!(parts.tail.starts_with(&line), "{excerpt}");
    assert!(parts.trailing_omitted.is_some_and(|count| count > 0));
    assert!(parts.head.starts_with("Done. "), "{excerpt}");
}

#[test]
fn a_short_next_step_survives_a_small_limit() {
    let message = format!("{}Next step: run tests.", "Earlier context. ".repeat(60));

    let excerpt = quote_excerpt(&message, 96);

    assert_bounded(&message, &excerpt, 96);
    let parts = assert_truthful(&message, &excerpt);
    assert_eq!(parts.tail, "Next step: run tests.");
    assert!(parts.head.starts_with("Earlier"), "{excerpt}");
}

#[test]
fn a_word_merely_starting_with_next_step_is_not_a_next_step() {
    let answer = format!(
        "{}Next stepper diagnostics follow. {}",
        "Earlier context. ".repeat(60),
        "Diagnostic line. ".repeat(40)
    );

    let parts = assert_truthful(&answer, &quote_excerpt(&answer, ANSWER_LIMIT));

    assert!(!parts.tail.starts_with("Next stepper"), "{parts:?}");
    assert!(parts.tail.ends_with("Diagnostic line. "));
}
