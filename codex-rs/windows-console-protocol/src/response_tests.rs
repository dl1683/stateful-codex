use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut result = (payload.len() as u32).to_le_bytes().to_vec();
    result.extend_from_slice(payload);
    result
}

#[test]
fn all_response_variants_roundtrip_without_losing_exit_or_error_bits() {
    let nonce = [7; 16];
    let mut events = vec![
        ResponseEvent::Ready {
            worker_pid: u32::MAX,
        },
        ResponseEvent::Spawned { target_pid: 1 },
        ResponseEvent::RootExited { exit_code: 0 },
        ResponseEvent::RootExited {
            exit_code: u32::MAX,
        },
    ];
    for stage in [
        FailureStage::Bootstrap,
        FailureStage::Console,
        FailureStage::HandleAdmission,
        FailureStage::Spawn,
        FailureStage::Control,
        FailureStage::Internal,
    ] {
        events.push(ResponseEvent::Failed {
            stage,
            win32_code: u32::MAX,
        });
    }
    for event in events {
        let response = Response { nonce, event };
        let encoded = encode_response(&response, &nonce).unwrap();
        assert_eq!(decode_response(&encoded, &nonce).unwrap(), response);
        assert_eq!(
            u32::from_le_bytes(encoded[..4].try_into().unwrap()) as usize,
            encoded.len() - 4
        );
    }
    let ready = Response {
        nonce,
        event: ResponseEvent::Ready { worker_pid: 123 },
    };
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&encode_response(&ready, &nonce).unwrap()[4..])
            .unwrap(),
        json!({
            "type":"ready", "version":1, "nonce":nonce, "workerPid":123,
        })
    );
}

#[test]
fn malformed_framing_is_refused_before_payload_parsing() {
    let nonce = [7; 16];
    let valid = encode_response(
        &Response {
            nonce,
            event: ResponseEvent::RootExited { exit_code: 0 },
        },
        &nonce,
    )
    .unwrap();
    for length in 0..valid.len() {
        assert_eq!(
            decode_response(&valid[..length], &nonce),
            Err(ResponseError::Framing)
        );
    }
    for mut bad in [
        vec![0; 4],
        (1_048_577_u32).to_le_bytes().to_vec(),
        u32::MAX.to_le_bytes().to_vec(),
        valid,
    ] {
        bad.push(0);
        assert_eq!(decode_response(&bad, &nonce), Err(ResponseError::Framing));
    }
}

#[test]
fn strict_shapes_duplicates_directions_versions_and_integer_types_are_refused() {
    let nonce = [7; 16];
    let valid = json!({"version":1,"type":"ready","nonce":nonce,"workerPid":1});
    let mut cases = vec![
        json!([1, "ready", nonce, 1]),
        json!("private payload"),
        json!({"type":{"ready":null}}),
    ];
    for (field, value) in [
        ("version", json!(2)),
        ("version", json!(1.0)),
        ("type", json!("request")),
        ("workerPid", json!(-1)),
        ("workerPid", json!(1.0)),
        ("workerPid", json!(4294967296_u64)),
        ("workerPid", json!(null)),
        ("nonce", json!(vec![7; 15])),
        ("unknown", json!("private payload")),
        ("targetPid", json!(1)),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        cases.push(bad);
    }
    let failed = json!({"version":1,"type":"failed","nonce":nonce,"stage":"spawn","win32Code":1});
    for (field, value) in [
        ("stage", json!({"spawn":null})),
        ("stage", json!("unknown")),
        ("win32Code", json!(-1)),
        ("win32Code", json!(1.0)),
        ("win32Code", json!(4294967296_u64)),
    ] {
        let mut bad = failed.clone();
        bad[field] = value;
        cases.push(bad);
    }
    for bad in cases {
        assert_eq!(
            decode_response(&frame(&serde_json::to_vec(&bad).unwrap()), &nonce),
            Err(ResponseError::Payload)
        );
    }
    let raw = serde_json::to_string(&valid).unwrap();
    for field in [
        "\"version\":1",
        "\"nonce\":[7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7]",
        "\"type\":\"ready\"",
        "\"workerPid\":1",
    ] {
        let duplicate = format!("{},{field}}}", &raw[..raw.len() - 1]);
        assert_eq!(
            decode_response(&frame(duplicate.as_bytes()), &nonce),
            Err(ResponseError::Payload)
        );
    }
    for bytes in [b"{broken private payload".as_slice(), b"\xff"] {
        let error = decode_response(&frame(bytes), &nonce).unwrap_err();
        assert_eq!(error, ResponseError::Payload);
        assert!(!error.to_string().contains("private"));
    }
}

#[test]
fn correlation_and_nonzero_pid_admission_apply_to_both_directions() {
    let nonce = [7; 16];
    for event in [
        ResponseEvent::Ready { worker_pid: 0 },
        ResponseEvent::Spawned { target_pid: 0 },
    ] {
        assert_eq!(
            encode_response(&Response { nonce, event }, &nonce),
            Err(ResponseError::Admission)
        );
    }
    for tag in ["ready", "spawned"] {
        let mut body = json!({"version":1,"type":tag,"nonce":nonce});
        body[if tag == "ready" {
            "workerPid"
        } else {
            "targetPid"
        }] = json!(0);
        assert_eq!(
            decode_response(&frame(&serde_json::to_vec(&body).unwrap()), &nonce),
            Err(ResponseError::Admission)
        );
    }
    let response = Response {
        nonce,
        event: ResponseEvent::RootExited { exit_code: 0 },
    };
    assert_eq!(
        encode_response(&response, &[8; 16]),
        Err(ResponseError::Admission)
    );
    assert_eq!(
        decode_response(&encode_response(&response, &nonce).unwrap(), &[8; 16]),
        Err(ResponseError::Admission)
    );
}
