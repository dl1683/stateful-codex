//! Exact conversation-history reads through the public app-server: every read checks current
//! memory eligibility when it runs, so once a Forget commits no history read of a later turn
//! returns the user message that stated the forgotten memory; reads are bounded per call and
//! per turn.
use super::capture_boundaries_tests::DECISION;
use super::capture_boundaries_tests::call;
use super::capture_boundaries_tests::message;
use super::capture_boundaries_tests::mount;
use super::capture_boundaries_tests::turn;
use super::memory_reads_tests::USER_WORDS;
use super::memory_reads_tests::outputs;
use super::memory_reads_tests::project_with_notes;
use super::memory_reads_tests::remember;
use super::memory_reads_tests::script;
use super::*;
use codex_app_server_protocol::SortDirection;
use codex_app_server_protocol::ThreadTurnsListParams;
use codex_app_server_protocol::ThreadTurnsListResponse;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;

const CONTROL: &str = "Record what the release checklist says.";

/// The history reads one turn makes; `phase` keeps call IDs unique across turns.
fn history_reads(
    phase: &str,
    thread: &str,
    [decision, rule, control]: [&str; 3],
) -> Vec<(String, String, Value)> {
    let exact = |turn: &str| json!({"threadId": thread, "turnId": turn, "part": "user"});
    [
        ("threads", json!({})),
        ("turns", json!({"threadId": thread})),
        ("decision", exact(decision)),
        ("rule", exact(rule)),
        ("control", exact(control)),
    ]
    .into_iter()
    .map(|(id, args)| {
        (
            format!("{phase}-{id}"),
            "conversation_read".to_string(),
            args,
        )
    })
    .chain([(format!("{phase}-end"), String::new(), Value::Null)])
    .collect()
}

#[tokio::test]
async fn history_reads_withhold_forgotten_user_messages_from_the_next_turn() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (home, root) = (TempDir::new()?, TempDir::new()?);
    let (mut server, project, thread) = project_with_notes(&home, &root, &responses_server).await?;
    let script_reads = Arc::new(Mutex::new(Vec::new()));
    let calls = mount(&responses_server, script(script_reads.clone())).await;
    let (decision, revision, rule) = remember(&mut server, &project, &thread).await?;
    let listed: ThreadTurnsListResponse = server
        .request(|request_id| ClientRequest::ThreadTurnsList {
            request_id,
            params: ThreadTurnsListParams {
                thread_id: thread.clone(),
                cursor: None,
                limit: Some(10),
                sort_direction: Some(SortDirection::Asc),
                items_view: None,
            },
        })
        .await?;
    let turns = listed
        .data
        .iter()
        .map(|turn| turn.id.as_str())
        .collect::<Vec<_>>();
    let turns: [&str; 3] = turns.try_into().expect("three remembered turns");
    let before = history_reads("before", &thread, turns);
    let after = history_reads("after", &thread, turns);
    *script_reads.lock().expect("targets lock") = [before.clone(), after.clone()].concat();

    // Before the Forget, history reads return the user's own messages exactly, attributed.
    let second = start_thread(&mut server, &project).await?;
    turn(&mut server, &second, &["What did we say earlier?"]).await?;
    let found = outputs(&calls.lock().expect("request log lock"), &before);
    let by_id = |found: &[(String, String)], wanted: &str| -> String {
        found
            .iter()
            .find(|(id, _)| id == wanted)
            .expect(wanted)
            .1
            .clone()
    };
    let page = |found: &[(String, String)], wanted: &str| -> Value {
        serde_json::from_str(&by_id(found, wanted)).expect("json page")
    };
    assert_eq!(
        ["decision", "rule", "control"].map(|id| {
            let read = page(&found, &format!("before-{id}"));
            (read["text"].clone(), read["by"].clone())
        }),
        [
            DECISION,
            "Ground rules for this project:\n- Never push.",
            CONTROL
        ]
        .map(|text| (json!(text), json!("user")))
    );
    let listing = page(&found, "before-turns");
    assert_eq!(listing["turnsWithheld"], json!(0));
    assert!(by_id(&found, "before-threads").contains("SQLite"));

    // The user forgets the rule and the decision. Every history read of a later turn
    // withholds the messages that stated them; the unrelated turn stays readable.
    for (entry_id, expected_revision) in [(rule, 1), (decision, revision)] {
        let _: StatefulMemoryForgetResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryForget {
                request_id,
                params: StatefulMemoryForgetParams {
                    thread_id: second.clone(),
                    expected_project_id: project.clone(),
                    entry_id,
                    expected_revision,
                },
            })
            .await?;
    }
    let third = start_thread(&mut server, &project).await?;
    turn(&mut server, &third, &["What did we say earlier?"]).await?;
    let found = outputs(&calls.lock().expect("request log lock"), &after);
    for (id, output) in &found {
        for word in USER_WORDS {
            assert!(!output.contains(word), "{id} carried {word:?}: {output}");
        }
    }
    let listing = page(&found, "after-turns");
    assert_eq!(
        (
            listing["turnsWithheld"].clone(),
            listing["turns"][0]["user"].clone()
        ),
        (json!(2), json!(CONTROL))
    );
    for id in ["after-decision", "after-rule"] {
        assert!(
            by_id(&found, id).starts_with("that turn part is not readable"),
            "{id}"
        );
    }
    assert_eq!(page(&found, "after-control")["text"], json!(CONTROL));
    Ok(())
}

