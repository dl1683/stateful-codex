use pretty_assertions::assert_eq;

use super::*;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn fixture() -> (Request, ReceiveContext) {
    let nonce = [3; 16];
    (
        Request {
            nonce,
            application: wide("C:\\trusted.exe"),
            command_line: wide("trusted.exe argument"),
            cwd: wide("C:\\project"),
            startup_desktop: wide("winsta0\\private"),
            environment_block: wide("A=B\0\0"),
            handles: HandleReferences {
                restricted_token: 1,
                inner_job: 2,
                stdin: 3,
                stdout: 4,
                stderr: 5,
            },
            output_mode: OutputMode::Separate,
        },
        ReceiveContext {
            nonce,
            width: WorkerWidth::Bits64,
            request_pipe: 6,
            status_pipe: 7,
        },
    )
}

fn raw(payload: &[u8]) -> Vec<u8> {
    let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(payload);
    frame
}

fn assert_refused(request: &Request, context: &ReceiveContext) {
    let payload = serde_json::to_vec(&serde_json::json!({"version":1,"type":"request","request":request})).unwrap();
    assert_eq!(
        encode_request(request, context),
        Err(ProtocolError::Admission)
    );
    assert_eq!(
        decode_request(&raw(&payload), context),
        Err(ProtocolError::Admission)
    );
}

fn assert_bad_payload(payload: &[u8], context: &ReceiveContext) {
    assert_eq!(decode_request(&raw(payload), context), Err(ProtocolError::Payload));
}

fn assert_roundtrip(request: &Request, context: &ReceiveContext) {
    let frame = encode_request(request, context).unwrap();
    assert_eq!(decode_request(&frame, context).unwrap(), *request);
}

#[test]
fn utf16_large_environment_and_frame_boundaries_round_trip_as_whole_objects() {
    let (mut request, context) = fixture();
    request
        .command_line
        .extend([0xd800, 0x092e, 0x0930, 0x093e, 0x0920, 0x0940]);
    request.environment_block.clear();
    for name in ["FIRST", "SECOND", "THIRD"] {
        request
            .environment_block
            .extend(wide(&format!("{name}={}\0", "x".repeat(/*n*/ 12000))));
    }
    request.environment_block.push(/*value*/ 0);
    assert_roundtrip(&request, &context);
    request.application = vec![u16::MAX; 32767];
    request.command_line = vec![u16::MAX; 32766];
    request.cwd = vec![u16::MAX; 32767];
    request.startup_desktop = vec![u16::MAX; 1024];
    request.environment_block = vec![u16::MAX; 65534];
    request.environment_block.extend([0, 0]);
    assert_roundtrip(&request, &context);
    request
        .environment_block
        .insert(/*index*/ 0, /*element*/ 65);
    assert_refused(&request, &context);
}

#[test]
fn invalid_buffers_refuse_on_both_sides() {
    let (request, context) = fixture();
    let mut cases = vec![];
    for field in ["application", "commandLine", "cwd", "startupDesktop"] {
        for value in [vec![], vec![0], vec![65; 32768]] {
            let mut object = serde_json::to_value(&request).unwrap();
            object[field] = serde_json::json!(value);
            cases.push(serde_json::from_value::<Request>(object).unwrap());
        }
    }
    for environment in [
        vec![],
        vec![0],
        vec![0, 0, 0],
        vec![0, 65, 0, 0],
        vec![65, 0, 0, 66, 0, 0],
        vec![65, 0, 66],
    ] {
        let mut invalid = request.clone();
        invalid.environment_block = environment;
        cases.push(invalid);
    }
    for invalid in cases {
        assert_refused(&invalid, &context);
    }
}

