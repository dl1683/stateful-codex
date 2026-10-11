use pretty_assertions::assert_eq;

use super::*;

fn event(payload: serde_json::Value) -> String {
    serde_json::json!({ "timestamp": "2026-10-11T00:00:00Z", "type": "event_msg", "payload": payload })
        .to_string()
}

fn command(call: &str, script: &str, exit_code: i32, output: &str) -> String {
    event(serde_json::json!({
        "type": "exec_command_end", "call_id": call, "turn_id": "t2",
        "command": ["bash", "-lc", script], "cwd": "file:///repo", "parsed_cmd": [],
        "stdout": output, "stderr": "", "aggregated_output": output, "exit_code": exit_code,
        "duration": { "secs": 0, "nanos": 0 }, "formatted_output": output, "status": "completed",
    }))
}

#[test]
fn recorded_rollout_reduces_and_renders_offline() {
    let rollout = [
        event(serde_json::json!({ "type": "turn_started", "turn_id": "t1", "model_context_window": null })),
        event(serde_json::json!({ "type": "agent_message", "message": "Baseline: parse takes 4,055.5 ms.\nNext step: rerun the IntRange tests." })),
        r#"{"timestamp":"x","type":"response_item","payload":{"type":"message"}}"#.to_string(),
        "not json".to_string(),
        command("c1", "rg IntRange", 127, "bash: rg: command not found"),
        command("c2", "git grep -n IntRange", 0, ""),
        event(serde_json::json!({
            "type": "item_completed", "thread_id": "01a12949-4b25-7093-9b54-1aca80bea50c", "turn_id": "t3",
            "item": { "type": "UserMessage", "id": "u3",
                      "content": [{ "type": "text", "text": "Priya changed config.py; keep it." }] },
        })),
    ]
    .join("\n");
    let files = [OfflineFileWitness {
        relative_path: "config.py",
        before: Some("X = \".clickrc.toml\"\n"),
        after: "X = \"click.toml\"\n",
        after_line: 6,
    }];
    let input = OfflineCapsuleInput {
        project_id: "p",
        thread_id: "th",
        window_id: "w1",
        files: &files,
        count_tokens: Some(&|text: &str| text.len().div_ceil(4)),
    };
    let out = offline_capsule(&rollout, &input);
    assert_eq!((out.events, out.unreadable_lines), (5, 1));
    let capsule = out.capsule.expect("fits");
    for expected in [
        "next: rerun the IntRange tests. @s1",
        "route: unavailable rg - bash: rg: command not found | exit 127 @s2 -> works git grep -n IntRange @s3",
        "coverage: directives: root packet",
    ] {
        assert!(capsule.contains(expected), "{expected}\n{capsule}");
    }
    assert!(out.tokens <= 512 && capsule.len() <= 2048 && out.omitted == 0);
    assert!(
        capsule.contains("; attributed by user @s4: \"Priya changed config.py; keep it.\""),
        "{capsule}"
    );
    let projection = Projection::from_payload(&out.projection_json).expect("current grammar");
    let change = projection
        .candidates
        .iter()
        .find(|c| c.kind == CandidateKind::Change);
    assert_eq!(
        change.and_then(|c| c
            .text
            .as_deref()
            .map(|t| t.split(" [sha").next().unwrap_or_default())),
        Some(r#"L1: X = ".clickrc.toml" -> X = "click.toml""#)
    );
    assert!(change.is_some_and(|c| c.linked.is_some()));
}