/// A 20 KB user message pages within the 8,192-byte read envelope; one turn's history reads
/// stop at the 32,768-byte turn allowance, and the next turn reads again.
#[tokio::test]
async fn history_reads_are_bounded_per_call_and_per_turn() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let (home, root) = (TempDir::new()?, TempDir::new()?);
    let (mut server, project, thread) = project_with_notes(&home, &root, &responses_server).await?;
    let reads = Arc::new(Mutex::new(vec![("end".to_string(), Value::Null)]));
    let scripted = reads.clone();
    let calls = mount(
        &responses_server,
        Box::new(
            move |sequence, _| match scripted.lock().expect("reads lock").get(sequence) {
                Some((id, args)) if !args.is_null() => call(id, "conversation_read", args.clone()),
                _ => message("Done."),
            },
        ),
    )
    .await;
    let long = (0..400)
        .map(|index| format!("Section {index}: keep \"quoted\" text.\n"))
        .collect::<String>();
    let first = server
        .start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread.clone(),
            input: vec![UserInput::Text {
                text: long.clone(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?
        .turn
        .id;
    let exact = json!({"threadId": thread, "turnId": first, "part": "user"});
    let ids = [
        "page-1", "page-2", "page-3", "page-4", "page-5", "end-1", "later", "end-2",
    ];
    *reads.lock().expect("reads lock") = [("end".to_string(), Value::Null)]
        .into_iter()
        .chain(ids.iter().map(|id| {
            let args = if id.starts_with("end") {
                Value::Null
            } else {
                exact.clone()
            };
            (id.to_string(), args)
        }))
        .collect();
    let second = start_thread(&mut server, &project).await?;
    turn(&mut server, &second, &["Read the brief."]).await?;
    turn(&mut server, &second, &["Read it again."]).await?;

    let bodies = calls.lock().expect("request log lock").clone();
    let output = |id: &str| -> String {
        let call = (id.to_string(), "conversation_read".to_string(), Value::Null);
        outputs(&bodies, &[call]).remove(0).1
    };
    for id in ["page-1", "page-2", "page-3", "page-4", "later"] {
        let text = output(id);
        let page: Value = serde_json::from_str(&text).expect("json page");
        assert!(
            (6_000..=8_192).contains(&text.len()),
            "{id}: {} bytes",
            text.len()
        );
        assert_eq!(
            (page["offset"].clone(), page["totalBytes"].clone()),
            (json!(0), json!(long.len()))
        );
        assert!(long.starts_with(page["text"].as_str().expect("page text")));
        assert!(page["nextOffset"].as_u64().is_some(), "{id}");
    }
    let refused = output("page-5");
    assert!(
        refused.contains("32768-byte conversation history allowance") && refused.len() <= 2_048,
        "{refused}"
    );
    Ok(())
}