#[test]
fn roles_width_and_explicit_output_aliasing_are_enforced() {
    let (request, mut context) = fixture();
    let mut cases = vec![];
    for value in [0, u64::MAX, u64::from(u32::MAX), 2, 3, 6, 7] {
        let mut invalid = request.clone();
        invalid.handles.restricted_token = value;
        cases.push(invalid);
    }
    let mut invalid = request.clone();
    invalid.handles.stdin = invalid.handles.stdout;
    cases.push(invalid);
    let mut invalid = request.clone();
    invalid.handles.stderr = invalid.handles.stdout;
    cases.push(invalid);
    let mut invalid = request.clone();
    invalid.output_mode = OutputMode::Merged;
    cases.push(invalid);
    for invalid in cases {
        assert_refused(&invalid, &context);
    }
    let mut merged = request.clone();
    merged.handles.stderr = merged.handles.stdout;
    merged.output_mode = OutputMode::Merged;
    assert_roundtrip(&merged, &context);
    let mut wide_handle = request.clone();
    wide_handle.handles.restricted_token = 1_u64 << 32;
    let frame = encode_request(&wide_handle, &context).unwrap();
    context.width = WorkerWidth::Bits32;
    assert_eq!(
        encode_request(&wide_handle, &context),
        Err(ProtocolError::Admission)
    );
    assert_eq!(
        decode_request(&frame, &context),
        Err(ProtocolError::Admission)
    );
    assert_roundtrip(&request, &context);
    context.status_pipe = context.request_pipe;
    assert_refused(&request, &context);
}

#[test]
fn framing_truncation_oversize_and_nonce_refuse() {
    let (request, mut context) = fixture();
    let frame = encode_request(&request, &context).unwrap();
    for length in 0..frame.len() {
        assert!(decode_request(&frame[..length], &context).is_err());
    }
    let mut trailing = frame;
    trailing.push(/*value*/ 0);
    assert_eq!(
        decode_request(&trailing, &context),
        Err(ProtocolError::Framing)
    );
    for header in [0_u32.to_le_bytes(), u32::MAX.to_le_bytes()] {
        assert_eq!(
            decode_request(&header, &context),
            Err(ProtocolError::Framing)
        );
    }
    context.nonce[0] ^= 1;
    assert_refused(&request, &context);
}

#[test]
fn json_direction_version_duplicates_nested_unknowns_and_types_refuse() {
    let (request, context) = fixture();
    let body = serde_json::to_string(&request).unwrap();
    let envelope = serde_json::json!({"version":1,"type":"request","request":request});
    for (parent, key, value) in [
        ("", "version", serde_json::json!(2)),
        ("", "type", serde_json::json!("ready")),
        ("", "type", serde_json::json!({"request":null})),
        ("", "unknown", serde_json::json!(0)),
        ("/request", "unknown", serde_json::json!(0)),
        ("/request", "outputMode", serde_json::json!({"separate":null})),
        ("/request/handles", "unknown", serde_json::json!(0)),
        (
            "/request/handles",
            "restrictedToken",
            serde_json::json!(true),
        ),
    ] {
        let mut invalid = envelope.clone();
        invalid
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.to_string(), value);
        assert_bad_payload(&serde_json::to_vec(&invalid).unwrap(), &context);
    }
    let req = &envelope["request"];
    let handles = &req["handles"];
    for (path, array) in [
        ("", serde_json::json!([1,"request",req])),
        ("/request", serde_json::json!([req["nonce"],req["application"],req["commandLine"],req["cwd"],
            req["startupDesktop"],req["environmentBlock"],req["handles"],req["outputMode"]])),
        ("/request/handles", serde_json::json!([handles["restrictedToken"],handles["innerJob"],
            handles["stdin"],handles["stdout"],handles["stderr"]])),
    ] {
        let mut invalid = envelope.clone();
        *invalid.pointer_mut(path).unwrap() = array;
        assert_bad_payload(&serde_json::to_vec(&invalid).unwrap(), &context);
    }
    let duplicate =
        format!("{{\"version\":1,\"version\":1,\"type\":\"request\",\"request\":{body}}}");
    assert_bad_payload(duplicate.as_bytes(), &context);
    assert_bad_payload(&[0xff], &context);
}
