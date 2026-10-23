use codex_extension_api::RenderedWorldStateFragment;
use codex_protocol::models::ContentItem;

pub(crate) fn text(fragment: &RenderedWorldStateFragment) -> &str {
    let ContentItem::InputText { text } = fragment.body() else {
        panic!("Stateful world state must contain input text");
    };
    text
}

pub(crate) fn snapshot(
    section: &codex_extension_api::WorldStateSectionContribution,
) -> serde_json::Value {
    section
        .render_diff(codex_extension_api::PreviousWorldStateSection::Absent)
        .0
        .expect("section snapshot")
}

#[test]
fn comparison_snapshot_survives_a_suppressed_fragment() {
    use codex_extension_api::PreviousWorldStateSection;
    use codex_protocol::models::input_text;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    let captured = json!({ "revision": 7 });
    let section = super::world_state_section::fixed_world_state_section(
        "test",
        captured.clone(),
        |previous| match previous {
            PreviousWorldStateSection::Known(_) => None,
            PreviousWorldStateSection::Absent | PreviousWorldStateSection::Unknown => {
                Some(RenderedWorldStateFragment::new(
                    "developer",
                    ("[test]", "[/test]"),
                    input_text("bounded update"),
                ))
            }
        },
    );
    let (snapshot, fragment) = section.render_diff(PreviousWorldStateSection::Absent);
    assert_eq!(snapshot, Some(captured.clone()));
    assert_eq!(
        text(&fragment.expect("new section renders")),
        "bounded update"
    );
    assert_eq!(
        section.render_diff(PreviousWorldStateSection::Known(&captured)),
        (Some(captured), None)
    );
}
