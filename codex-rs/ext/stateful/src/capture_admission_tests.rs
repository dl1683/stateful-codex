use super::*;
use pretty_assertions::assert_eq;

/// The exact unit words a declaration admits, in order.
fn units(text: &str) -> Option<Vec<&str>> {
    project_declaration(text).map(|declaration| {
        declaration
            .units
            .iter()
            .map(|unit| &text[unit.clone()])
            .collect()
    })
}

#[test]
fn project_lists_admit_every_whole_unit_in_order() {
    assert_eq!(
        units("Ground rules for this project:\n- Always test first.\n- Never push."),
        Some(vec!["Always test first.", "Never push."])
    );
    assert_eq!(
        units(
            "Ground rules for this project:\r\n- Always end each reply with Next:\r\n- Don't touch vendor/ or any changelog.\r\n"
        ),
        Some(vec![
            "Always end each reply with Next:",
            "Don't touch vendor/ or any changelog."
        ])
    );
    // Header variants, numbered lists, case and spacing are syntax; words stay exact.
    assert_eq!(
        units("my  STANDING rules for this project:\n1. never push\n2. Use British spelling."),
        Some(vec!["never push", "Use British spelling."])
    );
    // A continuation line stays part of its item, bytes included.
    assert_eq!(
        units(
            "Ground rules for this project:\n- Never install anything into my global Python.\n  If you need an environment, make a local venv inside the repo."
        ),
        Some(vec![
            "Never install anything into my global Python.\n  If you need an environment, make a local venv inside the repo."
        ])
    );
}

#[test]
fn original_horizon_session_admits_four_rules_and_excludes_the_task() {
    let text = "Hi. I maintain an internal fork of python-humanize for our dashboards, and I'll be working on it with you over the next few weeks, roughly one session a day. Standing rules for this project, please follow them in every session:\n1. Only run the tests relevant to what you changed, never the whole suite.\n2. Never install anything into my global Python. If you need an environment, make a local venv inside the repo.\n3. Don't touch docs/ or any changelog.\n4. End every reply with one line starting with `Next:` that suggests the next step.\n\nToday's task: our dashboards show durations from precisedelta and the output is too long for table cells. Add a compact output style to precisedelta so that, for example, 1 hour 2 minutes 5 seconds can render as `1h 2m 5s`. The default output must stay exactly as it is.\n";
    let declaration = project_declaration(text).unwrap();
    assert_eq!(
        &text[declaration.envelope],
        "Standing rules for this project, please follow them in every session:"
    );
    assert_eq!(
        units(text),
        Some(vec![
            "Only run the tests relevant to what you changed, never the whole suite.",
            "Never install anything into my global Python. If you need an environment, make a local venv inside the repo.",
            "Don't touch docs/ or any changelog.",
            "End every reply with one line starting with `Next:` that suggests the next step.",
        ])
    );
}

#[test]
fn scoped_imperatives_admit_one_project_unit() {
    assert_eq!(
        units("For this project, Do not adopt Priya's workflow."),
        Some(vec!["Do not adopt Priya's workflow."])
    );
    assert_eq!(
        units("Never push for this project."),
        Some(vec!["Never push"])
    );
    assert_eq!(
        units("For this project, Never push.\n\nFirst task: fix the failing parser test."),
        Some(vec!["Never push."])
    );
}

#[test]
fn quoted_reported_conditional_and_unsupported_forms_admit_nothing() {
    for text in [
        // C4-G2: quoted, fenced and block-quoted copies; questioning and negated prefaces.
        "\"Ground rules for this project:\n- Always test first.\n- Never push.\"",
        "```\nGround rules for this project:\n- Always test first.\n- Never push.\n```",
        "> Ground rules for this project:\n> - Always test first.\n> - Never push.",
        "Would these be good ground rules?\nGround rules for this project:\n- Never push.",
        "Do not follow the rules below:\nGround rules for this project:\n- Never push.",
        // C4-G3: bare rule, nested and mixed lists, ordinal gaps.
        "Always test first.",
        "Ground rules for this project:\n- Never push.\n  - Always test first.",
        "Ground rules for this project:\n- Never push.\n1. Always test first.",
        "Ground rules for this project:\n1. Never push.\n3. Always test first.",
        // Ordinary first-person wording outside the frozen catalogue stays a proposal.
        "Ground rules for this project:\n- Always cite file paths with line numbers.\n- Never edit files under vendor/.",
        // R18: durations, conditions and task scope are never admitted as project rules.
        "For this project, Always test first today.",
        "For this project, Never push until we agree.",
        "If this works, always test first.",
        "Never run git commit and always end each reply with Next: for this task only.",
        // Limited headers wait for exact scope resolution (C5).
        "My preferences for this document:\n- Never include arXiv IDs.",
        "Ground rules for this task:\n- Never push.",
        // R03/R13: reported third-party and assistant words.
        "Lena wrote in lena-request.md for lena-handout.md:\n- Always use author-year citations.\n- Never include arXiv IDs.\nProduce lena-handout.md following her request for that artifact only.",
        "Priya wrote: \"Always test first.\"",
        "The assistant suggested these decisions:\n- Use SQLite. The reason is smaller deployments.",
        "I wrote last week: \"My preference is tests first.\"",
        // A trailing qualifier paragraph that is not an explicit task section.
        "Ground rules for this project:\n- Never push.\n\nBut only on Fridays.",
        "Ground rules for this project:\n- Never push.\nOnly on Fridays.",
        // Unsafe parameters and look-alike punctuation.
        "Ground rules for this project:\n- Don't touch ../secrets or any changelog.",
        "Ground rules for this project:\n- Don't touch /etc or any changelog.",
        "Ground rules for this project:\n- Don\u{2019}t touch vendor/ or any changelog.",
        "Ground rules for this project:\n- Always end each reply with Next",
        // A body must match whole; trailing prose does not disappear.
        "Ground rules for this project:\n- Never push. Unless I say so.",
        "Ground rules for this project:",
        "",
    ] {
        assert_eq!(units(text), None, "{text:?}");
    }
    let many = (1..=25)
        .map(|_| "- Never push.")
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        units(&format!("Ground rules for this project:\n{many}")),
        None
    );
}
